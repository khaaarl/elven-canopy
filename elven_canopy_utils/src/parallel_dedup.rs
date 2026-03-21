//! Radix-partitioned parallel deduplication.
//!
//! Algorithm overview (see also `lib.rs`):
//!
//! 1. **Partition (parallel scatter):** Split the input into spans (one per
//!    rayon chunk). Each worker hashes every item once, uses
//!    `hash & (num_buckets - 1)` (bitmask — bucket count is always a power
//!    of 2) to assign it to a bucket, and stores `(hash, item)` in that
//!    bucket. Each worker has its own set of buckets — zero contention.
//!
//! 2. **Dedup (parallel gather):** For each bucket index 0..N, collect all
//!    items across every worker's version of that bucket, then insert them
//!    into a `hashbrown::HashTable` using the precomputed hash. Duplicates
//!    are detected here via `Eq`. Again, each bucket index is processed by
//!    one thread — zero contention.
//!
//! 3. **Collect:** Concatenate the per-bucket results into the output vec.
//!
//! Cost per item: 1 hash, ~2 copies (to bucket, to hash table; then move out),
//! plus an `Eq` comparison on hash collision. Falls back to sequential
//! `HashSet`-style dedup for inputs below `PARALLEL_THRESHOLD`.
//!
//! The bucket count is configurable (always rounded up to a power of 2).
//! More buckets than threads lets rayon work-steal to handle imbalances.
//! The default is `num_threads * 8`, rounded up to the next power of 2.
//!
//! The hasher is generic via `BuildHasher`, defaulting to `RandomState`
//! (std SipHash). Callers can substitute faster hashers (ahash, fxhash)
//! for non-adversarial inputs.
//!
//! Depends on `hashbrown` for `HashTable` (insert-with-precomputed-hash)
//! and `rayon` for the worker pool.

use hashbrown::HashTable;
use rayon::prelude::*;
use std::hash::{BuildHasher, Hash, RandomState};

/// Inputs smaller than this are deduped sequentially — the overhead of
/// partitioning and rayon task spawning isn't worth it below this point.
const PARALLEL_THRESHOLD: usize = 10_000;

/// Extra capacity factor for pre-allocated scatter buckets (Vecs, not
/// hash tables). 1.25x the expected size avoids most reallocations
/// when hash distribution isn't perfectly uniform.
const BUCKET_SLACK: f64 = 1.25;

/// Returns the default bucket count: `num_threads * 8`, rounded up to
/// the next power of 2 if not already one.
fn default_num_buckets() -> usize {
    (rayon::current_num_threads() * 8).next_power_of_two()
}

/// Deduplicate `items` in parallel using radix partitioning.
///
/// Returns a `Vec` containing one copy of each unique item. Output order
/// is arbitrary (not insertion-order-preserving).
///
/// Uses the default bucket count and std `RandomState` hasher. For
/// fine-grained control, use [`parallel_dedup_with`].
pub fn parallel_dedup<T>(items: Vec<T>) -> Vec<T>
where
    T: Hash + Eq + Clone + Send + Sync,
{
    if items.len() < PARALLEL_THRESHOLD {
        return sequential_dedup(items);
    }
    parallel_dedup_with(items, default_num_buckets(), RandomState::new())
}

/// Parallel radix-partition dedup with explicit bucket count and hasher.
///
/// `num_buckets` is rounded up to the next power of 2 if not already one.
/// More buckets (e.g., 4–8× thread count) enables better work stealing
/// at the cost of more scatter targets in phase 1.
///
/// The hasher is used for all hashing — both bucket assignment (via
/// bitmask on the hash) and hash-table probing. Faster hashers like
/// `ahash::RandomState` can improve throughput on non-adversarial inputs.
pub fn parallel_dedup_with<T, S>(items: Vec<T>, num_buckets: usize, hasher: S) -> Vec<T>
where
    T: Hash + Eq + Clone + Send + Sync,
    S: BuildHasher + Sync,
{
    let num_buckets = num_buckets.max(2).next_power_of_two();
    let bucket_mask = num_buckets - 1;

    // --- Phase 1: Parallel scatter into buckets ---
    //
    // Split the input into roughly equal spans. Each span is processed by
    // one rayon task, which hashes every item and places (hash, item) into
    // a thread-local bucket array indexed by `hash & bucket_mask`.

    let num_threads = rayon::current_num_threads();
    let span_size = items.len().div_ceil(num_threads);
    let bucket_capacity = ((span_size / num_buckets) as f64 * BUCKET_SLACK) as usize;

    let per_worker_buckets: Vec<Vec<Vec<(u64, T)>>> = items
        .par_chunks(span_size)
        .map(|span| {
            let mut buckets: Vec<Vec<(u64, T)>> = (0..num_buckets)
                .map(|_| Vec::with_capacity(bucket_capacity.max(1)))
                .collect();
            for item in span {
                let hash = hasher.hash_one(item);
                let bucket_idx = (hash as usize) & bucket_mask;
                buckets[bucket_idx].push((hash, item.clone()));
            }
            buckets
        })
        .collect();

    // --- Phase 2: Parallel gather + dedup per bucket ---
    //
    // For each bucket index, pull together all workers' contributions to
    // that bucket and insert into a HashTable using precomputed hashes.

    let deduped_buckets: Vec<HashTable<T>> = (0..num_buckets)
        .into_par_iter()
        .map(|bucket_idx| {
            // Total items destined for this bucket across all workers.
            // This is the absolute max the table can reach — no slack needed.
            let total: usize = per_worker_buckets.iter().map(|w| w[bucket_idx].len()).sum();
            let mut table: HashTable<T> = HashTable::with_capacity(total);

            for worker_buckets in &per_worker_buckets {
                for &(hash, ref item) in &worker_buckets[bucket_idx] {
                    if table.find(hash, |existing: &T| existing == item).is_none() {
                        table.insert_unique(hash, item.clone(), |v| hasher.hash_one(v));
                    }
                }
            }

            table
        })
        .collect();

    // --- Phase 3: Collect ---

    let total_unique: usize = deduped_buckets.iter().map(|b| b.len()).sum();
    let mut result = Vec::with_capacity(total_unique);
    for table in deduped_buckets {
        result.extend(table.into_iter());
    }
    result
}

