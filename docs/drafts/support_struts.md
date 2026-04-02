# F-support-struts — Support Strut Construction Draft (v5)

Design draft for support struts: a construction primitive that places a
diagonal line of solid voxels between two endpoints, with virtual "rod
springs" threading through the line for structurally efficient axial load
transfer. Struts enable knee braces, diagonal supports, truss structures,
and pile-driven foundations.

**Prerequisites:** F-no-bp-overlap (reject overlapping blueprint
designations). Strut-on-strut crossing requires the first strut to be
completed before the second can be designated over its voxels.

**Related future work:** F-batch-construct (batch construction mode) will
allow planning multiple pieces as a unit — e.g., struts + platform +
building validated and built together with a single choral composition.

Design doc reference: §9 (structural integrity — "A diagonal brace under a
platform converts bending stress into compression along the strut,
dramatically reducing stress at the connection").

## Summary of Changes

1. **New `VoxelType::Strut`** — solid voxel type for strut material. Acts
   like wood for rendering, nav graph, collision, and face-adjacency. Has
   its own `MaterialProperties` (face-adjacent springs) in
   `StructuralConfig`, plus separate rod spring config for the chain.
2. **New `BuildType::Strut`** — construction type for strut designation.
   Uses the existing `DesignateBuild` command with `BuildType::Strut` and
   a pre-computed voxel list (3D Bresenham line).
3. **Strut table in SimDb** — tracks strut identity: endpoint pair,
   owning blueprint/structure. Voxel list is recomputed from endpoints
   on demand (Bresenham is deterministic). Used for rod-spring generation
   and carve cleanup.
4. **Rod springs in structural solver** — both the full solver
   (`build_network()`) and the fast validator (`validate_blueprint_fast()`)
   generate additional springs along strut axes in a chain topology
   (each connection point linked to its neighbor along the strut). These
   springs give the strut efficient diagonal load transfer that a staircase
   of ordinary voxels lacks. Players see the structural benefit during
   interactive placement validation.
5. **Strut placement UX** — click-drag flow using the height stepper.
   Mousedown sets endpoint A, drag shows a live ghost preview of the
   voxel line, mouseup sets endpoint B, then confirm or cancel. Consistent
   with existing platform/carve interaction patterns.
6. **Material replacement rules** — strut voxels replace Air, Leaf, Fruit,
   Dirt, Trunk, Branch, Root, ForestFloor, and existing Strut. Placement
   is rejected if any voxel along the line is a player-built structure
   type or belongs to an existing blueprint (F-no-bp-overlap).

---

## Detailed Design

### 1. Voxel Type and Build Type

**File:** `elven_canopy_sim/src/types.rs`

Add `Strut` to `VoxelType`:

```rust
pub enum VoxelType {
    // ... existing variants ...
    /// A support strut voxel. Solid like wood, but carries rod-spring metadata
    /// for efficient diagonal load transfer. Can be placed through natural
    /// materials (dirt, trunk, leaves) and existing struts.
    Strut,
}
```

`Strut` is solid (`is_solid() → true`), opaque (`is_opaque() → true`),
blocks LOS (`blocks_los() → true`), and generates nav nodes like any
other solid type.

**Match arms to update:** Adding a new `VoxelType` variant requires updating
all match arms that handle voxel types. Key locations:
- `is_solid()` — Strut is solid (no change needed, falls through to default)
- `is_opaque()` — Strut is opaque (add to the opaque match list)
- `blocks_los()` — Strut blocks LOS (no change needed, falls through)
- `classify_for_overlap()` — Strut should be classified like other solid
  construction types (Blocked for normal builds, replaceable for struts)
- `to_voxel_type()` on `BuildType` — add `Strut → VoxelType::Strut`
- `CompletedStructure::display_name()` — add a `Strut` case (e.g.,
  "Strut #N")
- `mesh_gen.rs` / `texture_gen.rs` — texture selection for Strut faces
  (distinct wood tint; see §10)

Add `Strut` to `BuildType`:

```rust
pub enum BuildType {
    // ... existing variants ...
    /// A diagonal support strut between two endpoints.
    Strut,
}
```

`to_voxel_type() → VoxelType::Strut`.

**Tree overlap handling:** Struts do **not** use the existing
`allows_tree_overlap()` mechanism. That mechanism classifies tree voxels as
`AlreadyWood` and *skips* them (no blueprint entry, no materialization).
Struts need the opposite behavior: tree voxels are *replaced* by Strut
voxels. The designation handler uses custom replacement validation instead
(see §5).

### 2. Material Replacement Rules

A strut can be placed between any two voxel coordinates as long as every
voxel along the 3D Bresenham line between them has a replaceable type and
is not part of an existing blueprint (F-no-bp-overlap):

| Voxel type | Replaceable? | Notes |
|---|---|---|
| Air | Yes | Common case — strut through open space |
| Leaf | Yes | Strut through canopy |
| Fruit | Yes | Treated like leaf |
| Dirt | Yes | Pile-driven foundations; loses pinned status (see note) |
| Trunk | Yes | Embed into trunk for connection |
| Branch | Yes | Embed into branch |
| Root | Yes | Embed into root |
| Strut | Yes | Strut-on-strut for truss patterns (must be completed, not blueprinted) |
| ForestFloor | Yes | Anchor into ground; loses pinned status (see note) |
| GrownPlatform | **No** | Player-built structure |
| GrownWall | **No** | Player-built structure |
| GrownStairs | **No** | Player-built structure |
| Bridge | **No** | Player-built structure |
| BuildingInterior | **No** | Player-built structure |
| WoodLadder | **No** | Player-built structure |
| RopeLadder | **No** | Player-built structure |

**Pinning note:** Dirt and ForestFloor voxels are pinned in the structural
solver (immovable ground anchors). Replacing them with Strut voxels loses
that pinned status. This is acceptable because strut voxels embedded in
dirt/ground will be face-adjacent to other Dirt/ForestFloor voxels that
remain pinned, which constrains the strut. A strut driven into dirt
effectively "grips" the surrounding pinned earth. If a strut only grazes
the surface (replacing a single ground voxel with open air below), the
strut endpoint is still constrained by adjacent ground. In practice, players
building pile foundations will drive struts several voxels deep, providing
ample contact with pinned terrain.

**Known limitation:** The fast validator's BFS distance-to-ground measures
hops from pinned nodes. A strut voxel replacing ForestFloor is at BFS
distance 1 from its pinned neighbor (not distance 0 as the original
ForestFloor was). For short struts this barely matters; for long pile
foundations the extra hop slightly increases computed stress at the far
end. This is an acceptable approximation for v1.

This means struts can be built into open air first, with platforms added
around them later. Struts can punch through natural terrain (dirt, trunk
wood) for solid anchorage. Completed struts can cross to form trusses.

### 3. Command Integration

**No new command variant.** Strut designation reuses the existing
`DesignateBuild` command:

```rust
DesignateBuild {
    build_type: BuildType::Strut,
    voxels: Vec<VoxelCoord>,  // 3D Bresenham line, computed client-side
    priority: Priority,
}
```

The GDScript placement controller (or a `SimBridge` validation method)
computes the Bresenham line from the two endpoints and sends it as the
voxel list. The designation handler in `sim.rs` detects `BuildType::Strut`
and runs custom replacement validation instead of the normal
`allows_tree_overlap` / `classify_for_overlap` flow.

**Voxel list validation (multiplayer safety):** The designation handler
must verify that the submitted voxel list is a valid Bresenham line, not
an arbitrary collection of voxels. It does this by:
1. Extracting `voxels[0]` and `voxels[last]` as candidate endpoints.
2. Recomputing `VoxelCoord::line_to()` between them.
3. Verifying the recomputed line matches the submitted list (same length,
   same coordinates in order).
4. Rejecting if they don't match.

This prevents a malicious multiplayer client from submitting an arbitrary
voxel scatter as a "strut."

A `SimBridge` query method `validate_strut_preview(endpoint_a, endpoint_b)`
computes the line, checks replacement rules, and returns the voxel list
plus validation tier — mirroring the existing `validate_build_preview()`
/ `validate_carve_preview()` pattern.

### 4. 3D Bresenham Line Rasterization

**File:** `elven_canopy_sim/src/types.rs` (or a small utility module)

A 3D Bresenham implementation that returns all voxel coordinates along the
line from `endpoint_a` to `endpoint_b`, inclusive.

**Symmetry requirement (CRITICAL):** `a.line_to(b)` must produce the exact
same voxel set as `b.line_to(a)`. Standard 3D Bresenham does not guarantee
this. The implementation must enforce symmetry — e.g., by always iterating
from the lexicographically smaller endpoint (compare x, then y, then z to
break ties). This prevents subtle bugs where reversed endpoints produce
different voxel footprints.

**Tie-breaking:** When two axes have equal step sizes (e.g., dx == dy),
the algorithm must break ties deterministically with explicit, documented
rules (required for simulation determinism).

The line must include both endpoints. Minimum strut length is 2 voxels
(the two endpoints themselves). No maximum length — constrained only by
world bounds.

```rust
impl VoxelCoord {
    /// Returns all voxel coordinates along the 3D Bresenham line from
    /// `self` to `other`, inclusive of both endpoints.
    /// Guarantees symmetry: `a.line_to(b)` produces the same set of
    /// coordinates as `b.line_to(a)` (though order may differ).
    pub fn line_to(self, other: VoxelCoord) -> Vec<VoxelCoord> {
        // 3D Bresenham — always iterate from lexicographically smaller endpoint
    }
}
```

### 5. Designation Handler

**File:** `elven_canopy_sim/src/sim.rs`

When `designate_build()` receives `BuildType::Strut`, it branches into
strut-specific logic rather than the normal platform/wall flow:

1. **Validate the voxel list:** Length ≥ 2. Extract endpoints from
   `voxels[0]` and `voxels[last]`, recompute the Bresenham line, verify
   it matches the submitted list. Reject on mismatch.
2. **Blueprint overlap check (F-no-bp-overlap):** Reject if any voxel in
   the line belongs to an existing blueprint. This is enforced globally
   for all build types by F-no-bp-overlap, but listed here for clarity.
3. **Adjacency pre-check:** At least one endpoint must be face-adjacent to
   a solid voxel (overlay-aware — checking both world and blueprint overlay
   types, consistent with the existing `designate_build()` adjacency check
   which uses `effective_type()` and overlay neighbors). This cheaply
   rejects struts placed entirely in open air while allowing a strut to
   anchor against a designated-but-unbuilt neighboring structure.
4. **Replacement validation:** For each voxel in the line, check the world
   type. Reject if any voxel is a non-replaceable type per the table in §2.
5. **Record original voxels:** For each voxel that is not Air, record its
   current type in the blueprint's `original_voxels` list. This enables
   cancel to restore replaced materials (Trunk, Dirt, existing Strut at
   crossing points, etc.). See §8 for cancel flow.
6. **Build voxels include shared Strut voxels.** Do NOT filter out voxels
   that are already `VoxelType::Strut`. Include them in the blueprint's
   voxel list so the cancel handler can iterate them and restore to the
   original type (VoxelType::Strut, preserving the first strut). The build
   task writes Strut over Strut at these positions — a no-op in effect.
7. **Create blueprint:** `Blueprint` with `build_type: BuildType::Strut`
   and the full voxel list. The `original_voxels` field records what
   was replaced (for cancel restoration).
8. **Create Strut row** in SimDb recording the endpoint pair and
   blueprint ID.
9. **Structural validation:** Build a `BlueprintOverlay` and run
   `validate_blueprint_fast()`, passing strut data so rod springs are
   included. The overlay includes the strut's planned voxels so the
   validator sees the full structural picture.
10. If validation passes (Ok or Warning), spawn a build task.

**Construction music:** Like all blueprint designations, a strut
automatically gets a `MusicComposition` via the existing
`create_composition()` call. No special handling needed.

### 6. Strut Table in SimDb

**File:** `elven_canopy_sim/src/db.rs`

```rust
/// A support strut — a diagonal line of voxels with rod-spring metadata.
/// The voxel list is not stored; it is recomputed from the endpoints via
/// `endpoint_a.line_to(endpoint_b)` (Bresenham is deterministic).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Strut {
    #[primary_key(auto_increment)]
    pub id: StrutId,
    /// First endpoint of the strut line (lexicographically smaller).
    pub endpoint_a: VoxelCoord,
    /// Second endpoint of the strut line.
    pub endpoint_b: VoxelCoord,
    /// The blueprint that builds this strut (if not yet complete).
    #[indexed]
    pub blueprint_id: Option<ProjectId>,
    /// The completed structure this strut belongs to.
    #[indexed]
    pub structure_id: Option<StructureId>,
}
```

A `StrutId` newtype (auto-increment, created at designation time) identifies
each strut.

**FK policies:**
- `blueprint_id → blueprints`: **cascade delete.** When a blueprint is
  cancelled (`CancelBuild`), the Strut row is deleted. The cancel handler
  restores original voxels from `blueprint.original_voxels`, and with the
  Strut row gone, no orphan strut metadata remains.
- `structure_id → completed_structures`: **nullify.** If a completed
  structure is somehow removed (future demolition system), the Strut row
  persists with `structure_id: None`. The strut voxels still exist in the
  world and rod springs still function — the structure registration is
  just bookkeeping.

**Voxel-to-strut lookup:** To find which strut(s) own a given voxel (needed
for carve/damage), iterate all struts and check if the coord lies on the
line. For small strut counts this is fine. If it becomes a bottleneck, add
a `BTreeMap<VoxelCoord, Vec<StrutId>>` reverse index on SimState.

**CompletedStructure integration:** When a strut build completes, a
`CompletedStructure` is created via the existing `from_blueprint()` factory.
The bounding box will span the strut's diagonal extent (e.g., a 10-voxel
diagonal strut from (0,0,0) to (9,9,9) has a 10x10x10 bounding box).
This is cosmetically odd but not player-visible for struts.
`display_name()` needs a `BuildType::Strut` arm returning "Strut #N".

