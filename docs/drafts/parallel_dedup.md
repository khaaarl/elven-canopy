# Parallel Dedup — Design & Progress

## Current State (2026-03-21)

New crate `elven_canopy_utils` with a radix-partitioned parallel dedup algorithm. Functional, tested (14 unit tests), benchmarked, but **not yet integrated** into any consumer crate. Benchmark results so far show `par_sort_unstable + dedup` still wins — optimization work is ongoing.

### What exists

- **`parallel_dedup(items)`** — simple API, falls back to sequential below 10k items, uses default bucket count (num_threads × 8, rounded to power of 2) and std SipHash.
- **`parallel_dedup_with(items, num_buckets, hasher)`** — full control over bucket count and hash function. Bucket count always rounded up to next power of 2 for bitmask bucket assignment.
- **`sequential_dedup(items)`** — single-threaded hashbrown-based dedup, used as fallback and benchmark baseline.
- **Criterion benchmarks** (`benches/dedup.rs`) testing 4 bucket counts × 3 hashers × 3 item types × 5 input sizes, plus baseline strategies.

### Next steps

1. **Analyze benchmark results** — run on target hardware, compare parallel_dedup variants against par_sort_dedup. Determine if any bucket count / hasher combination beats par_sort for any item type or size.
2. **Tune defaults** — based on benchmark data, pick the best default bucket count and decide whether to ship ahash/fxhash as a runtime dependency (currently dev-only).
3. **Reduce copies** — the current algorithm clones each item into a scatter bucket (phase 1) and again into the hash table (phase 2). Investigate whether unsafe scatter-in-place or index-based approaches can reduce this to 1 copy.
4. **Investigate why par_sort wins** — par_sort_unstable + dedup benefits from cache-friendly sequential access and extremely optimized pdqsort. Our approach pays for scattered writes in phase 1 and hash table overhead in phase 2. The gap may narrow with larger/more expensive items (strings) or close with a better hasher.
5. **Integration** — once tuned, integrate into sim or tabulosity where bulk dedup is needed. The `Hash + Eq` (no `Ord` required) trait bound is the main API advantage over sort-based dedup.
6. **Threshold tuning** — the 10k fallback threshold is a guess. Benchmark data at 1k and 10k sizes will inform the right crossover point.

---

## Algorithm

### Overview

The algorithm is a standard **radix-partitioned parallel dedup**, the same pattern used internally by database query engines (DataFusion, Polars) for parallel DISTINCT / GROUP BY. The key insight: partition items by hash into buckets such that equal items always land in the same bucket, then dedup each bucket independently with zero cross-thread contention.

### Three phases

**Phase 1 — Parallel scatter.** Split the input vec into N equal spans (N = rayon thread count). Each span is processed by one rayon task. For each item in the span:
- Compute `hash = hasher.hash_one(item)` (done once per item, reused in phase 2).
- Assign to bucket via `hash & (num_buckets - 1)` (bitmask, since bucket count is always a power of 2 — avoids expensive integer division).
- Push `(hash, item)` into the worker's thread-local bucket for that index.

Each worker has its own set of `num_buckets` Vecs — no synchronization needed.

**Phase 2 — Parallel gather + dedup.** For each bucket index `b` in `0..num_buckets`, a rayon task collects all workers' contributions to bucket `b` and inserts them into a `hashbrown::HashTable` using the precomputed hash. `HashTable::insert_unique` takes the hash directly, avoiding re-hashing. Duplicates are detected via `Eq` on hash collision. The hasher closure passed to `insert_unique` is only called on table resize, which we avoid by pre-allocating to the exact item count.

**Phase 3 — Collect.** Drain each `HashTable` into a single output vec. Order is arbitrary.

### Cost model

Per item:
- **1 hash** (in phase 1; reused in phase 2)
- **~2 clones** (into scatter bucket, into hash table; then move out of table into result)
- **1 bitmask** (bucket assignment)
- **1 Eq comparison** per hash collision in the hash table

### Bucket count

The bucket count controls the tradeoff between work-stealing granularity and scatter overhead:

- **Too few buckets** (e.g., = thread count): Phase 2 tasks are large and any size imbalance between buckets causes threads to idle. But phase 1 scatter is cache-friendlier with fewer targets.
- **Too many buckets** (e.g., thousands): Phase 1 scatter becomes cache-hostile (writing to many Vecs), and the overhead of many tiny rayon tasks adds up. But load balance is near-perfect.
- **Sweet spot**: Typically 4–8× thread count. The bucket count is always rounded up to a power of 2 so that bucket assignment uses a bitmask (`&`) instead of modulo (`%`), saving ~20-90 cycles per item on x86.

When the bucket count equals the thread count and isn't a power of 2 (e.g., 7 cores), rounding up to the next power of 2 (8) would mean one thread gets two buckets while others get one — potentially doubling that thread's work. This is why the default uses a higher multiplier (8×): at that scale, rounding 56 → 64 is only ~14% overhead, well-absorbed by work stealing.

### Hash function choice

The hasher is generic (`BuildHasher + Sync`). Options:

| Hasher | Throughput | DoS-safe | Notes |
|--------|-----------|----------|-------|
| `std::RandomState` (SipHash-1-3) | Moderate | Yes | Default. Safe for adversarial input. |
| `ahash::RandomState` | Fast | Mostly | AES-NI accelerated. What hashbrown uses internally. |
| `rustc_hash::FxBuildHasher` | Very fast | No | Simple multiply-xor. Best for integers, poor for adversarial input. |

For game sim data (non-adversarial), ahash or fxhash should be safe and potentially faster. Benchmarks will determine whether the hash function is actually the bottleneck vs. memory access patterns.

### Comparison with par_sort + dedup

`par_sort_unstable + dedup` is a strong baseline because:
- pdqsort is extremely well-optimized (pattern-defeating, cache-oblivious)
- The sort + dedup access pattern is purely sequential — excellent cache behavior
- `dedup` after sorting is a trivial O(n) single pass

Our approach pays for:
- Scattered writes during phase 1 (each item written to one of N bucket Vecs)
- Hash table overhead in phase 2 (probing, collision handling, pointer chasing)
- Two clones per item vs. in-place sort

However, `par_sort + dedup` requires `Ord`. Our approach only needs `Hash + Eq`, which is a meaningful API advantage for types without a natural ordering.

### Potential future optimizations

- **Reduce clones**: Currently 2 clones per unique item. Could potentially use unsafe index-based approaches where phase 1 records `(hash, index)` instead of `(hash, item)`, then phase 2 reads items by index from the original slice. Requires careful lifetime management.
- **Two-pass scatter**: Count items per bucket first, then allocate exact-size Vecs and place. Avoids Vec growth overhead and gives better memory locality. Standard optimization in database radix partitioning.
- **SIMD hashing**: For fixed-size integer keys, SIMD hash functions can hash multiple items per cycle.
- **Partition the partition**: For very large inputs, a multi-level radix (scatter by high bits, then low bits) could improve cache behavior in phase 1.

## Files

- `elven_canopy_utils/Cargo.toml` — crate manifest; deps: hashbrown, rayon; dev-deps: criterion, ahash, rustc-hash
- `elven_canopy_utils/src/lib.rs` — crate root, re-exports
- `elven_canopy_utils/src/parallel_dedup.rs` — algorithm + 14 unit tests
- `elven_canopy_utils/benches/dedup.rs` — criterion benchmarks (bucket count × hasher × item type × input size)
