# RLE Column-Based Voxel Storage (F-rle-voxels)

## Overview

Replace the flat `Vec<VoxelType>` voxel grid with a compressed column-based
representation. Each (x, z) column stores a sorted list of spans describing
the voxel types from bottom to top. Columns are grouped in 16×16 blocks (sharing XZ alignment with the mesh
chunk grid) with a single heap allocation per group.

**Goal:** Reduce voxel memory from O(world_volume) to O(world_surface_complexity),
enabling 1024×128×1024+ worlds without GB-scale RAM.

## Span Representation

A span is a `(VoxelType, top_y)` pair packed into 2 bytes:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct Span {
    voxel_type: u8,  // VoxelType discriminant
    top_y: u8,       // highest Y coordinate (inclusive) in this run
}
```

Spans within a column are sorted ascending by `top_y`. The first span
implicitly starts at y=0. Each subsequent span starts at `prev.top_y + 1`.
The topmost span in a column always has `top_y = size_y - 1`... **except**
if the topmost material is Air, that final Air span is omitted (since Air
above the last explicit span is implied). This saves 2 bytes per column in
the overwhelmingly common case.

**World height constraint:** `size_y` must be in `[1, 255]`. Y coordinates
are stored as `u8` with valid range `[0, size_y - 1]`. The current default
is 128.

### Examples

A typical terrain column (dirt at y=0, air above):
```
[(Dirt, 0)]              → 2 bytes
// Implied: Air from y=1 to y=size_y-1
```

A column through a tree trunk with a platform at y=45:
```
[(Dirt, 0), (Trunk, 11), (Air, 44), (GrownPlatform, 45)]
                         → 8 bytes
// Implied: Air from y=46 to y=size_y-1
```

An all-air column:
```
[]                       → 0 bytes (0 spans)
// Implied: Air everywhere
```

## Column Group Layout

Columns are organized in 16×16 groups sharing XZ alignment with the mesh
chunk grid (mesh chunks are 16×16×16 3D cubes; column groups share the XZ
footprint but span the full world height). Each group owns a single heap
allocation for all its columns' span data, plus inline per-column metadata
packed into 32 bits:

```rust
/// Per-column metadata, 4 bytes, packed inline in the group.
#[repr(C)]
#[derive(Clone, Copy)]
struct ColMeta {
    /// Span index into the group's `spans` vec where this column starts.
    /// u16 is sufficient: worst case is 256 columns × 255 spans = 65,280,
    /// which fits in u16 (max 65,535).
    data_start: u16,
    /// Number of spans actually stored for this column.
    num_spans: u8,
    /// Number of span slots allocated (≥ num_spans). The column's data
    /// occupies spans[data_start..data_start + num_allocated].
    num_allocated: u8,
}

struct ColumnGroup {
    /// Per-column metadata. Index: local_x + local_z * 16.
    cols: [ColMeta; 256],       // 1024 bytes, inline

    /// Index into `spans` where unallocated free space begins.
    /// Everything in spans[free_start..] is available for new/moved columns.
    free_start: u16,

    /// Span data for all columns plus free tail space. Columns are not
    /// necessarily contiguous or in order — ColMeta.data_start is the
    /// source of truth for each column's location.
    spans: Vec<Span>,
}
```

**Inline overhead:** 1024 bytes per group (the `cols` array) + 2 bytes
(`free_start`).

### Allocation Strategy

**Initial allocation:** During worldgen and save-load, voxels are written
via `set()` which uses the incremental path below. After worldgen/load
completes, call `repack_all()` to compact every group tightly (eliminating
dead space and fragmentation from the bulk writes) and add a fresh free
tail (~25% of live spans or a minimum of 64 spans). This keeps the
incremental `set()` path simple while ensuring the steady-state layout is
clean.

**On `set_voxel`:**

1. Recompute the column's spans (split/merge as needed).
2. **Fits in place** (`new_count <= col.num_allocated`): rewrite the column's
   span data in place. Cost: O(column spans). **No other columns touched.**
3. **Needs more room, free tail has space**: move the column to `free_start`.
   Allocate generously: `max(new_count + 2, new_count * 3 / 2)` spans, so
   future growth is likely to hit case 2. Update `data_start`, `num_allocated`,
   advance `free_start`. The old slot becomes dead space. Cost: O(column spans).
4. **Free tail exhausted**: full repack — compact all 256 columns contiguously,
   reset `free_start`, grow the `spans` vec if total live data has increased.
   During repack, give each column a small growth margin. Cost: O(total spans),
   but amortized across many edits.

**Overflow guard:** `data_start` is `u16`, capping the group at 65,535 span
slots. This is safe in steady state (256 columns × 255 max spans = 65,280),
but during heavy bulk writes (e.g., worldgen) dead space from moves could
accumulate toward the limit before `repack_all()` runs. Guard: if the
group's `spans` vec would grow past half of u16 max (~32K entries), trigger
an immediate repack that allocates every column the maximum 255 spans. This
ensures the group never overflows `u16` and is only hit in extreme cases
(a group receiving hundreds of edits without a repack).

Columns are **not necessarily contiguous or ordered** in the `spans` vec.
After several move-to-tail operations, the vec may look like:

```
[col_A_data][dead][col_B_data][col_C_data][dead][col_A_new_data][free...]
                                                                 ^free_start