### 7. Rod Springs in Structural Solver

**File:** `elven_canopy_sim/src/structural.rs`

Both the full solver (`build_network()` + `solve()`) and the fast validator
(`build_network_from_set()` + `compute_weight_flow_stress()`) must include
rod springs. This is critical: `validate_blueprint_fast()` is what players
see during interactive placement. If rod springs aren't in the fast path,
players never see the structural benefit of their struts.

#### Rod spring topology: chain model

Rod springs use a **chain** topology: each connection point along the strut
is linked to its immediate neighbor, not to all other connection points.

Given a strut with N voxels and connection spacing S:
- Connection points are at voxel indices 0, S, 2S, ..., and the last voxel
  (if not already a multiple of S).
- A rod spring connects each consecutive pair of connection points.
- Total rod springs per strut: O(N/S) — linear, not quadratic.

Why chain, not all-pairs or a single long spring:
- **Physical accuracy:** A real beam transfers load at every point along
  its length. A chain of springs does this — load enters at any node and
  propagates along the chain. A single endpoint-to-endpoint spring would
  bypass all intermediate nodes; anything attached to the middle of the
  strut wouldn't feel the rod's benefit. All-pairs connections
  over-stiffen the strut and create unrealistic fully-connected subgraphs.
- **Length-dependent stiffness:** A chain of N/S springs in series has
  effective end-to-end stiffness k_chain = k_link × S / N. Longer struts
  are naturally more flexible, which is physically correct. (This formula
  describes end-to-end behavior; intermediate nodes collect additional
  load from laterally attached voxels, which the solver handles naturally.)
