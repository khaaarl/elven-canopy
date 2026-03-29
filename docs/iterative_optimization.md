# Iterative Agent-Driven Performance Optimization

A guide to using Claude Code agents for systematic, correctness-gated performance optimization. Written based on the F-mesh-pipeline-perf work (March 2026), which achieved ~1.8x speedup on the chunk mesh pipeline through six iterative optimization attempts.

## Overview

The process has two phases:

1. **Harness phase**: Build a snapshot regression test and criterion benchmarks that capture the exact output of the code being optimized. This is the correctness gate — no optimization can land if it changes the output.

2. **Optimization phase**: Repeatedly spawn agents that each implement one optimization, verify correctness, measure performance, and commit or revert. A persistent diary file records every attempt (successes and failures) so agents don't repeat work.

## Phase 1: Building the Harness

### Snapshot regression test

The goal is to freeze the output of the code being optimized so any change — even a single bit — is detected. For the mesh pipeline, this meant:

- **Fixtures**: Representative inputs covering edge cases. Hand-built (single voxel, flat slab, L-shape, staircase, diagonal adjacency, mixed material, fully solid, fully empty, thin wall, overhang, building-on-terrain-edge) plus world-generated samples (surface-heavy, canopy, trunk, sparse).
- **Serialization**: Fixtures are bincode-serialized and regenerated from seeds (not checked into the repo). This avoids binary blobs but couples to the generation code.
- **Multiple configurations**: The mesh pipeline has toggles (smoothing on/off, decimation on/off). The harness must test all configurations that matter, including the default. We initially missed this — the harness only tested smoothing-ON, but the game default is smoothing-OFF. This was caught and fixed mid-process.
- **Comparison**: Position/normal floats compared with epsilon tolerance (1e-5). Colors and indices compared exactly. Vertex/triangle counts checked first for fast failure.
- **Auto-generation on first run**: If expected files are missing or have a stale format, the test generates them rather than failing. This makes setup frictionless.

### Criterion benchmarks

- **Match real-world configuration**: Benchmark the actual code path users hit. For the mesh pipeline, this means smoothing OFF by default. We added separate groups for smoothing-ON to track that path too, but the primary optimization target must be the default.
- **Per-stage benchmarks**: If the pipeline has distinct stages (face generation → chamfer → decimation → output flattening), expose them as individually benchmarkable functions. This lets you identify which stage an optimization actually helped, even when end-to-end numbers are noisy.
- **Noisy environments**: On shared-CPU VMs, criterion results have 5-20% variance. Use longer measurement times, run benchmarks twice, and focus on consistent directional improvements rather than exact percentages. A 3% improvement is noise; a 15% improvement across multiple fixtures is real.

### Refactoring for benchmarkability

The mesh pipeline was one monolithic function (`generate_chunk_mesh`). To enable per-stage benchmarking, we extracted:

- `build_smooth_mesh()` — face generation stage
- `run_chamfer_smooth()` — chamfer + optional smoothing
- `run_decimation()` — decimation pipeline
- `flatten_to_chunk_mesh()` — output flattening

The main function became a thin wrapper calling these in sequence. This refactor is essential — without it, you can only measure the full pipeline, making it hard to know which stage improved.

## Phase 2: Iterative Optimization

### The optimization diary

A markdown file at `.tmp/mesh-perf-diary.md` that persists across agent invocations. Contains:

- **Baseline numbers**: Full benchmark results before any optimization.
- **Per-attempt entries**: Each entry records the idea, rationale, safety analysis, changes made, verification results, benchmark numbers (before/after), and verdict (KEPT or REVERTED).

The diary is critical because each agent starts fresh with no memory of previous agents. Without it, agents would repeat the same ideas.

### Agent loop procedure

Each optimization agent:

1. Reads the diary to understand what's been tried.
2. Reads the source code to identify the current bottleneck.
3. Implements one focused optimization.
4. Runs the snapshot regression test (must pass — non-negotiable).
5. Runs the full test suite.
6. Runs formatting/linting checks.
7. Runs benchmarks and records results.
8. If improvement + all checks pass: commits and pushes.
9. If no improvement or correctness failure: reverts and records the failure.

### Spawning agents: practical lessons

**Use the same branch, not worktrees.** Early attempts used `isolation: "worktree"`, which creates a temporary git worktree. This caused serious problems:

