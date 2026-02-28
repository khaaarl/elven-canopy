# Structural Integrity — Design Draft

Structural analysis for construction validation and tree generation
verification. Covers the spring-mass network model, building face integration,
material properties, blueprint validation UX, and tree generation soundness
checks.

**Tracker items:** F-voxel-fem (primary), F-struct-basic (prerequisite),
F-stress-heatmap (downstream), F-cascade-fail (out of scope — see §11)

**Design doc reference:** §9 (Structural Integrity)

---

## 1. Goals and Scope

**In scope:**

- Spring-mass network structural solver operating on the voxel grid.
- Solid voxels as mass-spring elements.
- Building faces (Wall, Window, Door, Floor, Ceiling) as shell-like spring
  elements in the same solver.
- Material properties per voxel type and per face type (data-driven via
  GameConfig).
- Tree voxels participate with very high but finite strength.
- Tree generation validation: reject structurally unsound generated trees
  (retry up to 4 attempts, then abort with error).
- Blueprint validation at designation time: tiered enforcement (hard block
  for extreme failures, warning for moderate stress).
- Stress heatmap overlay for blueprint preview (F-stress-heatmap).

**Out of scope (separate future features):**

- Runtime structural failure and cascading collapse (F-cascade-fail). This
  draft covers the solver and blueprint validation only. Runtime events like
  fire destroying a load-bearing voxel, triggering a branch to fall, are a
  separate feature that builds on this solver but adds fall physics, debris,
  impact damage, and creature evacuation. The solver designed here is
  *compatible* with that future use — it can be called on demand when the
  world changes — but this draft does not specify the failure/cascade
  mechanics.
- Dynamic creature loading. The initial system checks structural integrity
  under static gravity load only. Future work could track creature positions
  and add their weight to the analysis (30 elves on a platform tip), but this
  draft treats creature weight as negligible for the initial implementation.

---

## 2. Why Spring-Mass, Not Classical FEM

The design doc (§9) describes two approaches: direct sparse solve (global
stiffness matrix) and iterative relaxation. Both are mathematically valid, but
choosing to support **building face shell elements** alongside solid cube
elements creates a DOF mismatch problem for classical FEM:

- Solid (cube) elements have **3 DOFs per node** (x, y, z translation).
- Shell elements have **5–6 DOFs per node** (3 translations + 2–3 rotations).
- Coupling these at shared nodes requires multi-point constraints, which adds
  significant complexity to the global stiffness matrix assembly and solve.

A **spring-mass network** solved by iterative relaxation sidesteps this
entirely. Every element — solid voxel or building face — contributes springs
and masses to the same network. The solver iterates over nodes, computing net
forces from connected springs and adjusting positions toward equilibrium. No
global matrix, no DOF mismatch, no sparse solver dependency.

The trade-off: iterative relaxation converges more slowly than a direct solve
for large structures. But for gameplay-scale structures (hundreds to low
thousands of voxels), convergence in ~50–200 iterations is fast enough. And
the simplicity advantage is enormous — the core solver is perhaps 100 lines of
Rust, compared to several hundred for a sparse matrix FEM implementation.

**Fixed iteration budget:** Rather than iterating to convergence (which could
vary between runs and complicate determinism), use a fixed iteration count.
If the system hasn't converged after N iterations, the approximate answer is
good enough for gameplay. The fixed count also makes performance predictable.

---

## 3. The Spring-Mass Model

### 3.1 Nodes

Every structural element has a **node** — a point mass at its center with a
3D position. Nodes come from two sources:

- **Solid voxels.** Every solid voxel (Trunk, Branch, Root, GrownPlatform,
  GrownWall, GrownStairs, Bridge, ForestFloor) gets a node at its center
  position `(x + 0.5, y + 0.5, z + 0.5)`. The node's mass comes from the
  material's density.

- **Building interior voxels.** Every `BuildingInterior` voxel gets a node.
  Its mass represents the implied weight of the building's contents and the
  face elements attached to it (see §4). The node itself has no structural
  stiffness — it's a mass point connected to its neighbors only through face
  springs.

Nodes that are **grounded** — ForestFloor voxels, or any voxel at the
boundary conditions — are pinned: their position is fixed and they absorb
any forces applied to them. They act as rigid anchors.

### 3.2 Springs

Springs connect pairs of nodes and resist relative displacement. Each spring
has:

- **Stiffness** `k` — force per unit displacement. Higher k = stiffer
  material.
- **Strength** `s` — maximum force before the spring fails. When the force
  in a spring exceeds `s`, it indicates structural failure at that connection.