- **Manageable spring count:** A 20-voxel strut with spacing=2 generates
  10 rod springs. A truss of 5 crossing 20-voxel struts adds 50 rod
  springs total.

**Multi-chain voxels (trusses):** When two struts cross at a voxel, that
voxel participates in both chains independently. Each `Strut` row generates
its own chain of rod springs; the shared voxel's node accumulates springs
from both chains. No special "multi-strut voxel" type is needed — the
node at the crossing just has more springs attached to it, creating a
stronger junction. This falls out naturally from the data model.

#### Full solver integration

After the existing spring-generation pass (face-adjacent pairs), add a
second pass for strut rod springs. Factor this into a shared helper
function (`add_rod_springs()`) used by both `build_network()` and
`build_network_from_set()` to avoid duplicating the rod spring logic.

1. Accept strut data as a parameter: `struts: &[Strut]`.
2. For each strut, recompute its voxel line from endpoints.
3. **Integrity check (overlay-aware):** Verify every voxel along the line
   is `VoxelType::Strut` in the effective type. The helper checks against
   the BFS visited set (which already merges world + overlay + proposed
   voxel types), not a separate lookup — this is the same voxel map that
   `build_network_from_set()` constructs during its BFS pass. If any
   voxel has been carved or is not yet Strut (and not in the overlay or
   proposed set), skip rod spring generation for this strut entirely.
   Using overlay-aware types ensures that designated-but-unbuilt struts
   contribute rod springs during validation of subsequent designations —
   so a player who designates a strut and then a platform sees the
   strut's benefit immediately.
