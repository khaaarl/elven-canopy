# Per-Frame Pipeline

This document describes what happens each frame in the game loop, covering
the interaction between GDScript (`main.gd`, `tree_renderer.gd`), the
GDExtension bridge (`sim_bridge.rs`), and the background mesh generation
workers. Understanding this pipeline is essential for reasoning about
concurrency safety and frame budgets.

## Call sequence

Every frame, GDScript calls into the Rust bridge in this order:

```
GDScript main loop (every frame)
│
├─ 1. frame_update(delta)
│     │
│     ├─ Single-player: LocalRelay.update() → session.process(AdvanceTo)
│     │   → SimState::step() advances tick, processes events, mutates world
│     │
│     └─ Multiplayer: poll_network() → try_recv() inbox (non-blocking)
│         → session.process(SimCommand) for each received command
│         → session.process(AdvanceTo) to advance to the turn's target tick
│         → SimState::step() advances tick, processes events, mutates world
│
├─ 2. tree_renderer.refresh()
│     │
│     ├─ 2a. update_world_mesh()
│     │     Drains dirty_voxels from VoxelWorld (render-only metadata).
│     │     Marks affected chunks + neighbors as dirty in MeshCache.
│     │     Submits visible dirty chunks for background re-generation
│     │     via ChunkNeighborhood extraction (quick voxel copy).
│     │
│     ├─ 2a'. _apply_draw_distance()
│     │     Reads draw_distance from GameConfig; updates the bridge if
│     │     the value changed (e.g., via settings panel).
│     │
│     ├─ 2b. update_visibility(cam_pos, frustum)
│     │     │
│     │     ├─ drain_completed(): polls mpsc channel for finished meshes
│     │     │   from background workers. Inserts into cache, populates
│     │     │   chunks_generated delta list. Discards stale results
│     │     │   (sim_tick < cached version).
│     │     │
│     │     ├─ Culling pass: classifies chunks as visible (in frustum),
│     │     │   shadow-only (in shadow volume), or hidden. Uses MegaChunk
│     │     │   spatial hierarchy for coarse pass, then per-chunk AABBs.
│     │     │
│     │     ├─ Submission: for each newly-visible chunk without a cached
│     │     │   mesh (and not already in-flight), extracts a
│     │     │   ChunkNeighborhood (quick copy of the chunk's voxels plus
│     │     │   a 2-voxel border, along with any grassless dirt coords in
│     │     │   the region) and spawns a rayon task. All pending chunks
│     │     │   are submitted (no per-frame cap). Camera-nearest chunks
│     │     │   submitted first.
│     │     │
│     │     ├─ Delta lists: computes show/hide/shadow transitions by
│     │     │   diffing old vs new visible/shadow sets.
│     │     │
│     │     └─ LRU eviction: if over memory budget, evicts least-recently-
│     │         accessed non-visible chunks.
│     │
│     ├─ 2c. Process delta lists (GDScript)
│     │     Creates MeshInstance3D nodes for generated chunks.
│     │     Toggles .visible and cast_shadow for show/hide/shadow transitions.
│     │     Frees MeshInstance3D nodes for evicted chunks.
│     │     Builds ArrayMesh from cached ChunkMesh data (main thread).
│     │
│     └─ 2d. refresh_fruit()
│           Updates billboarded Sprite3D nodes for fruit voxels.
│
└─ 3. (other per-frame work: UI, input, creature rendering, etc.)
```

## Concurrency model

All numbered steps above are **sequential on the main thread**. There is no
overlap between sim advance (step 1) and mesh operations (step 2). This is
true in both single-player and multiplayer modes.

Background mesh generation runs on **rayon worker threads**. These workers
operate on owned `ChunkNeighborhood` snapshots — they do not hold references
to `VoxelWorld` or any other shared state. The snapshot is extracted on the
main thread during steps 2a (dirty re-generation) and 2b (first-time
generation of newly-visible chunks). Each extraction copies the chunk's
16x16x16 voxels plus a 2-voxel border (~20x20x20 = 8000 voxels), then
the copy is handed to the worker as owned data.

This means:
- **No locks are needed.** The main thread reads the world during extraction;
  rayon workers read only their own copies. There is no concurrent access to
  `VoxelWorld`.
- **Staleness is self-correcting.** A worker's snapshot may be several frames
  old if mesh generation takes longer than one frame (complex chunks with
  heavy smoothing/decimation). If the world changes while a mesh is being
  generated, the dirty-voxel mechanism marks the chunk for re-generation.
  When the stale result arrives it is inserted normally, then the dirty flag
  triggers a fresh submission on the next frame.
- **Freshness checks prevent stale overwrites.** Each `ChunkNeighborhood`
  carries the `sim_tick` at extraction time. When a result arrives, it is
  discarded if a newer version is already cached (two results for the same
  chunk raced and the newer one won).

## Key files

| File | Role |
|------|------|
| `godot/scripts/main.gd` | Drives the per-frame loop, calls `frame_update` and `tree_renderer.refresh` |
| `godot/scripts/tree_renderer.gd` | GDScript rendering: frustum extraction, delta list processing, MeshInstance3D management |
| `elven_canopy_gdext/src/sim_bridge.rs` | GDExtension bridge: thin wrappers exposing sim and mesh cache to GDScript |
| `elven_canopy_gdext/src/mesh_cache.rs` | MegaChunk hierarchy, async submission, channel drain, visibility culling, LRU eviction |
| `elven_canopy_sim/src/chunk_neighborhood.rs` | `ChunkNeighborhood`: lightweight voxel snapshot for off-thread mesh generation |
| `elven_canopy_sim/src/mesh_gen.rs` | Pure mesh generation: `ChunkNeighborhood` → `ChunkMesh` |
| `elven_canopy_sim/src/world.rs` | `VoxelWorld`: RLE voxel grid, dirty tracking, `sim_tick` |
| `elven_canopy_sim/src/sim/mod.rs` | `SimState`: owns `VoxelWorld` and `grassless: BTreeSet<VoxelCoord>` (grazed dirt coords, also captured into neighborhoods) |