- The worktree branched from the repo's current HEAD at spawn time, but if previous optimizations hadn't been merged yet, the worktree got stale code missing prior improvements.
- One agent's worktree branched from `main` instead of the feature branch, causing it to revert all prior optimizations in its diff.
- Fixture files in `.tmp/` aren't part of the git tree, so worktree agents couldn't find them.

**The fix**: Run agents directly on the feature branch without worktree isolation. Since optimization attempts are sequential (each builds on the last), parallelism isn't needed. The agent prompt must explicitly state:

```
You are working directly in /path/to/repo/ (the main repo checkout).
You are on branch `feature/branch-name`. Stay on this branch.
Do NOT create new branches or switch branches.
Fixture files are at /path/to/repo/.tmp/mesh_fixtures/.
```

**Be explicit about the target configuration.** When the code has runtime toggles (like smoothing on/off), the agent prompt must specify which configuration to optimize and benchmark. Agents will otherwise benchmark a non-default configuration and optimize dead code.

**Tell agents what's already been done.** Include a numbered list of previous optimizations directly in the prompt. Agents can read the diary, but having the summary in the prompt prevents wasted time re-reading and reduces the chance of repeating work.

**Suggest concrete targets but don't over-constrain.** List 4-6 specific optimization ideas ranked by likely impact, but make clear the agent should read the code and choose. Early agents pick the obvious wins; later agents need more guidance toward subtler targets.

### Example agent prompt structure

```
You are an optimization agent for [component] in [crate].

## Working environment
- Working directory: /path/to/repo/
- Branch: feature/branch-name (stay on it, don't create new branches)
- Fixtures: /path/to/.tmp/fixtures/
- Diary: /path/to/.tmp/diary.md
- Scratch: /path/to/.tmp/

## Target configuration
[Specify which runtime toggles/flags represent the real-world hot path]

## Procedure
[Numbered steps: read diary → read code → implement → test → bench → commit/revert]

## Previous optimizations (DO NOT repeat)
1. [Description of attempt 1]
2. [Description of attempt 2]
...

## Optimization targets to consider
- [Specific idea 1 with rationale]
- [Specific idea 2 with rationale]
...
```

## Lessons Learned

### What worked well

- **Snapshot regression as the correctness gate.** Every optimization was verified to produce bit-identical output. No correctness regressions shipped. The A/B verification (reverting the optimization, generating expected output, re-applying, comparing) is worth doing for subtle changes.

- **The diary.** Agents reliably read the diary and avoided repeating work. The "safety analysis" section in each entry was particularly valuable — it forced agents to reason about why their change was semantics-preserving before implementing.

- **Sequential agents on one branch.** Simple, no merge conflicts, each agent sees all prior work. The lack of parallelism is fine because each optimization changes the performance profile, making the next bottleneck different.

- **Low-hanging fruit ordering.** The first attempts (BTreeMap → FxHashMap, removing O(n) linear searches) were high-impact and low-risk. Memory pre-allocation came later as a smaller but safe win. This natural ordering emerged from agents reading the code and picking the most obvious bottleneck.

### What went wrong

- **Worktree isolation caused stale code.** Agents in worktrees got a snapshot of the repo at spawn time, missing prior optimizations. One agent accidentally reverted the FxHashMap change because its worktree branched from `main`. Fix: don't use worktrees for sequential optimization work.

- **Benchmarking the wrong configuration.** The initial benchmarks ran with smoothing ON, but the game default is smoothing OFF. This meant Attempt 2 (smoothing optimization) appeared impactful but was actually optimizing a debug-only code path. Fix: always verify that benchmarks match the production configuration before starting optimization work.

- **VM noise masking results.** On a shared-CPU VM, benchmark variance was 5-20%. Some improvements were invisible in the noise. Fix: run benchmarks twice, focus on fixtures where the optimized code path dominates (dense chunks for decimation, large flat areas for retri), and accept that small improvements may not be measurable in this environment.

- **Agent fixture access in worktrees.** Benchmark fixtures in `.tmp/` aren't tracked by git, so worktree agents couldn't find them. Fix: either include fixture paths in the prompt or avoid worktrees entirely.

## Rust-Specific Optimization Patterns

The mesh pipeline optimizations followed a clear progression that generalizes to most Rust performance work:

### 1. Replace BTreeMap/BTreeSet with HashMap/HashSet

BTreeMap is O(log n) per operation; HashMap is O(1) amortized. BTreeMap is required when iteration order matters for determinism, but many maps are only used for point lookups. Audit each usage: if the map is never iterated (only `get`, `insert`, `remove`, `contains`), it's safe to replace.

For this project we used `FxHashMap` from the `rustc-hash` crate — a fast non-cryptographic hash that's ideal for integer/small-key maps. The `rustc-hash` crate is lightweight and widely used.

**Determinism caution**: If the codebase requires deterministic output (as this one does for multiplayer), you must verify that HashMap iteration order doesn't leak into the output. Check every `.iter()`, `.keys()`, `.values()`, `.into_iter()` call on the map.

### 2. Eliminate O(n) searches in hot loops

`Vec::contains()` is O(n). When called in a loop that also grows the Vec, it's O(n^2) total. Common pattern: building a neighbor list by checking `if !list.contains(&x) { list.push(x) }`. Fix: push unconditionally, then deduplicate once at the end. This turns O(n^2) into O(n).

For small lists (< 20 elements), a simple quadratic dedup is fine. For larger lists, sort + dedup or use a HashSet.

### 3. Pre-allocate collections

`Vec::new()` starts with zero capacity. Each `push` that exceeds capacity triggers a reallocation (memcpy of the entire buffer). For predictable sizes, use `Vec::with_capacity(n)`. Similarly, `HashMap::with_capacity(n)` avoids rehashing.

The heuristic doesn't need to be exact — oversizing by 20% is better than undersizing by 20% (one wastes memory, the other triggers a reallocation that copies the entire contents).

### 4. Precompute loop-invariant values

When an inner loop recomputes a value that doesn't change across iterations, hoist it out. The smoothing optimization precomputed neighbor centroids once instead of recomputing them 5 times per vertex. This is the same principle as loop-invariant code motion in compilers, but at a higher level where the compiler can't see the invariance (e.g., "the centroid doesn't change because we're only moving vertex vi, not its neighbors").

## Mesh Pipeline Specifics (F-mesh-pipeline-perf)

### Pipeline architecture

```
ChunkNeighborhood (voxel snapshot)
    → build_smooth_mesh()     [face gen: 8 subdivided triangles per visible face]
    → run_chamfer_smooth()    [resolve non-manifold, anchor, chamfer, optional smooth]
    → run_decimation()        [coplanar retri, collinear collapse, QEM edge-collapse]
    → flatten_to_chunk_mesh() [split by surface tag, filter to chunk bounds]
```

### Optimization results summary

| # | Optimization | Target | Impact |
|---|-------------|--------|--------|
| 1 | BTreeMap → FxHashMap (vertex dedup) | Face gen | -12-14% full pipeline |
| 2 | Precomputed smoothing centroids | Smoothing (debug-only) | ~3x less work in smooth loop |
| 3 | BTreeMap/BTreeSet → FxHashMap/FxHashSet (decimation) | Decimation | -17-49% full pipeline |
| 4 | Batch edge insertion (deferred dedup) | Face gen + non-manifold | -15-21% on dense fixtures |
| 5 | Batch dedup in compact_after_decimation | Decimation compaction | -6-15% on dense fixtures |
| 6 | Memory pre-allocation (8 sites) | All stages | -16-23% on flat_slab |

Cumulative effect on the heaviest real-world fixture (worldgen_trunk): baseline ~58ms → ~33ms, roughly **1.8x speedup**.

### Files involved

- `elven_canopy_sim/src/mesh_gen.rs` — face generation, pipeline orchestration
- `elven_canopy_sim/src/smooth_mesh.rs` — SmoothMesh data structure, chamfer, smoothing
- `elven_canopy_sim/src/mesh_decimation.rs` — QEM decimation, coplanar retri, collinear collapse
- `elven_canopy_sim/tests/mesh_snapshots.rs` — fixture generation and snapshot regression
- `elven_canopy_sim/benches/mesh_pipeline.rs` — criterion benchmarks
- `docs/optimization-diaries/mesh-pipeline-perf.md` — optimization diary (committed)
- `.tmp/mesh-perf-diary.md` — working copy of diary during active optimization (not committed)
- `.tmp/mesh_fixtures/` — serialized fixture files (not committed)