- **Rest length** — the natural length of the spring. Always 1.0 for
  face-adjacent voxels (the only type that gets springs — see below).

Springs come from two sources:

- **Solid-to-solid adjacency.** Two face-adjacent solid voxels are connected
  by a spring. The spring's stiffness is the harmonic mean of the two
  materials' stiffness values: `k = 2 * k1 * k2 / (k1 + k2)`. Strength is
  similarly the minimum of the two materials' strengths. Only face-adjacent
  (6-connectivity) pairs get springs — not edge or corner adjacency. Rationale:
  face-adjacent voxels share a full face of contact area; edge/corner adjacency
  has zero or negligible contact in a cube geometry.

- **Building face springs.** See §4.

### 3.3 Gravity Load

Every node has a downward force: `F_gravity = mass * g`, where `g` is a
config parameter (`structural_gravity`, defaulting to 1.0 — unitless, tuned
for gameplay feel). Mass comes from the material's density config value.

BuildingInterior nodes carry a configurable `building_interior_weight` per
voxel (representing implied furniture, occupants, etc.) plus the weight
contributed by their face elements.

### 3.4 Solver: Iterative Relaxation

```
for iteration in 0..max_iterations:
    for each non-pinned node (in voxel coordinate order):
        net_force = gravity_force
        for each spring connected to this node:
            displacement = other_node.position - this_node.position
            extension = |displacement| - rest_length
            spring_force = k * extension * normalize(displacement)
            net_force += spring_force
        node.position += net_force * damping_factor
```

After the final iteration, compute stress in each spring:

```
for each spring:
    displacement = node_a.position - node_b.position
    extension = |displacement| - rest_length
    force = k * |extension|
    if force > spring.strength:
        mark as failed
```

The `damping_factor` controls convergence speed. Too high and the system
oscillates; too low and convergence is slow. A value around `0.2 / k_max`
(where `k_max` is the highest stiffness in the system) is a reasonable
starting point. This should be a config parameter for tuning.

**Iteration order:** Always iterate nodes in `VoxelCoord` order (x, then z,
then y — matching the existing flat array layout). This ensures determinism
across all clients.

**Fixed iteration count:** `structural_max_iterations` in GameConfig,
defaulting to something like 100. Can be tuned for speed vs accuracy.

---

## 4. Building Face Elements

Buildings use `BuildingInterior` voxels with per-face `FaceData`. The faces
(Wall, Window, Door, Floor, Ceiling) are structurally meaningful: they
transfer forces between the building interior and its surroundings.

### 4.1 Face-to-Spring Mapping

Each face with a structural type generates a spring connecting the
BuildingInterior node to the node on the other side of that face (which may
be a solid voxel node, another BuildingInterior node, or an air boundary).

| FaceType | Stiffness | Strength | Notes |
|----------|-----------|----------|-------|
| Wall     | High      | High     | Full structural wall |
| Window   | Low       | Low      | Glass is weak; more windows = weaker |
| Door     | Very low  | Very low | Minimal structural contribution |
| Floor    | High      | High     | Distributes vertical load |
| Ceiling  | High      | High     | Top diaphragm, distributes load to walls |
| Open     | 0         | 0        | No spring — no structural connection |

The specific stiffness and strength values are config parameters (see §5).

**Boundary behavior:** If the face points toward an Air voxel that has no
node in the structural network, no spring is created. The face is a free
boundary — it doesn't transfer load to empty air. This is correct: a wall
facing outward into air doesn't transfer force to anything.

**Interior-to-interior faces:** Two adjacent BuildingInterior voxels with
`Open` faces between them have no spring — they're an open interior. If one
of them has a Wall face toward the other (e.g., an interior partition wall),
that generates a spring. This correctly models interior walls as structural
bracing.

### 4.2 Face Weight

Each face also contributes mass to its parent node. A Wall face implies
physical material (wood planks) that has weight. Face weight values per
FaceType are in the config. This mass is added to the BuildingInterior node's
base weight.

### 4.3 Gameplay Implications

This design creates interesting building trade-offs:

- **More windows = weaker.** A wall of windows looks nice but provides little
  structural support. An all-window building on a cantilevered platform is at
  risk.
- **Interior partition walls add bracing.** Multi-room buildings are
  structurally stronger than open-plan ones.
- **Multi-story buildings work** because each Floor face distributes vertical
  load down to the walls below, and the walls transfer it to the foundation.
- **A building without a ceiling** (if that becomes possible) has no top
  diaphragm and is weaker against lateral forces.