4. Compute connection points at every Sth voxel along the line.
5. For each consecutive pair of connection points, add a rod spring.

Rod spring properties:
- **Rest length:** Euclidean distance between the two connection point
  coordinates (voxel centers). Computed exactly from the actual positions,
  not approximated.
- **Stiffness:** `strut_rod_stiffness` from config (per-link stiffness).
- **Strength:** `strut_rod_strength` from config.

**Note on convergence:** Rod springs are the first non-face-adjacent springs
in the system (rest_length > 1.0). The Gauss-Seidel damping
(`damping_factor / local_stiffness`) may need re-tuning since long springs
couple distant nodes. Monitor for oscillation or slow convergence during
testing and adjust `damping_factor` or iteration count if needed.

#### Fast validator integration

`validate_blueprint_fast()` uses BFS distance-to-ground and weight-flow
analysis. Rod springs create shortcuts in the BFS distance graph: a rod
spring connecting a distant node to a nearer-to-ground node reduces the
distant node's effective BFS distance, which is exactly the strut's
intended structural effect.

The fast validator's `node_springs` adjacency list must include rod springs
so the BFS and weight-flow passes see them.

**Load distribution at suggested defaults:** At a node where a strut rod
spring (k=150) meets a face-adjacent platform spring (k=20), the rod
spring carries 150/170 ≈ 88% of the load. At a node with 3 face-adjacent
springs (k=20 each, total 60), the rod carries 150/210 ≈ 71%. This is
the intended behavior: struts are load-bearing shortcuts that take the
lion's share but don't completely zero out face-adjacent contributions.

