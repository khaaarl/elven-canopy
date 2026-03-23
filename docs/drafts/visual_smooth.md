# F-visual-smooth: Smooth Voxel Surface Rendering

**Status:** Draft · **Feature:** F-visual-smooth · **Phase:** 2

## Overview

Replace the current flat-quad-per-face chunk mesh generation with a subdivided,
chamfered, and iteratively smoothed mesh. The sim truth remains a discrete voxel
grid — smoothing is purely a rendering concern. This initial pass covers **solid opaque voxels only** (Trunk, Branch, Root,
Dirt, GrownPlatform, GrownWall, Strut); leaves and fruit will be handled
separately in a future iteration due to their different visual requirements
(transparency, alpha-scissor textures).

Texturing of solid voxels (the prime-period tiling noise system in
`texture_gen.rs` and `bark_ground.gdshader`) is **dropped** for now — the new
geometry makes axis-aligned tiling impractical. The old texture code is kept in
the codebase for reference but is not invoked for solid surfaces. Solid voxels
will use vertex colors only (already present in `mesh_gen.rs`). No interim
texturing solution is planned; vertex colors are sufficient until a new
texturing approach is designed for the smooth geometry. Meshes will use **smooth
shading** with explicit vertex normals computed from the final geometry.

## Scope

### In scope
- Subdivided face geometry (8 triangles per visible face)
- Vertex anchoring rules
- Chamfer pass (along vertex normals)
- Iterative curvature-minimizing smoothing
- Voxel border around each chunk for cross-boundary correctness (border
  thickness and iteration count tuned empirically)
- Smooth shading with computed vertex normals
- Chunk-boundary alignment unit test
- Dropping solid-voxel texturing (keeping code for reference)

### Out of scope
- Leaf voxel smoothing (future work)
- New texturing for smooth surfaces (future work)
- Ambient occlusion adaptation (F-voxel-ao, separate feature)
- LOD (F-mesh-lod, blocked by this feature)
- Y-cutoff on smoothed meshes (see Open Questions)

## Geometry

### Current system

Each visible voxel face produces 2 triangles (4 vertices, 6 indices). Faces
between two opaque voxels are culled. This produces a blocky, Minecraft-style
appearance.

### New system: face subdivision

Each visible voxel face produces **8 triangles** from **9 vertices**:

- 4 **corner vertices** (at the voxel face corners, as before)
- 4 **edge-midpoint vertices** (at the midpoint of each edge of the face square)
- 1 **center vertex** (at the center of the face square)

The 8 triangles radiate from the center vertex like slices of a pie, each
connecting the center to two adjacent perimeter vertices (corner, midpoint,
corner, midpoint, ...).

**Vertex sharing:** Corner vertices and edge-midpoint vertices are shared with
adjacent faces where they exist. During mesh construction, vertices at the same
grid position (pre-smoothing integer or half-integer coordinates) are
deduplicated so that adjacent faces reference the same vertex. This ensures the
mesh is watertight and that smoothing operations produce consistent results
across shared geometry.

**1-ring connectivity in the subdivided mesh:** Each vertex's 1-ring (its
edge-connected neighbors) depends on its type:

- **Center vertex:** Connected to all 8 perimeter vertices of its face
  (valence 8). Not shared with other faces.
- **Corner vertex:** Connected to 2 edge-midpoint vertices per incident face.
  At a convex corner shared by 3 faces, that's 6 neighbors (valence 6).
  On an edge shared by 2 faces, valence 4. On a single face, valence 2.
- **Edge-midpoint vertex:** Connected to the center vertex and the 2 adjacent
  corner vertices on each incident face, plus the edge-midpoint vertex of the
  neighboring face if one exists. Typical valence 4–6.

These valences determine the cost of the Laplacian centroid computation per
vertex and are used in the performance estimate below.

**Edge-midpoint vertex normals:** A vertex shared between 2 perpendicular faces
has its initial normal set to the normalized average of those two face normals
(e.g., a vertex on an edge between +Y and +X faces gets normal (1,1,0)/√2).
This is analogous to the corner vertex case described in the Chamfer section.