```

This is fine — `ColMeta.data_start` always points to the right place.

### Shrinking

When a column's span count drops well below its allocation (e.g., demolition
clears a complex structure), simply update `num_spans`. The excess allocated
space is harmless dead weight until the next repack reclaims it. Do not
eagerly repack on shrink — it would thrash during alternating build/demolish.

If dead space (allocated but unused spans + abandoned old slots) exceeds a
configurable fraction of the `spans` vec, trigger a compaction repack. The
threshold is a `GameConfig` parameter (default 50%, tunable via JSON).
In practice it's rarely hit because most groups are stable terrain.

## VoxelWorld API

The public API remains unchanged:

```rust
impl VoxelWorld {
    pub fn new(size_x: u32, size_y: u32, size_z: u32) -> Self;
    pub fn in_bounds(&self, coord: VoxelCoord) -> bool;
    pub fn get(&self, coord: VoxelCoord) -> VoxelType;
    pub fn set(&mut self, coord: VoxelCoord, voxel: VoxelType);
    pub fn drain_dirty_voxels(&mut self) -> Vec<VoxelCoord>;
    pub fn clear_dirty_voxels(&mut self);
    pub fn heightmap(&self) -> Vec<u8>;
    pub fn has_solid_face_neighbor(&self, coord: VoxelCoord) -> bool;
    pub fn has_los(&self, from: VoxelCoord, to: VoxelCoord) -> bool;
    pub fn raycast_hits_solid(&self, from: [f32; 3], to: [f32; 3]) -> bool;
    /// Compact all column groups, eliminating dead space and fragmentation.
    /// Call after worldgen or save-load to ensure clean steady-state layout.
    pub fn repack_all(&mut self);
}
```

All existing call sites across the sim crate continue to use `get`/`set`
and see no change.

### `get` Implementation

```rust
pub fn get(&self, coord: VoxelCoord) -> VoxelType {
    if !self.in_bounds(coord) {
        return VoxelType::Air;
    }
    let group = &self.groups[group_index(coord)];
    let col_idx = local_col_index(coord);
    let meta = &group.cols[col_idx];
    let count = meta.num_spans as usize;
    if count == 0 {
        return VoxelType::Air;
    }
    let offset = meta.data_start as usize;
    let spans = &group.spans[offset..offset + count];
    let y = coord.y as u8;

    // Linear search for small span counts, binary for large.
    let voxel_type = if count <= 6 {
        // Linear: find first span where top_y >= y, walking from bottom.
        let mut result = VoxelType::Air; // implicit top air
        for span in spans {
            if y <= span.top_y {
                result = VoxelType::from_u8(span.voxel_type);
                break;
            }
        }
        result
    } else {
        // Binary search: find the first span with top_y >= y.
        match spans.binary_search_by_key(&y, |s| s.top_y) {
            Ok(i) => VoxelType::from_u8(spans[i].voxel_type),
            Err(i) => {
                if i < count {
                    VoxelType::from_u8(spans[i].voxel_type)
                } else {
                    VoxelType::Air // above all explicit spans
                }
            }
        }
    };
    voxel_type
}
```

### `set` Implementation (Sketch)

```rust
pub fn set(&mut self, coord: VoxelCoord, voxel: VoxelType) {
    if !self.in_bounds(coord) {
        return;
    }
    let gi = group_index(coord);
    let col = local_col_index(coord);
    let y = coord.y as u8;

    // 1. Read existing spans for this column into a small scratch buffer.
    // 2. Find the span containing y.
    // 3. If it's already the target type, return (no-op).
    // 4. Split/merge to produce the new span list:
    //    - If y is the only voxel in its span, replace the type.
    //    - If y is at the top of a span, shrink it and insert new.
    //    - If y is at the bottom of a span, shrink it and insert new.
    //    - Otherwise, split the span into three (before, new, after).
    //    - Merge adjacent spans of the same type.
    //    - Trim trailing Air span (implied).
    // 5. Write back:
    //    a. If new count <= num_allocated → write in place.
    //    b. Else if free tail has room → move column to free_start,
    //       allocate generously (new_count + growth margin),
    //       advance free_start. Old slot becomes dead space.
    //    c. Else → full repack: compact all live columns, reset
    //       free_start, grow Vec if needed.
    // 6. Push coord to dirty_voxels.
}
```

The split/merge logic is the most complex part and warrants thorough unit
testing (see Testing section).

## Performance Analysis: `get()` in Hot Paths

The `get()` cost increases from O(1) (flat array index) to O(spans) per
lookup. For typical columns (2–5 spans), the linear scan is ~3–10 comparisons
— likely faster than an L2/L3 cache miss on a large flat array, and the
column metadata fits in a cache line. This is acceptable for most callers.

**Neighbor queries** (`has_solid_face_neighbor`, `has_face_neighbor_of_type`):
6 `get()` calls per invocation. Each call hits a different column (or the same
column at an adjacent Y), so the span search runs 6 times. For 2–5 span
columns this is ~30 comparisons total — comparable to current flat-array
cache-miss patterns for large worlds. No mitigation needed.

**DDA raycasts** (`raycast_hits_solid`, `has_los`): These step voxel-by-voxel,
calling `get()` per step. A long ray (e.g., 50 voxels) means 50 span searches.
For short rays between nearby creatures this is fine. For long-range combat
LOS checks, this is a potential regression.

Mitigations (implement only if profiling shows a problem):
- **Column-local stepping:** When the DDA steps along the Y axis within the
  same column, reuse the last span lookup result — if the new Y is still
  within the same span's range, skip the search entirely.
- **Span-aware vertical skip:** When stepping vertically through a tall
  homogeneous span, jump directly to the span boundary instead of stepping
  voxel-by-voxel.
- **Deferred to Phase 2:** These optimizations can be added after Phase 1
  ships and profiling identifies actual hotspots. The basic `get()` path
  will work correctly at Phase 1 and is likely fast enough.

## Bulk Iteration API

For consumers that iterate large regions (mesh generation, heightmap, initial
nav graph build), add span-level iteration to avoid per-voxel `get` overhead:

```rust
impl VoxelWorld {
    /// Iterate the spans of a single column. Returns (VoxelType, y_start, y_end)
    /// triples covering [0, size_y). The implicit trailing Air span IS included
    /// in the iteration.
    pub fn column_spans(&self, x: u32, z: u32) -> impl Iterator<Item = (VoxelType, u8, u8)>;