The `build_network_from_set()` function needs the same strut data parameter
as `build_network()`, using the shared `add_rod_springs()` helper.

**Strut data query:** At each call site (`designate_build()`,
`designate_carve()`), query **all Strut rows from SimDb regardless of
completion state** — both completed struts (`blueprint_id: None`,
`structure_id: Some`) and designated-but-unbuilt struts (`blueprint_id:
Some`). The integrity check (step 3 above) uses the BFS visited set to
determine whether each strut's voxels are effectively Strut, so
incomplete struts whose voxels appear in the overlay or proposed set will
correctly contribute rod springs.

**`validate_carve_fast()` also needs strut data.** When a player carves
voxels near or through a strut, the carve validator must see the rod springs
to correctly assess the structural impact of the carve. Pass strut data
through the same parameter path. The strut data is queried from the SimDb
Strut table at each call site (`designate_build()`, `designate_carve()`)
and passed down through the validation functions.

#### Config additions

**File:** `config.rs` / `StructuralConfig`

**Strut face-adjacent material properties** (in the `materials` BTreeMap):

```rust
materials.insert(
    VoxelType::Strut,
    MaterialProperties {
        density: 0.6,       // Same as GrownPlatform (player-built wood)
        stiffness: 25.0,    // Slightly stiffer than GrownPlatform (20.0)
        strength: 12.0,     // Slightly stronger than GrownPlatform (8.0)
    },
);
```

These govern the face-adjacent springs between strut voxels (and between
strut voxels and their non-strut neighbors). They make strut voxels roughly
comparable to grown platform wood — sturdy enough as individual cubes, but
the real structural strength comes from the rod springs.

**Rod spring config** (new fields on `StructuralConfig`):

```rust
/// Per-link stiffness of rod springs along a strut's axis. The effective
/// end-to-end stiffness of a strut is k_link × spacing / N, so longer
/// struts are naturally more flexible (physically correct).
pub strut_rod_stiffness: f32,     // Default: 150.0
/// Per-link strength of rod springs along a strut's axis.
pub strut_rod_strength: f32,      // Default: 150.0
/// Spacing (in voxels) between rod spring connection points along a strut.
/// E.g., 2 means connection points at every 2nd voxel. Lower spacing
/// means more springs (O(N/spacing) per strut) and stiffer behavior.
pub strut_rod_spacing: u32,       // Default: 2
```

At per-link stiffness of 150.0, rod springs are ~7.5× stiffer than
GrownPlatform face-adjacent springs (20.0) and ~0.075× Trunk (50000.0) or
~0.075× Branch (2000.0). Struts are strong player-built supports but don't
rival ancient tree wood. Tuning will be needed via playtesting.

### 8. Cancel and Carve

#### Cancel (blueprint not yet complete)

When `CancelBuild` is issued for a strut blueprint:
- The Strut row is cascade-deleted (FK policy from §6).
- Already-materialized Strut voxels are restored to their original types
  from `blueprint.original_voxels` (the standard cancel restoration flow).
- Unmaterialized voxels are still their original type — no action needed.

This means cancelling a partial strut through trunk wood restores the trunk
voxels that were already converted, and leaves not-yet-converted trunk
voxels untouched. The `original_voxels` tracking in step 5 of §5 is
what makes this work.

**No overlapping-blueprint cancel conflicts:** Because F-no-bp-overlap
prevents any two blueprints from sharing voxels, cancelling one strut
cannot affect another strut's blueprint. Strut-on-strut crossings are only
possible with completed struts (whose voxels are already materialized and
not part of any blueprint). See §12.

#### Carve (completed strut)

When a completed strut voxel is carved (removed to Air):

- The voxel becomes Air (standard carve behavior).
- Rod spring generation automatically detects the break: the integrity
  check in §7 (step 3 of full solver integration) finds a non-Strut
  voxel along the line and skips all rod springs for that strut. No
  mutable state flag is needed — just check reality at generation time.
- The remaining Strut voxels still provide face-adjacent spring support,
  just without the efficient axial load transfer.