- **Doors are structural weak points.** A wide doorway is weaker than a solid
  wall. Players learn to keep structural doors narrow.

---

## 5. Material Properties in GameConfig

All material and face properties are data-driven. New config sections:

```json
{
  "structural": {
    "gravity": 1.0,
    "max_iterations": 100,
    "damping_factor": 0.01,
    "building_interior_base_weight": 0.1,
    "warn_stress_ratio": 0.5,
    "block_stress_ratio": 3.0,
    "tree_gen_max_retries": 4,
    "materials": {
      "Trunk":         { "density": 1.0,  "stiffness": 50.0, "strength": 40.0 },
      "Branch":        { "density": 0.8,  "stiffness": 35.0, "strength": 25.0 },
      "Root":          { "density": 0.8,  "stiffness": 35.0, "strength": 25.0 },
      "GrownPlatform": { "density": 0.6,  "stiffness": 20.0, "strength": 15.0 },
      "GrownWall":     { "density": 0.6,  "stiffness": 20.0, "strength": 15.0 },
      "GrownStairs":   { "density": 0.5,  "stiffness": 15.0, "strength": 12.0 },
      "Bridge":        { "density": 0.5,  "stiffness": 15.0, "strength": 12.0 },
      "ForestFloor":   { "density": 999.0, "stiffness": 999.0, "strength": 999.0 },
      "Leaf":          { "density": 0.05, "stiffness": 0.1,  "strength": 0.1  },
      "Fruit":         { "density": 0.1,  "stiffness": 0.0,  "strength": 0.0  }
    },
    "face_properties": {
      "Wall":    { "weight": 0.3, "stiffness": 15.0, "strength": 10.0 },
      "Window":  { "weight": 0.1, "stiffness": 3.0,  "strength": 2.0  },
      "Door":    { "weight": 0.15, "stiffness": 1.0,  "strength": 1.0  },
      "Floor":   { "weight": 0.4, "stiffness": 18.0, "strength": 12.0 },
      "Ceiling": { "weight": 0.3, "stiffness": 15.0, "strength": 10.0 },
      "Open":    { "weight": 0.0, "stiffness": 0.0,  "strength": 0.0  }
    }
  }
}
```

**Notes on the numbers:**

- All values are unitless and relative. They're tuned for gameplay feel, not
  physical accuracy. The absolute values don't matter — only the ratios
  between materials matter.
- Trunk is ~2.5x stiffer and stronger than player-built platforms. It should
  take extreme loading to threaten a tree voxel.
- ForestFloor has extreme values — it's effectively rigid (pinned nodes
  regardless, but the high values prevent any numerical issues).
- Leaf and Fruit have negligible structural contribution. They're included
  for completeness but effectively don't participate.
- Face properties are roughly 75% the strength of solid GrownWall, reflecting
  that thin wall elements are weaker than solid voxels.
- `warn_stress_ratio`: fraction of material strength at which a blueprint
  triggers a warning (see §7). At 0.5, any spring at >50% of its strength
  limit triggers a caution.
- `block_stress_ratio`: multiple of material strength at which a blueprint
  is hard-blocked. At 3.0, a spring under 3x its strength limit will
  *definitely* fail and is rejected outright.

**Rust representation:** A `StructuralConfig` struct nested inside
`GameConfig`, with a `MaterialProperties` struct per VoxelType and a
`FaceProperties` struct per FaceType. Both use `BTreeMap` keyed by the enum
for config-driven flexibility.

---

## 6. Tree Generation Validation

Generated trees must be structurally sound under their own weight. A tree
with a 30-voxel horizontal branch of radius 1 might be topologically valid
(6-connected) but structurally absurd (bending stress at the connection
far exceeds branch material strength).

### 6.1 Validation procedure

After `generate_tree()` produces the tree geometry:

1. Build the spring-mass network for all tree voxels (Trunk, Branch, Root)
   plus ForestFloor as boundary. Leaf and Fruit voxels are included as mass
   (they weigh something) but contribute negligible stiffness.
2. Run the iterative relaxation solver with gravity load only (no player
   construction, no creature load).
3. Check all springs. If any spring's force exceeds its strength, the tree
   fails validation.
4. If failed, retry generation with the same (incremented) RNG state. This
   burns through the RNG sequence, producing a different tree on the next
   attempt — no need to re-seed.
5. If validation fails after `tree_gen_max_retries` (default: 4) consecutive
   attempts, abort with an error. This indicates the tree profile's
   parameters are fundamentally incompatible with the material properties
   and should be tuned.

### 6.2 Where it runs