/// Sequential dedup using a single-threaded hash table.
///
/// This is the fallback used by `parallel_dedup` for small inputs, but is
/// also exposed publicly for benchmarking comparisons.
pub fn sequential_dedup<T>(items: Vec<T>) -> Vec<T>
where
    T: Hash + Eq + Clone,
{
    let hasher = RandomState::new();
    let mut table = HashTable::with_capacity(items.len());
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let hash = hasher.hash_one(&item);
        let entry = table.find(hash, |existing: &T| existing == &item);
        if entry.is_none() {
            table.insert_unique(hash, item.clone(), |v| hasher.hash_one(v));
            out.push(item);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: sort a vec for order-independent comparison.
    fn sorted<T: Ord + Clone>(v: Vec<T>) -> Vec<T> {
        let mut v = v;
        v.sort();
        v
    }

    #[test]
    fn empty_input() {
        let input: Vec<i32> = vec![];
        let result = parallel_dedup(input);
        assert!(result.is_empty());
    }

    #[test]
    fn single_element() {
        assert_eq!(parallel_dedup(vec![42]), vec![42]);
    }

    #[test]
    fn all_duplicates() {
        let input = vec![7; 100];
        let result = parallel_dedup(input);
        assert_eq!(result, vec![7]);
    }

    #[test]
    fn no_duplicates_small() {
        let input: Vec<i32> = (0..100).collect();
        let result = parallel_dedup(input.clone());
        assert_eq!(sorted(result), input);
    }

    #[test]
    fn mixed_duplicates_small() {
        let input = vec![1, 2, 3, 2, 1, 4, 3, 5, 5, 5];
        let result = parallel_dedup(input);
        assert_eq!(sorted(result), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn strings() {
        let input = vec!["apple", "banana", "apple", "cherry", "banana"];
        let result = parallel_dedup(input);
        assert_eq!(sorted(result), vec!["apple", "banana", "cherry"]);
    }

    #[test]
    fn exercises_parallel_path() {
        // Above PARALLEL_THRESHOLD to exercise the actual parallel algorithm.
        let input: Vec<i32> = (0..20_000).chain(0..20_000).collect();
        let result = parallel_dedup(input);
        assert_eq!(result.len(), 20_000);
        let expected: Vec<i32> = (0..20_000).collect();
        assert_eq!(sorted(result), expected);
    }

    #[test]
    fn exercises_parallel_path_all_same() {
        let input = vec![999i32; 50_000];
        let result = parallel_dedup(input);
        assert_eq!(result, vec![999]);
    }

    #[test]
    fn exercises_parallel_path_no_dupes() {
        let input: Vec<i32> = (0..30_000).collect();
        let result = parallel_dedup(input.clone());
        assert_eq!(result.len(), 30_000);
        assert_eq!(sorted(result), input);
    }

    #[test]
    fn exercises_parallel_path_high_duplication() {
        // Many items, few unique values — stresses the hash table.
        let input: Vec<i32> = (0..50_000).map(|i| i % 100).collect();
        let result = parallel_dedup(input);
        assert_eq!(result.len(), 100);
        let expected: Vec<i32> = (0..100).collect();
        assert_eq!(sorted(result), expected);
    }

    #[test]
    fn sequential_dedup_directly() {
        // Verify the sequential path works correctly.
        let input = vec![10, 20, 10, 30, 20, 40];
        let result = sequential_dedup(input);
        assert_eq!(sorted(result), vec![10, 20, 30, 40]);
    }

    #[test]
    fn threshold_boundary() {
        // Exactly at the threshold — should take the sequential path.
        let input: Vec<i32> = (0..PARALLEL_THRESHOLD as i32).collect();
        let result = parallel_dedup(input.clone());
        assert_eq!(sorted(result), input);

        // One above — should take the parallel path.
        let input2: Vec<i32> = (0..PARALLEL_THRESHOLD as i32 + 1).collect();
        let result2 = parallel_dedup(input2.clone());
        assert_eq!(sorted(result2), input2);
    }

    #[test]
    fn explicit_bucket_count_and_hasher() {
        let input: Vec<i32> = (0..20_000).chain(0..20_000).collect();
        let result = parallel_dedup_with(input, 64, RandomState::new());
        assert_eq!(result.len(), 20_000);
        let expected: Vec<i32> = (0..20_000).collect();
        assert_eq!(sorted(result), expected);
    }

    #[test]
    fn non_power_of_two_bucket_count_rounded_up() {
        // 37 should round up to 64.
        let input: Vec<i32> = (0..20_000).chain(0..20_000).collect();
        let result = parallel_dedup_with(input, 37, RandomState::new());
        assert_eq!(result.len(), 20_000);
    }
}