- `validate_carve_fast()` receives strut data and correctly assesses the
  impact. When a carve breaks a strut's integrity, the fast validator sees
  the loss of rod springs and reports higher stress on structures that
  depended on the strut.

**Carving through replaced trunk:** A strut that replaced trunk voxels
reverts those positions to Air on carve (the original trunk is consumed).
After construction completes, the original-type information from the
blueprint is no longer available (blueprints are consumed on completion),
so the carve tooltip can only say "This is a strut voxel" — not "this
replaced trunk." Per-voxel original-type tracking on the Strut row is a
possible future enhancement (see open questions).

**Strut-severing exploit:** A player could build a strut through the tree's
load-bearing trunk and then carve it to leave an air gap where trunk used
to be. This is mitigated by `validate_carve_fast()`, which runs structural
validation before confirming a carve. If the carve would create a
structural failure (disconnected or overstressed), it is blocked or warned
just like any other dangerous carve.

### 9. Strut Placement UX

**File:** `godot/scripts/construction_controller.gd`

Click-drag placement flow, consistent with existing platform/carve
interaction. No new state machine states needed — struts use the existing
HOVER → DRAGGING → PREVIEW flow with `build_mode = "strut"`.

1. **Select strut mode** from the construction toolbar (new button).
2. **Mousedown** on the height-stepper grid sets `endpoint_a`.
3. **Drag** — a live ghost preview of the voxel line (3D Bresenham from
   `endpoint_a` to the current cursor position) updates as the mouse
   moves. The height-stepper determines the Y coordinate of endpoint B.
   Invalid voxels along the line are highlighted red. Validation runs
   continuously using `validate_strut_preview()`.
4. **Mouseup** sets `endpoint_b` and enters the PREVIEW state. The ghost
   shows the final line with structural tier coloring (blue=ok,
   orange=warning, red=blocked).
5. **Enter** confirms → sends `DesignateBuild` command. **ESC** cancels.