In `SimState::with_config()`, after `tree_gen::generate_tree()` returns
but before building the nav graph. The retry loop wraps the tree generation
call:

```rust
let mut tree_result = None;
for attempt in 0..config.structural.tree_gen_max_retries {
    let candidate = tree_gen::generate_tree(&mut world, &config, &mut rng);
    if structural::validate_tree(&world, &config) {
        tree_result = Some(candidate);
        break;
    }
    // Clear the world and retry. The RNG has advanced, so the next
    // attempt produces different geometry.
    world.clear();
    rebuild_forest_floor(&mut world, &config);
}
let tree_result = tree_result.expect(
    "Tree generation failed structural validation after max retries. \
     Tree profile parameters are incompatible with material properties."
);
```

### 6.3 Tuning expectations

With the default material properties (§5), tree profiles should virtually
always pass on the first attempt. The tree's natural tapering (radius
decreases with remaining energy) already produces physically reasonable
geometry — thicker at the base, thinner at the tips. The validation is a
safety net, not a routine filter.

If a tree profile consistently fails validation, the right fix is to adjust
the profile (more energy-to-radius for thicker branches, higher gravitropism
to keep branches shorter, or fewer splits to reduce cantilever length) —
not to weaken the material properties.

### 6.4 Determinism note

The retry loop affects the RNG state. Two clients with the same seed will
produce the same number of retries (since the tree generation is
deterministic), so they'll end up with the same RNG state and the same tree.
Determinism is preserved.

---

## 7. Blueprint Validation (Tiered Enforcement)

When the player designates a blueprint, the sim runs structural analysis on
the hypothetical world state (current world + proposed blueprint voxels) and
returns a tiered result.

### 7.1 Validation tiers

| Tier | Condition | Response |
|------|-----------|----------|
| **OK** | No spring exceeds `warn_stress_ratio * strength` | Blueprint accepted, rendered in green/blue |
| **Warning** | At least one spring exceeds `warn_stress_ratio * strength` but none exceed `block_stress_ratio * strength` | Blueprint accepted with caution overlay. Warning text: "This structure is under significant stress. Consider adding supports." Yellow/orange rendering. |
| **Blocked** | Any spring exceeds `block_stress_ratio * strength`, OR the blueprint is not connected to any grounded structure | Blueprint rejected, rendered in red. Error text: "This structure would collapse under its own weight." or "This structure is not connected to any support." |

The warning tier is the "it works but it's risky" zone. The structure will
stand under its own weight but has limited margin. Future additional load
(more construction above, eventual creature weight tracking) could push it
into failure. The player is informed and can proceed at their own risk.

### 7.2 Per-voxel stress data

The validation returns per-spring (or per-voxel) stress ratios so the UI can
render a stress heatmap:

```rust
pub struct BlueprintValidation {
    /// Overall result.
    pub tier: ValidationTier,
    /// Per-voxel maximum stress ratio (max spring stress / spring strength
    /// across all springs touching this voxel). Values > 1.0 mean failure.
    /// Keyed by VoxelCoord for the proposed blueprint voxels AND their
    /// immediate neighbors (so existing structure near the connection shows
    /// stress too).
    pub stress_map: BTreeMap<VoxelCoord, f32>,
    /// Human-readable warning/error message.
    pub message: String,
}

pub enum ValidationTier {
    Ok,
    Warning,
    Blocked,
}
```

The stress map covers both the new blueprint voxels and the existing voxels
near the connection point. This lets the UI show "the existing trunk voxel
here is now under high stress because of your proposed platform."

### 7.3 Connectivity pre-check

Before running the full spring-mass solver, a quick connectivity check
(BFS/DFS flood fill from grounded voxels) determines whether the blueprint
is connected to the grounded structure at all. If not, it's immediately
blocked — no need to run the expensive solver. This is essentially the
F-struct-basic check, used as a fast pre-filter.

### 7.4 Performance budget

Blueprint validation runs when the player confirms a designation — not every
frame during placement preview. For large blueprints, the solver should
complete in under 50ms to avoid a perceptible hitch. With ~100 iterations
over ~1000 nodes, this is comfortably achievable.

For the **stress heatmap preview** during placement (F-stress-heatmap), the
solver would need to run more frequently (every time the player moves the
blueprint). This can use a reduced iteration count (20–30 instead of 100)
for an approximate but responsive preview. The approximate result is fine
for a color overlay — it just needs to show the general stress pattern.

### 7.5 Bridge method

