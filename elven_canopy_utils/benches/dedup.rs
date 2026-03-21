//! Dedup benchmarks: compare parallel_dedup against alternative dedup strategies
//! across different item types, input sizes, bucket counts, and hash functions.
//!
//! Run with: `cargo bench -p elven_canopy_utils`
//!
//! Strategies benchmarked:
//! - **parallel_dedup** with varying bucket counts (32, 64, 128, 256) and
//!   hash functions (std, ahash, fxhash).
//! - **par_sort_dedup**: `par_sort_unstable` + `dedup` — the primary baseline.
//! - **sequential_hashbrown**, **sort_dedup**, **hashset_collect**,
//!   **rayon_hashset**: additional baselines, only run at smaller sizes.
//!
//! Item types:
//! - `u64` — small, cheap to hash and compare.
//! - `[u64; 6]` — 48-byte tuple, more realistic for composite keys.
//! - `String` — heap-allocated, variable-cost hash and clone.
//!
//! Input sizes: 1k, 10k, 100k, 1M, 10M (strings cap at 1M).
//!
//! All inputs use ~50% duplication: values are drawn from 0..n/2.

use ahash::RandomState as AhashState;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use elven_canopy_utils::{parallel_dedup_with, sequential_dedup};
use rayon::prelude::*;
use rustc_hash::FxBuildHasher;
use std::collections::HashSet;
use std::hash::RandomState as StdState;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Input generators
// ---------------------------------------------------------------------------

fn gen_u64(count: usize) -> Vec<u64> {
    let unique = count / 2;
    (0..count).map(|i| (i % unique) as u64).collect()
}

fn gen_tuple6(count: usize) -> Vec<[u64; 6]> {
    let unique = count / 2;
    (0..count)
        .map(|i| {
            let v = (i % unique) as u64;
            [v, v.wrapping_mul(3), v ^ 0xDEAD, v + 7, v >> 1, v | 1]
        })
        .collect()
}

fn gen_string(count: usize) -> Vec<String> {
    let unique = count / 2;
    (0..count)
        .map(|i| format!("item-{:012}", i % unique))
        .collect()
}

// ---------------------------------------------------------------------------
// Baseline strategies
// ---------------------------------------------------------------------------

fn strat_sequential<T: std::hash::Hash + Eq + Clone>(items: Vec<T>) -> Vec<T> {
    sequential_dedup(items)
}

fn strat_sort_dedup<T: Ord + Clone>(mut items: Vec<T>) -> Vec<T> {
    items.sort_unstable();
    items.dedup();
    items
}

fn strat_par_sort_dedup<T: Ord + Clone + Send>(mut items: Vec<T>) -> Vec<T> {
    items.par_sort_unstable();
    items.dedup();
    items
}

fn strat_hashset_collect<T: std::hash::Hash + Eq + Clone>(items: Vec<T>) -> Vec<T> {
    items
        .into_iter()
        .collect::<HashSet<T>>()
        .into_iter()
        .collect()
}

fn strat_rayon_hashset<T: std::hash::Hash + Eq + Clone + Send>(items: Vec<T>) -> Vec<T> {
    items
        .into_par_iter()
        .fold(HashSet::new, |mut set, item| {
            set.insert(item);
            set
        })
        .reduce(HashSet::new, |mut a, b| {
            a.extend(b);
            a
        })
        .into_iter()
        .collect()
}

// ---------------------------------------------------------------------------
// Benchmark config
// ---------------------------------------------------------------------------

const SIZES: &[usize] = &[1_000, 10_000, 100_000, 1_000_000, 10_000_000];
const STRING_SIZES: &[usize] = &[1_000, 10_000, 100_000, 1_000_000];
const BUCKET_COUNTS: &[usize] = &[32, 64, 128, 256];

fn size_label(n: usize) -> String {
    match n {
        1_000 => "1k".to_string(),
        10_000 => "10k".to_string(),
        100_000 => "100k".to_string(),
        1_000_000 => "1M".to_string(),
        10_000_000 => "10M".to_string(),
        _ => format!("{n}"),
    }
}

fn bench_params(size: usize) -> (usize, Duration) {
    match size {
        n if n <= 1_000 => (100, Duration::from_secs(3)),
        n if n <= 10_000 => (50, Duration::from_secs(3)),
        n if n <= 100_000 => (30, Duration::from_secs(5)),
        n if n <= 1_000_000 => (15, Duration::from_secs(8)),
        _ => (10, Duration::from_secs(15)),
    }
}

// ---------------------------------------------------------------------------
// Benchmark macro — generates one group per item type
// ---------------------------------------------------------------------------

macro_rules! bench_type {
    ($c:expr, $group_name:expr, $gen:expr, $sizes:expr) => {
        let mut group = $c.benchmark_group($group_name);
        for &size in $sizes {
            let input = $gen(size);
            let label = size_label(size);
            let (sample_size, measurement_time) = bench_params(size);
            group.sample_size(sample_size);
            group.measurement_time(measurement_time);

            let large = size >= 1_000_000;

            // --- Parallel dedup variants: bucket count × hasher ---
            for &buckets in BUCKET_COUNTS {
                group.bench_with_input(
                    BenchmarkId::new(format!("par_dedup_std_b{buckets}"), &label),
                    &input,
                    |b, input| {
                        b.iter(|| parallel_dedup_with(input.clone(), buckets, StdState::new()))
                    },
                );
                group.bench_with_input(
                    BenchmarkId::new(format!("par_dedup_ahash_b{buckets}"), &label),
                    &input,
                    |b, input| {
                        b.iter(|| {
                            parallel_dedup_with(input.clone(), buckets, AhashState::default())
                        })
                    },
                );
                group.bench_with_input(
                    BenchmarkId::new(format!("par_dedup_fxhash_b{buckets}"), &label),
                    &input,
                    |b, input| {
                        b.iter(|| parallel_dedup_with(input.clone(), buckets, FxBuildHasher))
                    },
                );
            }

            // --- Baselines ---
            group.bench_with_input(
                BenchmarkId::new("par_sort_dedup", &label),
                &input,
                |b, input| b.iter(|| strat_par_sort_dedup(input.clone())),
            );
            if !large {
                group.bench_with_input(
                    BenchmarkId::new("sequential_hashbrown", &label),
                    &input,
                    |b, input| b.iter(|| strat_sequential(input.clone())),
                );
                group.bench_with_input(
                    BenchmarkId::new("sort_dedup", &label),
                    &input,
                    |b, input| b.iter(|| strat_sort_dedup(input.clone())),
                );
                group.bench_with_input(
                    BenchmarkId::new("hashset_collect", &label),
                    &input,
                    |b, input| b.iter(|| strat_hashset_collect(input.clone())),
                );
                group.bench_with_input(
                    BenchmarkId::new("rayon_hashset", &label),
                    &input,
                    |b, input| b.iter(|| strat_rayon_hashset(input.clone())),
                );
            }
        }
        group.finish();
    };
}

fn bench_u64(c: &mut Criterion) {
    bench_type!(c, "dedup_u64", gen_u64, SIZES);
}

fn bench_tuple6(c: &mut Criterion) {
    bench_type!(c, "dedup_tuple6", gen_tuple6, SIZES);
}

fn bench_string(c: &mut Criterion) {
    bench_type!(c, "dedup_string", gen_string, STRING_SIZES);
}

criterion_group!(benches, bench_u64, bench_tuple6, bench_string);
criterion_main!(benches);