**Leaf and fruit faces:** Leaf and Fruit voxels are non-opaque and use
alpha-scissor textures. Their faces are **not subdivided** in this pass — they
continue to use the existing 2-triangle flat quads with UVs. At boundaries
between a solid voxel and a leaf/fruit voxel, the solid face is subdivided and
smoothed normally; the leaf/fruit face remains flat. This may produce a slight
visual discontinuity at the boundary, acceptable until leaf smoothing is
implemented.

### Extended chunk buffer

When generating a chunk mesh, the generator reads voxels in a border around the
chunk (e.g., 2 voxels → up to 20×20×20 for a 16×16×16 chunk, clamped to world
bounds). The `generate_chunk_mesh` function already has access to the full
`&VoxelWorld`, so this is a conceptual read radius, not a data-plumbing change.
The border provides:

1. Correct face culling at chunk boundaries (existing behavior)
2. Anchoring context — faces in the border contribute anchoring information
3. Sufficient vertex context for the chamfer and smoothing passes to produce
   results identical to what a whole-world mesh would produce

The mesh output still contains only geometry for the chunk's own 16³ region.
The border is read-only context.

The exact border thickness and iteration count will be **tuned empirically**.
We start with a 2-voxel border and determine via the chunk-boundary alignment
test how many smoothing iterations it supports. If more iterations are desired,
we increase the border to 3 voxels.

## Anchoring

Every vertex carries a boolean **anchored** flag. Anchored vertices are fixed
in place — neither the chamfer pass nor the smoothing passes move them.

### Anchoring rules (applied in order)

1. **Face centers are anchored.** The center vertex of every visible face is
   always anchored. This preserves the face's original plane and prevents
   flat surfaces from drifting.

2. **Non-solid voxel adjacency.** For any face of a solid voxel that
   immediately borders a `BuildingInterior`, `WoodLadder`, or `RopeLadder`
   voxel, **all vertices** of that face are anchored. These non-solid voxel
   types represent constructed interiors and fixtures; the solid faces adjacent
   to them are the visible walls/floors of buildings and should remain sharp and
   architectural. Note: `GrownPlatform`, `GrownWall`, and `Strut` are all
   solid/opaque and participate in smoothing normally — they are natural tree
   growth, not interior fixtures. `GrownStairs` does not exist yet and is not
   relevant to this design.

3. **Insufficient-neighbor anchoring.** After rules 1–2, any vertex that is
   adjacent (by edge) to **fewer than 2 anchored vertices** is itself anchored.
   This catches edge-of-mesh vertices (at world boundaries or isolated
   surfaces) that lack enough constraints for meaningful smoothing, preventing
   edge artifacts.

## Chamfer Pass

**Purpose:** Quickly approximate the common 45° bevel at voxel edges and
corners, giving the subsequent smoothing passes a good starting point.

**Algorithm:** For each non-anchored vertex **v** with ≥2 anchored
edge-neighbors:

1. Compute the offset from **v** to the average position of its anchored
   neighbors:
   ```
   offset = average(anchored_neighbor_positions) - v.position
   ```

2. Project the offset onto **v**'s normal. For a vertex belonging to a single
   face, this is the axis-aligned face normal. For a vertex shared between
   multiple faces, this is the averaged normal (see Edge-midpoint vertex
   normals and Corner vertex normals):
   ```
   displacement = dot(offset, v.normal) * v.normal
   ```

3. Update **v**'s position:
   ```
   v.position += displacement
   ```

**Why project onto the normal?** Unconstrained averaging can pull vertices
laterally — particularly problematic for corner vertices at the edge of the
world, where the average of anchored neighbors isn't in the "inward" direction.
Projecting onto the normal restricts movement to the outward/inward axis of the
original face, which is the correct direction for a chamfer.

**Corner vertex normals:** A corner vertex shared by 3 mutually perpendicular
faces has its normal set to the average of those face normals (e.g., the
(1,1,1)/√3 diagonal for a convex corner). The chamfer thus pulls it inward
along the corner's diagonal — exactly the 45° bevel geometry we want.