```rust
/// Validate a proposed blueprint against structural integrity.
///
/// `proposed_voxels` are the voxel coordinates of the blueprint.
/// `proposed_type` is the VoxelType they'd become.
/// Returns validation tier, stress map, and message.
///
/// For buildings, `proposed_faces` contains the face data for interior
/// voxels. For solid construction, this is empty.
fn validate_blueprint(
    proposed_voxels: &[VoxelCoord],
    proposed_type: VoxelType,
    proposed_faces: &BTreeMap<VoxelCoord, FaceData>,
) -> BlueprintValidation
```

The GDScript bridge in `sim_bridge.rs` wraps this, converting between Godot
types and Rust types. The stress map is returned as a `PackedFloat32Array`
or similar for the renderer to consume.

---

## 8. Interaction with F-struct-basic (Connectivity Flood Fill)

F-struct-basic is a simpler, earlier feature: pure connectivity checking via
flood fill. It answers one question: "is every solid voxel reachable from a
grounded voxel via face-adjacent solid voxels?"

This draft **incorporates** the connectivity check as a fast pre-filter in
blueprint validation (§7.3). When F-struct-basic is implemented as a
standalone feature, it should be designed so that the FEM system can reuse
its flood fill implementation.

**F-struct-basic's own scope** (not detailed here):
- Connectivity check after construction/deconstruction events.
- Disconnected clusters are flagged but NOT automatically dropped (that's
  F-cascade-fail, out of scope). For the initial connectivity-only feature,
  a disconnected cluster triggers a warning/event but remains in place.
- This is Phase 3 work; the full FEM system is Phase 5.

---

## 9. Solver Architecture and File Layout

### 9.1 New file: `structural.rs`

A new module in `elven_canopy_sim` containing:

- `StructuralNetwork` — the spring-mass network built from the voxel world.
  Contains nodes (positions, masses, pinned status) and springs (pairs of
  node indices, stiffness, strength, rest length).
- `build_network(world, face_data, config)` — constructs the network from
  the voxel world state.
- `solve(network, config)` — runs iterative relaxation, returns per-spring
  stress values.
- `validate_tree(world, config)` — convenience: build network for tree-only
  voxels + ground, solve, check for failures.
- `validate_blueprint(world, face_data, proposed, config)` — convenience:
  build network with hypothetical additions, solve, return
  `BlueprintValidation`.
- `flood_fill_connected(world, proposed)` — connectivity check (shared with
  F-struct-basic).

### 9.2 Config additions: `config.rs`

New structs:

```rust
pub struct StructuralConfig {
    pub gravity: f32,
    pub max_iterations: u32,
    pub damping_factor: f32,
    pub building_interior_base_weight: f32,
    pub warn_stress_ratio: f32,
    pub block_stress_ratio: f32,
    pub tree_gen_max_retries: u32,
    pub materials: BTreeMap<VoxelType, MaterialProperties>,
    pub face_properties: BTreeMap<FaceType, FaceProperties>,
}

pub struct MaterialProperties {
    pub density: f32,
    pub stiffness: f32,
    pub strength: f32,
}

pub struct FaceProperties {
    pub weight: f32,
    pub stiffness: f32,
    pub strength: f32,
}
```

`StructuralConfig` becomes a field on `GameConfig`:

```rust
pub struct GameConfig {
    // ... existing fields ...
    pub structural: StructuralConfig,
}
```

### 9.3 Sim integration: `sim.rs`

- `with_config()`: tree generation retry loop (§6.2).
- `designate_build()` / `designate_building()`: call
  `structural::validate_blueprint()` and return the result to the caller.
  If blocked, reject the command. If warning, accept but tag the blueprint.
- Blueprint struct gains an optional `stress_warning: bool` flag.

### 9.4 Bridge additions: `sim_bridge.rs`

- `validate_blueprint_structural(voxels, type, ...)` — exposes validation
  to GDScript for preview rendering.
- Return format: flat arrays of coordinates and stress values, plus a tier
  enum as an integer.

### 9.5 GDScript UI: `blueprint_renderer.gd` / `construction_controller.gd`

- Stress heatmap rendering: color-map voxels from green (0% stress) through
  yellow (50%) to red (100%+).
- Warning/error text display near the blueprint.
- Tier-based accept/reject logic in the construction controller.

---

## 10. Determinism

The structural solver must be deterministic for multiplayer and replay
support. Key guarantees:

- **Fixed iteration order.** Nodes are always processed in VoxelCoord order.
  The network construction iterates voxels in flat-array order (x, z, y),
  which is deterministic for the same world state.
- **Fixed iteration count.** No early termination based on convergence
  threshold (which could differ due to FP rounding). Always run exactly
  `max_iterations` iterations.
