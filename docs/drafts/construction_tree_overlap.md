# Construction Tree Overlap — Design Plan

Structural build types like platforms grow *out of* the tree. When the player
designates a platform that intersects the tree's geometry, the system should:

1. Allow the overlap instead of rejecting it.
2. Skip blueprint voxels for wood that's already there (Trunk/Branch/Root).
3. Convert non-wood tree geometry (Leaf/Fruit) to wood via normal construction.
4. Render ghost voxels inside existing solid material as wireframe edges, not
   solid translucent cubes.
5. Reject the placement only if *zero* voxels would actually be constructed
   (the entire footprint is already wood).

This behavior is specific to "structural" build types. Most future build types
(furniture, decorations, etc.) will keep the current all-must-be-Air validation.

---

## 1. BuildType: `allows_tree_overlap()`

Add a method on `BuildType` (in `types.rs` or `blueprint.rs`):

```rust
impl BuildType {
    pub fn allows_tree_overlap(&self) -> bool {
        matches!(self, BuildType::Platform | BuildType::Bridge | BuildType::Stairs)
    }
}
```

Wall and Enclosure stay `false` for now. Easy to flip later.

---

## 2. Voxel Classification

When processing a `DesignateBuild` command for a type where
`allows_tree_overlap()` is true, the sim classifies each voxel in the
submitted rectangle into one of three buckets:

| Voxel content          | Classification   | Action                                  |
|------------------------|------------------|-----------------------------------------|
| Air                    | **Exterior**     | Normal blueprint voxel — will be built  |
| Leaf, Fruit            | **Convertible**  | Blueprint voxel — will be replaced with wood during construction |
| Trunk, Branch, Root    | **Already wood** | Skipped — no blueprint voxel needed     |
| ForestFloor, GrownPlatform, GrownStairs, GrownWall, Bridge | **Blocked** | Treated like the existing non-overlap logic — blocks placement |

Validation rules for overlap-enabled types:

- **No blocked voxels** in the footprint (ForestFloor, existing construction).
- **At least one Exterior or Convertible voxel** — if 100% of voxels are
  already wood, the placement is invalid (nothing to build).
- The existing adjacency-to-solid check is inherently satisfied (the tree *is*
  the adjacent solid), but we can keep it as a sanity check.

For non-overlap build types (`allows_tree_overlap() == false`), the existing
validation is unchanged: all voxels must be Air, at least one adjacent to solid.

---

## 3. Sim-Side Changes

### 3a. `designate_build()` in `sim.rs`

Branch on `build_type.allows_tree_overlap()`:

- **false (existing path):** Current logic unchanged. All voxels must be Air,
  at least one adjacent to solid.
- **true (new path):** Classify voxels per the table above. Collect only
  Exterior + Convertible voxels into the blueprint. Reject if none qualify or
  if any are Blocked.

The blueprint's `voxels` field stores only the voxels that need construction
(Exterior and Convertible), not the full submitted rectangle. This keeps the
build task cost proportional to actual work.

### 3b. `do_build_work()` / `materialize_next_build_voxel()` in `sim.rs`

Convertible voxels (Leaf/Fruit) are in the blueprint's voxel list. When
materializing one:

- The voxel may no longer be Air (it's Leaf or Fruit). The current code checks
  `world.get(coord) == VoxelType::Air` — this check needs to become "is this
  voxel still un-materialized by *us*?" instead. Simplest approach: check
  against the blueprint's already-materialized set rather than checking the
  world's voxel type.
- Overwrite Leaf/Fruit with the target type (GrownPlatform). The existing
  voxel-type priority system (Trunk > Branch > Root > Leaf > Air) would
  normally prevent this, but construction is an intentional override — use
  `world.set()` directly rather than the priority-aware setter.
- Nav graph update: `update_after_voxel_solidified()` should still work
  correctly since the voxel was already solid (Leaf/Fruit), so its nav
  neighbors won't change topology. But worth a test.

### 3c. New bridge method: `validate_build_rect_overlap()`

The GDScript UI needs to query validity *before* the player confirms. Add a
bridge method that returns per-voxel classification data so the UI can render
ghosts appropriately:

```rust
// Returns a flat array: [x, y, z, classification, x, y, z, classification, ...]
// classification: 0 = exterior, 1 = convertible, 2 = already_wood, 3 = blocked
fn validate_build_rect_with_overlap(
    x: i32, y: i32, z: i32,
    width: i32, depth: i32,
    build_type: BuildType,
) -> PackedInt32Array
```

Or simpler: two separate arrays — one for "will be built" voxels (exterior +
convertible) and one for "already wood" voxels (wireframe display). Plus a
boolean for overall validity.

The exact API shape can be decided during implementation. The key point is that
the sim classifies and the UI just renders what the sim tells it.

---

## 4. Rendering Changes (GDScript)

### 4a. Wireframe Ghost Material

`blueprint_renderer.gd` currently has one ghost material (translucent cube
faces). Add a second material for wireframe rendering:

- 12 edges of a unit cube, rendered as thin lines or narrow quads.
- Same blue/red color scheme as the solid ghost (valid = blue, invalid = red).
- Wireframe ghosts are used for "already wood" voxels — the player can see
  the platform footprint extends into the tree, but those voxels are just
  outlines since nothing will be built there.

Implementation options for wireframe:
- **Line-based:** Use `ImmediateMesh` or `MeshInstance3D` with line primitives.
  Cleanest visual but doesn't integrate with MultiMesh (each wireframe cube is
  a separate draw call, or batch into one ImmediateMesh).