**Flat-surface vertices:** For vertices on a flat surface (all anchored
neighbors lie on the same plane as the vertex), the offset will be parallel to
the face and `dot(offset, normal)` will be zero. The chamfer correctly produces
no displacement — flat surfaces are already smooth.

## Curvature-Minimizing Smoothing

**Purpose:** Refine the chamfered geometry toward smoother surfaces. Unlike
Laplacian smoothing (which minimizes each vertex's displacement from its
neighbors), this approach minimizes the **total curvature of the vertex's
neighborhood**, including curvature at anchored neighbors. This allows free
vertices to absorb curvature on behalf of nearby anchored points, avoiding the
"sharp crease at anchors" artifact of Laplacian smoothing.

### Pointiness metric

We use the **Laplacian displacement magnitude** as the pointiness metric:

```
κ(v) = ‖centroid(neighbors(v)) − v.position‖
```

where neighbors(v) are the edge-connected vertices (1-ring). A vertex that lies
exactly at the centroid of its neighbors has κ = 0 (perfectly smooth); a vertex
far from its neighbors' centroid has high κ (pointy). This is purely vector
arithmetic — no trigonometry — making it very cheap to compute.

**Squaring rationale:** The objective squares κ, so the optimization
preferentially attacks the worst offenders. A single very pointy vertex
contributes more to the objective than several mildly pointy ones, which is the
behavior we want — smooth the sharpest features first.

**Saddle point caveat:** The Laplacian metric can underestimate curvature at
saddle points (e.g., valley floors, branch-trunk junctions), because neighbors
curving up and down partially cancel out in the centroid. If saddle artifacts
are visible in practice, we can switch to the **discrete Gaussian curvature via
angle deficit** (κ(v) = 2π − Σθᵢ, where θᵢ are incident triangle angles),
which correctly captures saddle curvature as a negative value that still
contributes when squared. The angle deficit metric requires `acos` calls per
incident triangle and is roughly 10-20× more expensive, so it should only be
adopted if the Laplacian results are insufficient — or potentially used for the
final iteration only, where the fine-detail saddle awareness matters most and
the cost is amortized over fewer/smaller displacements.

### Per-vertex objective

For each non-anchored vertex **v**, the objective is to minimize:

```
Σ κ(u)²   for all u ∈ {v} ∪ neighbors(v)
```

That is, the sum of squared pointiness over **v** and its immediate
edge-connected neighbors (the 1-ring). The Laplacian displacement at each
vertex depends only on that vertex and its immediate neighbors, making this
inherently a 1-ring quantity. Including neighbors in the objective is what lets
free vertices smooth out curvature at anchored points (whose positions can't
change, but whose pointiness depends on the positions of their free neighbors).

### Optimization method

For each non-anchored vertex **v**:

1. Compute **v**'s normal (area-weighted average of incident triangle normals).
2. Sample the objective function at candidate positions along the normal: **v**
   displaced by each of {−2d, −d, 0, +d, +2d} where d is the sample distance
   for the current iteration.
3. Choose the candidate that minimizes the total squared curvature objective.
4. Record the chosen displacement (do not apply yet).

After all vertices have computed their displacements, **apply all displacements
simultaneously** (Jacobi-style). This prevents order-dependence (important for
determinism and chunk-boundary consistency) and avoids over-correction from
vertices chasing each other.

### Iteration schedule

3 smoothing iterations are planned. The sample distance **d** halves each
iteration, providing coarse-to-fine refinement:

- Iteration 1: d = 0.1 voxel-lengths (samples at ±0.2, ±0.1, 0)
- Iteration 2: d = 0.05 voxel-lengths (samples at ±0.1, ±0.05, 0)
- Iteration 3: d = 0.025 voxel-lengths (samples at ±0.05, ±0.025, 0)

The rectangular prism → octagonal prism convergence requires ~0.3 total
displacement at corners. Each iteration can move a vertex up to 2d, giving a
theoretical max of 0.2 + 0.1 + 0.05 = 0.35 over 3 iterations — sufficient.
Exact distances are subject to empirical tuning; performance can be improved
later by reducing the number of samples (e.g., 3 instead of 5) if profiling
shows the smoothing pass is a bottleneck.