**Height at different endpoints:** The two endpoints may need different Y
values (that's the whole point of diagonal struts). The height-stepper Y
at mousedown determines endpoint A's Y, and the height-stepper Y at
mouseup determines endpoint B's Y. The player adjusts the height-slice
between mousedown and mouseup using the existing scroll/+/- controls.
This is analogous to how carving works (the height changes during drag to
set the prism height).

**Ghost preview rendering:** The existing ghost is a single scaled BoxMesh,
which doesn't work for a diagonal line of voxels. The strut ghost uses a
**MultiMesh** pool:

- Create a `MultiMeshInstance3D` with a unit-cube mesh and a max instance
  count (e.g., 100 — sufficient for most struts).
- During drag, recompute the Bresenham line each frame, set
  `visible_instance_count` to the line length (clamped to pool size), and
  position each instance at its voxel center (+0.5 offset).
- Color instances based on validation: blue for valid replaceable voxels,
  red for invalid/blocked voxels.
- If the line exceeds the pool size, show only the first N voxels and
  display a "(too long)" indicator. In practice, useful struts are rarely
  longer than 30 voxels.
- Validation caching: track the previous `(endpoint_a, endpoint_b)` pair
  and skip revalidation if unchanged (same pattern as the existing
  anchor+dimensions change detection).

**Endpoint A marker:** When the height-slice changes during drag (endpoint
B at a different Y than endpoint A), endpoint A's ghost voxel should be
visually distinct (e.g., brighter color or wireframe outline) so the player
can see where it was placed even when the grid plane has moved to a
different height.

### 10. Rendering

**File:** `elven_canopy_graphics/src/mesh_gen.rs` (chunk mesh system)

Strut voxels are solid and participate in the existing chunk-based mesh
generation. They render as wood-colored cubes, identical to GrownPlatform
or Branch visually. No special rendering needed — the staircase of cubes
is the visual representation. The structural magic is invisible (rod
springs are a simulation concept, not rendered).

A distinct texture tint for Strut voxels (slightly different wood color)
would help players visually distinguish struts from platforms and natural
wood. This can be done via the existing per-VoxelType texture generation
in `texture_gen.rs`. Add a Strut case to the texture type selection in
`mesh_gen.rs`.

### 11. Nav Graph Impact

Strut voxels are solid, so they create nav nodes on adjacent air voxels
(creatures can walk on top of or cling to struts). The nav graph must be
rebuilt after strut construction completes, same as any other build type.
This is already handled by the existing construction completion flow.

### 12. Strut-on-Strut (Trusses)

**Prerequisite:** F-no-bp-overlap means strut-on-strut crossings are only
possible when the first strut is already completed (materialized as
`VoxelType::Strut`). You cannot designate two overlapping strut blueprints
simultaneously. For ensemble construction (multiple struts planned at once),
see F-batch-construct.

When a new strut's line passes through completed `VoxelType::Strut` voxels:

- Those voxels are included in the blueprint's voxel list (step 6 of §5).
  The build task writes Strut over Strut at these positions (a no-op in
  effect). The `original_voxels` list records their type as
  `VoxelType::Strut` (step 5 of §5).
- On cancel, the cancel handler iterates `bp.voxels`, sees these shared
  voxels, and restores them to `VoxelType::Strut` from `original_voxels`
  — preserving the first strut. This works because the cancel handler
  iterates `bp.voxels` (not `original_voxels`), so shared voxels must be
  in `bp.voxels` to be visited.
- Both struts independently generate their own rod spring chains through
  the shared voxels (see "Multi-chain voxels" in §7). The shared node
  accumulates springs from both chains.
- Carving a shared voxel compromises rod springs for **all** struts passing
  through it (the integrity check fails for each).

This enables triangulated truss patterns where multiple completed struts
share intersection points, creating very strong structures.

---

## Open Questions

1. **Original voxel memory on carve:** When a completed strut is carved,
   voxels revert to Air (the replaced material is consumed). If this
   feels wrong in playtesting (e.g., carving a strut through trunk should
   restore trunk), we can add per-voxel original-type tracking on the
   Strut row. Not needed for v1 since cancel already restores originals
   via the blueprint's `original_voxels`.

2. **Construction time:** Should strut build time scale with length? A
   20-voxel strut should take longer than a 3-voxel one. The existing
   build task uses per-voxel work, so this may already fall out naturally
   from the blueprint's voxel count.

3. **Strut material cost:** Currently construction is free (no mana/resource
   cost). When the mana economy is implemented, struts should have a cost
   proportional to length. Not needed for v1.

4. **Maximum length:** No maximum length is enforced. A very long strut
   (e.g., 100 voxels) would have a large blueprint, long build time, and
   O(N/spacing) rod springs. The ghost MultiMesh pool caps the preview at
   ~100 voxels. If uncapped length causes problems in practice, add a
   configurable max in the `validate_strut_preview()` bridge method. Not
   needed for v1.

---

## Test Plan

1. **3D Bresenham correctness:** Unit tests for `VoxelCoord::line_to()` —
   axis-aligned lines, 45-degree diagonals, arbitrary slopes, single-point
   (a to a), length-1 rejection, and symmetry (a→b produces same voxel
   set as b→a). Explicit tie-breaking tests for dx==dy cases.
2. **Replacement validation:** Test that designation succeeds when the line
   passes through Air, Dirt, Trunk, Leaf, Root, Branch, completed Strut,
   ForestFloor. Test that designation is rejected when the line hits
   GrownPlatform, GrownWall, BuildingInterior, WoodLadder, etc.
3. **Bresenham list validation:** Submit a voxel list that doesn't match
   its endpoints' Bresenham recomputation. Verify rejection.
4. **Blueprint overlap rejection:** Designate a strut, then attempt to
   designate another build overlapping the first strut's blueprint voxels.
   Verify rejection (F-no-bp-overlap).
5. **Adjacency pre-check:** Designate a strut entirely in open air (no
   endpoint face-adjacent to solid). Verify rejection.
6. **Original voxel tracking:** Designate a strut through Trunk, verify
   `blueprint.original_voxels` records the Trunk entries.
7. **Cancel restoration:** Designate a strut through Trunk, partially
   materialize it, then cancel. Verify materialized Strut voxels revert
   to Trunk (from `original_voxels`), unmaterialized voxels unchanged.
8. **Blueprint creation:** Designate a strut via `DesignateBuild` with
   `BuildType::Strut`, verify blueprint is created with correct voxel
   list. Verify a `Strut` row is created in SimDb with correct endpoints.
9. **Materialization:** Complete a strut build task, verify all line voxels
   become `VoxelType::Strut`.
10. **Rod springs (full solver):** Build a network with a completed strut,
    verify that chain-topology rod springs exist along the strut axis.
    Verify spring count is O(N/spacing), not O(N²).
11. **Rod springs (fast validator):** Verify that `validate_blueprint_fast()`
    sees strut rod springs and produces lower stress for a platform with
    a diagonal strut vs. without.
12. **Rod springs (overlay-aware):** Designate a strut (not yet built),
    then validate a platform that depends on it. Verify the fast validator
    sees the designated strut's rod springs via the overlay.
13. **Structural benefit:** Compare max stress of a cantilevered platform
    with and without a diagonal strut underneath. The strutted version
    should show meaningfully lower peak stress.
14. **Solver convergence:** Run `solve()` with rod springs and verify
    `max_stress_ratio` is finite and reasonable (no oscillation/divergence).
15. **Strut-on-strut:** Complete strut A, then designate strut B crossing
    it. Verify both generate independent rod spring chains. Verify the
    shared node has springs from both chains. Carve the shared voxel,
    verify both struts lose their rod springs.
16. **Strut-on-strut cancel:** Complete strut A, designate strut B crossing
    it, cancel strut B. Verify the shared voxel reverts to Strut (from
    `original_voxels`), not Air.
17. **Carve:** Carve a strut voxel mid-line, verify rod springs are
    removed (integrity check fails) but remaining voxels stay solid.
18. **Carve validation with strut data:** Carve a voxel that breaks a
    load-bearing strut. Verify `validate_carve_fast()` reports increased
    stress / structural warning.
19. **Carve through replaced trunk:** Build and complete a strut through
    trunk, carve the strut voxel. Verify it becomes Air (not Trunk).
20. **Pinning behavior:** Build a strut that passes through Dirt. Verify
    the strut voxels are not pinned but adjacent Dirt voxels remain
    pinned, constraining the strut.
21. **FK cascade on cancel:** Cancel a strut blueprint, verify the Strut
    row is cascade-deleted from SimDb.
22. **Serde roundtrip:** `StrutId`, `Strut` table, `BuildType::Strut`
    variant in `DesignateBuild`.
23. **Save/load roundtrip:** Save a game with completed struts and
    designated strut blueprints. Load and verify Strut table, rod spring
    generation, and structural validation all work correctly.
24. **Nav graph:** Verify nav nodes are created adjacent to strut voxels
    after construction completes.
25. **CompletedStructure display:** Verify completed strut shows
    "Strut #N" in display_name().
26. **Strut MaterialProperties:** Verify Strut voxels use the correct
    face-adjacent material properties (density, stiffness, strength) from
    the config, not defaulting to zero.

---

## Implementation Phases

### Phase A: Core Types and Line Algorithm
- Add `VoxelType::Strut`, `BuildType::Strut`
- Update all match arms (`is_solid`, `is_opaque`, `blocks_los`,
  `classify_for_overlap`, `to_voxel_type`, `display_name`,
  `mesh_gen.rs`/`texture_gen.rs` texture selection)
- Implement `VoxelCoord::line_to()` (3D Bresenham with symmetry guarantee)
- Add Strut MaterialProperties to `StructuralConfig` defaults
- Unit tests for line algorithm and type properties

### Phase B: Designation and Construction
- Add strut branch in `designate_build()` with custom replacement validation
- Bresenham list validation (recompute and verify)
- Blueprint overlap check (F-no-bp-overlap, may already be implemented)
- Adjacency pre-check
- Original voxel tracking for cancel restoration
- Add `Strut` table to SimDb with FK policies (cascade on blueprint,
  nullify on structure)
- Blueprint creation and build task flow
- Materialization (voxels become `VoxelType::Strut`)
- CompletedStructure display_name for Strut
- `validate_strut_preview()` bridge method
- Tests for designation, validation, cancel, materialization

### Phase C: Rod Springs
- Factor out shared `add_rod_springs()` helper for both `build_network()`
  and `build_network_from_set()`
- Chain-topology rod springs with overlay-aware integrity check
- Extend `validate_carve_fast()` to accept and use strut data
- Add `strut_rod_stiffness`, `strut_rod_strength`, `strut_rod_spacing`
  to `StructuralConfig`
- Structural benefit test (cantilever with/without strut, both solver paths)
- Overlay-aware rod spring test (designated strut benefits subsequent
  platform validation)
- Convergence test with mixed rest lengths
- Strut-on-strut rod spring behavior (multi-chain nodes)

### Phase D: Placement UX
- Add strut placement to `construction_controller.gd` (click-drag flow,
  `build_mode = "strut"` using existing state machine)
- Height-stepper Y adjustment during drag for diagonal endpoints
- Ghost preview via MultiMesh pool (unit cubes, max ~100 instances)
- Endpoint A visual marker (distinct from line voxels)
- Validation caching keyed on `(endpoint_a, endpoint_b)`
- Validation feedback (color-coded, rejection on illegal voxels)
- Toolbar button / keybind for strut mode

### Phase E: Carve Interaction
- Verify integrity-check-based rod spring removal works on carve
- Strut-severing carve validation (validate_carve_fast with strut data)
- Voxel-to-strut reverse lookup (iterate-all for v1)