- **No HashMap.** All collections are `BTreeMap` or `Vec` with deterministic
  ordering.
- **No transcendental functions.** The solver uses only addition,
  subtraction, multiplication, division, and `sqrt` (for vector magnitude).
  `sqrt` is IEEE 754 and deterministic on the same architecture.
- **Cross-architecture note.** `f32` arithmetic is deterministic within a
  single architecture but may differ across architectures (x86 vs ARM) due
  to different FP rounding behaviors. For same-architecture multiplayer,
  this is fine. If cross-architecture multiplayer is ever needed, the solver
  could switch to fixed-point arithmetic — the structural values are
  approximate enough that 16.16 fixed-point would suffice. This is a future
  concern, not an initial requirement.
- **Tree generation retries** advance the RNG deterministically (§6.4).

---

## 11. Out-of-Scope: Runtime Failure and Cascade

This section documents what the draft explicitly **does not** cover, so that
the future F-cascade-fail feature has clear context about what to build on.

When a voxel is destroyed at runtime (fire, deconstruction, combat damage):

1. **Connectivity check.** Re-run flood fill. If any cluster is now
   disconnected from ground, it needs to fall.
2. **Structural re-check.** Re-run the spring-mass solver on the modified
   world. If any spring exceeds strength, that voxel fails, which may
   disconnect further clusters (chain reaction).
3. **Fall physics.** Disconnected clusters become falling rigid bodies.
   Compute fall height from their lowest voxel to the first solid surface
   below. Schedule an impact event.
4. **Impact damage.** Falling debris damages whatever it lands on (other
   structures, creatures, the ground). This may trigger secondary structural
   checks on the impacted structure.
5. **Creature handling.** Creatures on failing/falling structures need to be
   handled — injured, killed, or displaced to the nearest safe nav node.

All of this requires the solver built in this draft, but adds fall physics,
impact mechanics, creature damage, and event scheduling that are separate
concerns. The solver's `validate_blueprint` and `validate_tree` functions
are read-only — they don't modify the world. The cascade system would need
a `process_structural_failure` function that does modify the world.

---

## 12. Structural Integrity During Construction

Blueprint validation checks the **completed** structure only. Intermediate
construction states — where only some voxels have materialized — are exempt
from structural checks. A half-built arch or a partially-extended bridge may
be physically unsound as a cantilever, but construction proceeds anyway.

### 12.1 Rationale

**Thematic justification.** The player is a tree spirit — a magical
consciousness that sings wood into shape. During construction, the spirit
actively holds the growing structure together. The tree IS the falsework.
In the real world, arches require temporary supports (centering) that are
removed after the keystone is placed. In this game, the tree spirit provides
that support inherently.

**Practical justification.** The alternative — requiring every intermediate
construction state to be structurally sound — is extremely hard. Finding a
valid materialization ordering such that the structure remains sound at every
step is a constrained topological sort problem. For arches and bridges, no
such ordering exists without adding a scaffolding/falsework build type with
its own construction and deconstruction lifecycle. The complexity cost is
high and the gameplay value is low.

**Player experience.** If intermediate states were checked, building a
bridge between two branches would trigger "structurally unsound" warnings at
every step (the half-bridge is a cantilever). Players would need to build
temporary supports first — busywork, not fun.

### 12.2 What this means for materialization order

The current `materialize_next_build_voxel()` already requires each voxel to
be face-adjacent to existing solid (so construction grows outward, no
floating blobs). No structural check is added to this step. Within the
adjacency constraint, voxel order can follow the existing heuristic (prefer
unoccupied positions to avoid displacing elves).

A nice-to-have aesthetic improvement: prefer voxels closer to the grounded
support, so construction visually grows outward from the trunk. For bridges
where both ends are adjacent to solid, alternate ends so it grows from both
sides toward the middle. This is a visual polish, not a structural
requirement.

### 12.3 Exploit potential: incomplete construction

Because intermediate states are exempt, a player could theoretically exploit
this by intentionally leaving construction incomplete — building the first
few voxels of an absurdly long cantilever that was "approved" because the
full blueprint (with supports at the far end) is structurally sound, then
cancelling the blueprint and keeping the unsound partial structure.

This is a known edge case deferred to F-partial-struct (see tracker). Possible
future mitigations:
- Run structural checks on cancellation and mark unsound remnants.
- Periodic structural heartbeat that catches structures whose blueprints
  were cancelled mid-construction.
- Limit how far construction can extend from its nearest grounded support
  before the next support must be in place.