### Information causality

Each smoothing iteration propagates influence by **1 vertex hop** — a vertex
moves based on the current (not yet updated) positions of its immediate
neighbors. The chamfer pass is also 1 hop. So:

- Chamfer: 1 hop
- N smoothing iterations: N hops
- Total: N+1 hops

The exact relationship between border thickness (in voxels) and the number of
vertex hops it provides will be determined empirically via the chunk-boundary
alignment test. We start with a 2-voxel border and 3 smoothing iterations; the
test will tell us if this is safe or if we need to adjust.

### Expected behavior: rectangular prism example

A long rod of solid voxels (rectangular prism, square cross-section) has high
curvature (angle deficit ≈ π/2) at its 4 longitudinal edges. The chamfer pass
bevels these edges to 45°. The smoothing iterations redistribute curvature more
uniformly, and the cross-section converges toward a **beveled rectangle
approaching an octagonal profile** — the most uniform curvature distribution
achievable for a convex shape with these constraints (face centers remain
anchored, so the flat faces hold their original planes while corners round off).

## Smooth Shading and Vertex Normals

After all smoothing is complete, compute a **vertex normal** for each vertex as
the **area-weighted average** of the normals of all incident triangles. These
normals are passed to the GPU for smooth (Gouraud/Phong) shading.

Because both chunks sharing a boundary produce identical vertex positions and
see the same incident triangles at the seam, vertex normals will match across
chunk boundaries without special-casing.

## Y-Cutoff Interaction

The current mesh system supports a Y-cutoff that hides geometry above a
threshold (for peeking inside the canopy). Ideally, the Y-cutoff would be
applied **post-smoothing** — the smoothed mesh is cached, and the cutoff is a
separate cached view derived from it. However, cutting an arbitrary smoothed
mesh at a Y plane is more complex than culling axis-aligned voxel faces.

Post-smoothing cutoff is deferred to a follow-up tracker item. The initial
implementation applies cutoff pre-smoothing (at the voxel level, as today).

**Known limitation:** With pre-smoothing cutoff, changing the Y-cutoff
invalidates the smoothed mesh for every chunk that crosses the cut plane,
requiring full regeneration (subdivision → anchoring → chamfer → smoothing).
This will cause lag when scrolling the cutoff up/down.

**Deferred post-smoothing cutoff design notes:** The proper fix is to cache the
smoothed mesh and clip it at an arbitrary Y plane, caching the clipped result
separately. The basic clipping operation is standard polygon-plane clipping
(interpolate new vertices where triangle edges cross the cut plane, emit 1-2
sub-triangles per clipped triangle). The main complications are:

1. **Cap faces.** Clipping leaves open edges along the cut plane. These need to
   be capped to avoid seeing inside the mesh. For a horizontal Y plane, the cap
   vertices all share the same Y coordinate, so 2D triangulation of the cap
   polygon is straightforward.

2. **Per-voxel-material caps.** A single cap polygon can span multiple adjacent
   voxels of different types (e.g., trunk next to dirt). The cap can't be
   triangulated as one surface — it must be split along voxel boundaries so
   each cap triangle gets the correct vertex color for its voxel type. This is
   essentially a 2D boolean intersection of the cap polygon with the voxel
   grid projected onto the cut plane.

3. **Normals at cut edges.** Cap faces need their own normals (pointing up for
   a horizontal cut). Vertices along the cut edge are shared between the
   clipped surface and the cap, so they must be duplicated — one copy with the
   interpolated smooth normal for the surface side, one with (0,1,0) for the
   cap.

These complications make the post-smoothing cutoff a meaningful chunk of work
beyond the core smoothing feature, justifying deferral.

## Performance Estimate

The new system is significantly more expensive per chunk than the current flat
quads:

- **Geometry:** 8 triangles per face (vs 2), ~4× vertex count after sharing.
- **Chamfer:** One pass over all non-anchored vertices. Cheap (averaging +
  dot product per vertex).
