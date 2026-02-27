# Construction Feature — Iterative Roadmap

Summary of the planned iterative approach discussed before diving into Step 1.

## Agreed Decisions

- **Adjacency requirement**: Platforms must have at least one voxel face-adjacent to existing solid voxels (trunk, branch, etc.). No floating platforms.
- **Voxel list from UI**: The UI computes the full `Vec<VoxelCoord>` and sends it in the `DesignateBuild` command. The sim doesn't interpret shapes — it just validates and stores the list. This is more general and works for any shape later.
- **Incremental nav graph update**: Deferred to its own step after blueprints and construction work end-to-end. Full rebuild is fine for now (milliseconds on a 256^3 world).
- **No shortcuts**: Work iteratively through each step, verifying behavior at each stage before moving on.

## Step 1: Blueprint Data Model (sim only) ✓ DONE

Add a `Blueprint` struct to the sim. Wire `DesignateBuild` command to validate and store blueprints. Wire `CancelBuild` to remove them. Blueprints are just recorded intent — no voxels are placed, no nav graph changes. Full test suite.

Done on branch `feature/blueprint-data-model`. Files: `blueprint.rs` (new), `sim.rs`, `world.rs`, `event.rs`, `command.rs`, `lib.rs`. 14 new tests, 136 total passing.

## Step 2: Construction Mode Toggle (GDScript + gdext)

Enter/exit a construction mode that sets up the UI and camera for building.

**UI:**
- Construction button (top of screen) toggles construction mode on/off
- When active, a panel appears on the right side of the screen (empty for now — future steps add build options to it)
- ESC exits construction mode (fits existing ESC precedence chain)

**Camera behavior in construction mode:**
- The orbital camera's focus point (orbit target) smoothly slides/pulls toward the nearest voxel center when the player releases movement keys
- Snaps in all 3 dimensions (x, y, z)
- Smooth interpolation so it doesn't feel jerky
- Normal camera movement still works (orbit, zoom, pan) — the snap only activates after input stops

**Files likely touched:**
- `orbital_camera.gd` — add snap-to-voxel-center behavior, toggled by construction mode
- New `construction_controller.gd` — manages construction mode state, UI panel
- `main.gd` — wire the new controller
- `spawn_toolbar.gd` or new toolbar — add the construction mode button

**Validation bridge (prep for Step 3):**
- `sim_bridge.rs` — expose `validate_blueprint_voxels(voxels) -> bool` to GDScript
- Calls `world.in_bounds()`, `world.get()`, `world.has_solid_face_neighbor()`

## Step 3: 1x1 Platform Blueprint Placement (GDScript + gdext)

Place a single-voxel platform blueprint at the camera focus position.

**Ghost mesh:**
- Translucent 1x1x1 cube rendered at the snapped camera focus position
- **Blue** if the position is valid (Air + adjacent to solid)
- **Red** if invalid
- Validation calls through the sim bridge to reuse Rust logic

**Confirm/cancel:**
- Click or hotkey to confirm → sends `DesignateBuild` command with the single voxel
- ESC or right-click to cancel / exit construction mode

**Designated blueprint rendering:**
- Once confirmed, the blueprint appears in-game as a **light-blue ghost mesh**
- New renderer (like `tree_renderer.gd`) that reads `SimState.blueprints` and draws translucent cubes for each `Designated` blueprint's voxels
- `sim_bridge.rs` needs to expose blueprint data to GDScript

**Files likely touched:**
- `construction_controller.gd` — placement logic, ghost mesh preview
- New `blueprint_renderer.gd` — renders designated blueprints as light-blue ghost meshes
- `sim_bridge.rs` — `designate_build()` + `get_blueprints()` methods
- `main.gd` / `main.tscn` — wire blueprint renderer

## Step 4: Adjustable Platform Size (GDScript)

Expand the 1x1 platform to an adjustable rectangle via click-and-drag.

**Interaction:**
- Click to set one corner, drag to set the opposite corner of the rectangle
- All voxels are on the same Y level (horizontal platform)
- Ghost mesh updates in real-time as the player drags
- Color reflects validity of the entire rectangle (all voxels must be valid)
- Release to confirm the shape, then click "Construct" or press Enter

**Files likely touched:**
- `construction_controller.gd` — drag-to-select rectangle logic
- Ghost mesh generation for multi-voxel preview

## Step 5: Platform Placement (sim only)

When a blueprint is designated, immediately place `GrownPlatform` voxels in the world. Rebuild the nav graph. Mark the blueprint as `Complete`. This is a temporary "cheat mode" — it skips mana cost and construction time so we can verify that:
- Voxels appear in the world correctly
- Nav graph updates and creatures can walk on platforms
- Save/load works with placed platforms (rebuild_world needs to restore them)

## Step 6: Build Task + Single Worker

Replace instant placement with a `Build` task kind in the task system:
- `DesignateBuild` creates the blueprint AND a `Build` task at the blueprint location
- An idle elf claims the task, pathfinds to the site
- Verify the elf walks to the correct location
- On arrival, the elf does work (progress increments per activation tick)
- Mana is deducted from the tree per work unit
- When progress reaches total_cost, voxels are placed and nav graph rebuilds
- Blueprint transitions to `Complete`

## Step 7: Incremental Nav Graph Update

Replace the full `build_nav_graph()` call with an incremental update:
- When voxels change, invalidate nav nodes within Manhattan distance 1
- Re-derive affected nodes and edges
- Re-check connectivity
- This is deterministic because voxel changes go through SimCommand

## Future Steps (not yet detailed)

- Platform rendering (tree_renderer or new renderer for GrownPlatform voxels)
- Multiple workers on one build project
- Priority system for build queue ordering
- Mana economy: elves generate mana, tree stores it, construction spends it
- Structural adjacency warnings in the UI
- Circular/oval platform designation (design doc preference)
- Other build types: bridges, stairs, walls, enclosures