None of these are needed for the initial implementation. The exploit requires
deliberate effort, produces fragile structures that would fail under any
future runtime structural check, and isn't a competitive advantage in a
single-player game.

---

## 13. Test Plan

The solver produces approximate physical results, so tests need to verify
that the *qualitative* behavior is correct: longer cantilevers are more
stressed, supports reduce stress, disconnected things are caught, etc. The
exact stress values will shift as parameters are tuned — tests should check
relative relationships and threshold crossings, not specific numbers.

### 13.1 Graduated cantilever (core physics validation)

The single most important test family. Build a horizontal platform
cantilevered off a vertical column, and verify that stress increases with
length.

**Setup:** A column of N solid voxels on ForestFloor (grounded), with a
horizontal arm extending from the top.

```
    ████████  ← arm (variable length)
    █
    █  ← column
    █
  ▓▓▓▓▓▓▓▓▓  ← forest floor (pinned)
```

**Test cases:**

- **Short arm (3 voxels).** Structurally sound. All springs well below
  warn threshold. Validates that basic structures pass.
- **Medium arm (8–10 voxels).** Still structurally sound but stress at the
  column-arm junction is noticeably higher than the short arm. Assert
  `stress(medium) > stress(short)`. Validates that the solver produces
  physically reasonable stress gradients.
- **Long arm (20+ voxels).** Exceeds strength threshold. The junction
  spring fails. Assert that `validate_blueprint` returns Blocked. Validates
  that the solver catches structurally unsound geometry.
- **Verify monotonicity.** For arms of length 3, 5, 8, 12, 16, 20, verify
  that peak stress is monotonically increasing with length. This catches
  solver bugs where stress behaves non-physically.

### 13.2 Support struts reduce stress

Adding a diagonal brace under a cantilever should reduce stress at the
junction.

```
    ████████          ████████
    █            vs   █
    █                 █ █
    █                 █   █  ← diagonal support
  ▓▓▓▓▓▓▓▓▓        ▓▓▓▓▓▓▓▓▓
```

- Build a medium-length cantilever that produces moderate stress.
- Add a diagonal line of solid voxels from the column base to the arm tip.
- Assert that peak stress at the junction is lower with the support than
  without.

### 13.3 Thickness matters

A 3-voxel-thick arm should be stronger than a 1-voxel-thick arm of the
same length.

- Build two cantilevers of the same length: one 1 voxel thick (1×1 cross
  section), one 3 voxels thick (3×1 or 3×3).
- Assert that peak stress is lower for the thicker arm.

### 13.4 Disconnected structure (connectivity check)

- Place a cluster of solid voxels in the air, not face-adjacent to any
  grounded structure.
- Assert that `validate_blueprint` returns Blocked with a connectivity
  failure message.
- Place a cluster that IS connected via one voxel to the ground.
- Assert it passes connectivity (though it may warn/fail on stress).

### 13.5 Building face structural contribution

Verify that building walls provide structural value.

- Build a platform cantilevered at medium length (in the "stressed but OK"
  range).
- Place a building on the platform with all-Wall faces.
- Assert that the building adds load (stress at junction increases relative
  to bare platform) but the walls provide some bracing benefit.
- Replace all Wall faces with Window faces.
- Assert that peak stress is higher with windows than with walls (windows
  are weaker).
- Replace all faces with Open.
- Assert this is the worst case — no structural contribution from building
  at all, just dead weight.

### 13.6 Building weight vs reinforcement

A building should add weight (increasing stress on its foundation) but
its walls should also resist deformation. Test the net effect:

- Cantilevered platform alone: measure junction stress.
- Same platform with a building on it: junction stress should be *higher*
  (the building is heavy) but the building's walls partially offset this.
- Same platform with just the building's weight added as dead load (no face
  springs): junction stress should be higher than with the building walls
  active, proving the walls help.

### 13.7 Tiered validation thresholds

- Build a short cantilever: assert `ValidationTier::Ok`.
- Extend it to moderate length: assert `ValidationTier::Warning` (stress
  above `warn_stress_ratio` but below `block_stress_ratio`).
- Extend it to extreme length: assert `ValidationTier::Blocked`.
- This directly tests the threshold logic and config parameters.

### 13.8 Tree generation validation

- Generate a tree with the default `fantasy_mega` profile and default
  material properties. Assert it passes structural validation on the
  first attempt (no retries needed).
- Create a config with absurdly weak material properties (Branch strength
  near zero). Attempt tree generation. Assert that it retries and
  eventually panics after `tree_gen_max_retries` attempts.