- **Thin-quad edges:** Model a wireframe cube as 12 thin box meshes (like a
  picture frame). Can use MultiMesh for instancing. Slightly hacky but
  GPU-friendly.
- **Shader-based:** Use the same BoxMesh but with a shader that discards
  fragments except near edges. Single MultiMesh, looks clean. Probably the
  best option.

Recommendation: **shader-based**. Write a simple fragment shader that computes
distance-to-nearest-edge in UV space and discards fragments beyond a threshold.
Apply it to a second MultiMeshInstance3D in the blueprint renderer.

### 4b. Ghost Layer Split

`blueprint_renderer.gd` currently has one ghost MultiMesh. Split into two:

1. **Solid ghost layer** — Exterior + Convertible voxels (will actually be
   built). Current translucent material, unchanged.
2. **Wireframe ghost layer** — Already-wood voxels. Wireframe material.

Both layers use the same blue/red color logic (valid placement = blue,
invalid = red). The distinction is purely visual: "this will be built" vs
"this is already there."

### 4c. Data Flow

Each frame during PLACING mode:

```
construction_controller.gd
    → calls bridge.validate_build_rect_with_overlap(...)
    → receives: build_voxels[], wood_voxels[], is_valid
    → passes to blueprint_renderer:
        build_voxels → solid ghost MultiMesh
        wood_voxels → wireframe ghost MultiMesh
        is_valid → color selection (blue vs red)
```

The `construction_controller.gd` already does per-frame validation in PLACING
mode; this just enriches the data it receives.

---

## 5. Rough File Change Summary

| File | Changes |
|------|---------|
| `elven_canopy_sim/src/types.rs` or `blueprint.rs` | `BuildType::allows_tree_overlap()` method |
| `elven_canopy_sim/src/sim.rs` | Branched validation in `designate_build()`, relaxed Air check in `materialize_next_build_voxel()` |
| `elven_canopy_gdext/src/sim_bridge.rs` | New `validate_build_rect_with_overlap()` bridge method |
| `godot/scripts/blueprint_renderer.gd` | Second MultiMesh + wireframe material/shader |
| `godot/scripts/construction_controller.gd` | Use new bridge method, pass split voxel lists to renderer |

---

## 6. Open Questions / Future Considerations

- **Leaf/Fruit destruction during construction:** When a Leaf/Fruit voxel is
  converted to GrownPlatform, should there be a visual/audio effect? A "leaves
  falling" particle? Not needed for the initial implementation but worth noting.

- **Tree regrowth:** If the tree grows new branches/leaves that collide with an
  in-progress blueprint, what happens? The current system doesn't have dynamic
  tree growth yet, so this is a future concern.

- **Undo/cancel with converted voxels:** If a platform that converted Leaf→
  GrownPlatform is cancelled, should those voxels revert to Leaf or to Air?
  Reverting to Leaf would require storing the original voxel type per blueprint
  voxel. Air is simpler but means cancelling construction permanently removes
  foliage. Worth deciding during implementation.

- **Bridge/Stairs overlap:** The flag includes Bridge and Stairs as overlap-
  enabled. Their construction shapes will be more complex than rectangles, but
  the same voxel classification logic applies. No special handling needed now.