- **Smoothing:** 3 iterations × 5 candidate positions × (Laplacian displacement
  computation for vertex + 1-ring neighbors). The Laplacian metric is pure
  vector arithmetic (centroid computation + distance), no trig. For a typical
  interior vertex with ~6 neighbors (see 1-ring connectivity above), each
  evaluation is ~6 additions + a distance calculation. A surface-heavy chunk
  might have ~1500 post-subdivision surface vertices (after deduplication of
  shared corners and edge midpoints). Each iteration requires ~1500 × 5 × 7 ≈
  52K centroid+distance computations. Total across 3 iterations: ~157K
  evaluations — well within budget for
  cached/parallel mesh generation. (If we switch to the angle-deficit metric,
  cost increases ~10-20× due to `acos` calls per incident triangle.)

This is tractable given that mesh generation is already lazy, cached, and
parallelized via rayon in `mesh_cache.rs`. Chunks only regenerate when their
voxels change. Performance will be profiled after implementation and optimized
as needed (e.g., fewer samples, SIMD-friendly angle computation, or reducing
the smoothing radius).

**Determinism note:** Mesh generation is a rendering concern and is explicitly
outside the sim's lockstep determinism contract. The Laplacian metric uses only
basic floating-point arithmetic, but if the angle-deficit metric is adopted
later, it introduces `acos` calls which are not guaranteed to be bit-identical
across platforms.

## Testing

### Chunk-boundary alignment test

Generate hilly terrain (multiple solid voxel types at varying heights). For
each pair of adjacent chunks, independently generate the smoothed mesh for each
chunk (each with its own voxel border). Verify that vertices on the shared
boundary have **identical positions** (within a small floating-point epsilon).

Additionally, increase the iteration count until the test breaks — this
empirically determines the safe iteration limit for a given border thickness
and validates the information-causality analysis. This test is the primary
mechanism for tuning the border-thickness / iteration-count tradeoff.

### Octagonal prism convergence test

Create a long (e.g., 8-voxel) rod of solid voxels in open air. Generate the
smoothed mesh. Verify that the cross-sectional profile at the rod's midpoint
approximates a regular octagon — specifically, that corner vertices have moved
inward and the curvature distribution across the profile vertices is
approximately uniform (acknowledging that anchored face centers keep the flat
sides at their original planes).

## Implementation Plan

### In `mesh_gen.rs`

1. Add the subdivided face geometry generation (8 triangles, 9 vertices per
   face) with vertex deduplication keyed on pre-smoothing grid positions
   (integer coordinates for corners, half-integer for edge midpoints and face
   centers).
2. Implement the anchoring rules.
3. Implement the chamfer pass (project onto vertex normal).
4. Implement the curvature-minimizing smoothing loop.
5. Compute final vertex normals (area-weighted).
6. Drop UV generation and texture sampling for solid surfaces (keep leaf UVs).

### In `sim_bridge.rs`

7. Update `ArrayMesh` construction to pass vertex normals to Godot.
8. Remove or bypass solid-voxel texture setup (keep leaf texture setup).

### In `tree_renderer.gd` / shaders

9. Use a simple vertex-color material for solid surfaces (no texture sampling).
10. Ensure smooth shading is enabled on the mesh material.

### In `mesh_cache.rs`

11. Ensure `generate_chunk_mesh` reads voxels within the required border radius
    (it already has `&VoxelWorld` access; this is a matter of expanding the
    iteration bounds, not plumbing new data).

## Open Questions

- **Border thickness vs iteration count:** Starting with 2-voxel border and 3
  iterations; the chunk-boundary alignment test will determine if this is safe
  or if we need to adjust either parameter.
- **Sample distances per iteration:** Starting with d = 0.1 halving each
  iteration; subject to visual and performance tuning.
- **Leaf voxel treatment:** Deferred to a future iteration. Leaves remain
  flat-quad for now.
- **Y-cutoff on smoothed meshes:** See Y-Cutoff Interaction section. May be
  deferred to a follow-up item if complex.