- Create a config with moderately weak properties. Verify that the system
  retries and eventually produces a valid tree (the RNG generates a
  different, sturdier tree on a subsequent attempt).

### 13.9 Determinism

- Run the solver twice on the same world state. Assert that per-spring
  stress values are bit-for-bit identical.
- Generate a tree with the same seed twice (including any retries). Assert
  identical tree geometry.
- Run blueprint validation twice on the same world + blueprint. Assert
  identical `BlueprintValidation` results.

### 13.10 Stress map spatial correctness

- Build a simple cantilever. Verify that the stress map has the highest
  values at the junction (where the arm meets the column) and lower values
  at the arm tip and column base.
- This validates that the solver isn't just producing stress values, but
  producing them at the *right locations*.

### 13.11 Per-phase test expectations

Each implementation phase (§14 below) should have passing tests before
moving to the next:

| Phase | Required passing tests |
|-------|----------------------|
| A (config) | Config serialization roundtrip with structural fields |
| B (solver) | §13.1 graduated cantilever, §13.2 supports, §13.3 thickness, §13.9 determinism, §13.10 spatial correctness |
| C (tree gen) | §13.8 tree generation validation |
| D (blueprint) | §13.4 disconnected, §13.7 tiered validation |
| E (building faces) | §13.5 face contribution, §13.6 weight vs reinforcement |
| F (UI) | Manual testing (GDScript rendering, no sim unit tests) |

---

## 14. Implementation Phases

### Phase A: Material properties and config

- Add `StructuralConfig`, `MaterialProperties`, `FaceProperties` to
  `config.rs`.
- Add `structural` field to `GameConfig` with `Default` implementation.
- Update `default_config.json`.
- Pure data work, no logic. Quick to implement and review.

### Phase B: Spring-mass solver (solid voxels only)

- Create `structural.rs` with network construction (solid voxels only,
  no building faces yet).
- Implement iterative relaxation solver.
- Implement `validate_tree()`.
- Write tests: simple structures with known stress patterns (single
  cantilever, supported beam, column under compression).
- This is the core algorithmic work and the bulk of the implementation.

### Phase C: Tree generation validation

- Add retry loop in `SimState::with_config()`.
- Test with intentionally weak material properties to force retries.
- Verify determinism: same seed produces same tree after retries.

### Phase D: Blueprint validation

- Add `flood_fill_connected()` for connectivity pre-check.
- Add `validate_blueprint()` combining connectivity + solver.
- Integrate into `designate_build()` / `designate_building()`.
- Add bridge method for GDScript.
- Write tests: valid blueprint passes, cantilevered-too-far fails,
  disconnected fails.

### Phase E: Building face integration

- Extend `build_network()` to include BuildingInterior nodes and face
  springs.
- Add face property config values.
- Test: building on cantilever adds load, building with walls vs windows
  shows different stress patterns.

### Phase F: Stress heatmap UI

- GDScript: stress overlay rendering in blueprint preview mode.
- Warning/error text display.
- This is purely UI work, no sim changes.

---

## 15. Open Questions

- **Solver parameter tuning.** The damping factor, iteration count, and
  material property values all need gameplay tuning. Initial values in this
  draft are educated guesses. Plan for an iteration cycle of: implement →
  test with various tree profiles → adjust values → repeat.

- **Leaf/Fruit structural role.** Currently modeled as having mass but
  negligible stiffness (they weigh down branches but don't hold anything
  up). This seems right for gameplay, but it means a branch covered in
  leaves is slightly more stressed than a bare branch. Is that desirable?
  Probably yes — it creates a reason to prune/harvest.

- **Support strut build type.** The design doc mentions diagonal braces as
  a way to reinforce platforms. There's no `Strut` build type yet. When one
  is added, it would just be another solid voxel type with its own material
  properties — the solver handles it automatically.

- **Approximate preview vs exact validation.** The solver runs with reduced
  iterations for real-time preview (§7.4). How different can the approximate
  result be from the exact result? If a structure is borderline (near the
  warn threshold), the preview might show green while the final validation
  shows yellow. Acceptable? Probably — the exact validation happens when the
  player confirms, and they get the accurate tier then.

- **Performance with large trees.** A fantasy mega tree can have thousands of
  voxels. Building the full spring-mass network for the entire tree + all
  construction is expensive. An optimization: only include voxels within some
  radius of the proposed blueprint, treating distant voxels as fixed boundary
  conditions. The stress effects of a small platform don't propagate across
  the entire tree. This is a performance optimization to add if needed, not
  an initial requirement.