    /// Iterate all columns within a chunk footprint. For mesh generation.
    /// Partial groups at world boundaries yield only valid columns.
    pub fn chunk_columns(&self, chunk_x: u32, chunk_z: u32)
        -> impl Iterator<Item = (u32, u32, impl Iterator<Item = (VoxelType, u8, u8)>)>;
}
```

Note: `column_spans` synthesizes the implicit trailing Air span that isn't
stored internally, so callers always see spans covering `[0, size_y)`.
For empty columns (0 stored spans), it yields a single Air span.

Mesh generation uses 16×16×16 3D chunks, so when iterating column spans for
a mesh chunk, the caller must clip each column's spans to the chunk's Y range
(e.g., chunk_y=3 means y=[48, 64)). This clipping is a simple range
intersection on the span's `(y_start, y_end)`.

This lets mesh gen and nav build work in O(spans) instead of O(voxels).
The heightmap becomes trivial: the last non-Air span's `top_y` is the answer.

## Internal Layout

```rust
pub struct VoxelWorld {
    size_x: u32,
    size_y: u32,
    size_z: u32,
    /// Number of groups in each dimension.
    groups_x: u32,
    groups_z: u32,
    /// Flat array of column groups, indexed by gx + gz * groups_x.
    groups: Vec<ColumnGroup>,
    /// Coordinates modified since last drain.
    dirty_voxels: Vec<VoxelCoord>,
}
```

Groups at the world boundary may cover fewer than 16×16 columns (if
`size_x` or `size_z` isn't a multiple of 16). Edge columns outside the
world simply have 0 spans and return Air on `get`.

## Memory Estimates

| World Size | Flat Array | RLE (est.) | Ratio |
|---|---|---|---|
| 256×128×256 (current) | 8.4 MB | ~0.5 MB | 17× |
| 512×128×512 | 33 MB | ~2 MB | 17× |
| 1024×128×1024 | 134 MB | ~8 MB | 17× |
| 4096×128×4096 | 2.1 GB | ~130 MB | 16× |

Estimates assume ~2.5 spans per column average (terrain + scattered tree
geometry). Heavily built areas will have more spans but are a small fraction
of the world.

**Note:** The nav graph's `spatial_index` is a separate flat `Vec<u32>` sized
to `size_x * size_y * size_z` (4 bytes/voxel). At 1024×128×1024 that's
512 MB — larger than the voxel savings. F-nav-gen-opt replaces this with a
`HashMap<VoxelCoord, NavNodeId>`. Without it, the nav index is the memory
bottleneck for large worlds, not voxel storage. F-nav-gen-opt should ship
alongside or shortly after F-rle-voxels before increasing world size.

## Migration Strategy

### Phase 1: New VoxelWorld, Same API

Replace `VoxelWorld` internals. All existing code continues to call `get`/`set`.
Add `repack_all()` calls after worldgen and save-load (in `SimState::new()`
and the load path) to compact groups after bulk writes.
Run the full test suite — every existing test should pass without modification
since the API is unchanged.

### Phase 2: Bulk Iteration for Perf-Critical Paths

Add `column_spans` / `chunk_columns`. Migrate `heightmap()` (trivial),
`mesh_gen.rs` (moderate — already chunk-based), and `build_nav_graph` in
`nav.rs` (F-nav-gen-opt, separate feature) to use span iteration.

### Phase 3: Increase Default World Size

With memory no longer the bottleneck, increase `world_size` in
`default_config.json`. This is F-bigger-world territory and depends on
F-nav-gen-opt also being complete.

## Testing

### Unit Tests (Critical)

The span split/merge logic in `set` is the highest-risk area. Required tests:

**Basic operations:**
- New world is all Air (0 spans everywhere)
- Set single voxel in empty column → 1 span + implied Air
- Get voxel above explicit spans returns Air
- Set and get roundtrip for every VoxelType variant

**Span splitting:**
- Set middle of a span → 3 spans (before, new, after)
- Set bottom of a span → 2 spans (new, rest)
- Set top of a span → 2 spans (rest, new)
- Set the only voxel in a 1-high span → replaces type

**Span merging:**
- Set voxel to same type as span below → merges down
- Set voxel to same type as span above → merges up
- Set voxel between two same-type spans → merges all three
- Set voxel creating Air gap → no spurious merge

**Trailing Air trimming:**
- Setting topmost voxel to Air reduces span count
- Setting topmost voxel to non-Air adds explicit span
- Column becomes all-Air → 0 spans

**Allocation and group management:**
- Column that grows past `num_allocated` moves to free tail
- After move, old slot is dead space, column readable at new location
- Free tail exhaustion triggers full repack
- After repack, all 256 columns readable correctly, `free_start` reset
- Generous allocation on move: column gets growth margin
- Shrink doesn't trigger repack, just updates `num_spans`
- Dead space > 50% threshold triggers compaction

**Compatibility with existing world.rs tests:**
- All existing tests in `world.rs::tests` pass unchanged (they exercise
  the public API which is preserved)

**Bulk iteration:**
- `column_spans` returns correct (type, start, end) triples
- Implicit trailing Air span included
- Empty column yields single Air span covering full height
- `heightmap()` matches flat-array implementation for a complex world

### Stress / Fuzzing

- Randomized set/get sequences on a small world, cross-checked against a
  naive flat array implementation (oracle test)
- Random worlds with many set operations, verify all groups repack correctly

## Resolved Design Decisions

1. **`VoxelType` representation.** Add `#[repr(u8)]` to VoxelType (currently
   16 variants, well within u8). Implement `from_u8()` via a const lookup
   table or match. Add a compile-time assert:
   `const_assert!(std::mem::variant_count::<VoxelType>() <= 256)`.
   This is a prerequisite for Span's `voxel_type: u8` storage.

2. **`data_start: u16` capacity.** Sufficient: worst case is 256 columns ×
   255 spans = 65,280, which fits in u16 (max 65,535). In practice, columns
   rarely exceed 10–20 spans. Dead space from moves is reclaimed on repack
   before the Vec grows large enough to overflow the index.

3. **`Default` impl.** `VoxelWorld::default()` creates a world with 0 groups
   and dimensions (0, 0, 0). `get()` returns Air for all coords. Matches
   current behavior.

## Notes

- **Concurrent read access:** The RLE groups are safe for shared reads
  (immutable `Vec` contents), same as the current flat array. Relevant
  for future multithreaded mesh gen.

- **Save/load:** VoxelWorld is `#[serde(skip)]` on SimState (rebuilt from
  seed). If persistence is ever needed, use proper serde derives on Span
  fields — do not rely on `repr(C)` memory layout for serialization.

- **Linear search threshold:** The `count <= 6` threshold for linear vs
  binary search in `get()` is a tunable const. Profile after implementation
  to find the optimal crossover.
