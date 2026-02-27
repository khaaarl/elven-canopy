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

## Step 2: Construction Mode Toggle (GDScript + gdext) ✓ DONE

Enter/exit a construction mode that sets up the UI and camera for building.

Done on branch `feature/construction-mode`. Construction button toggles mode on/off, ESC exits, camera snaps to voxel centers in construction mode, validation bridge exposes `validate_blueprint_voxels()` from Rust to GDScript.

## Step 3: 1x1 Platform Blueprint Placement (GDScript + gdext) ✓ DONE

Place a single-voxel platform blueprint at the camera focus position. Ghost mesh preview (blue=valid, red=invalid), confirm/cancel, designated blueprints rendered as light-blue ghost meshes.

Done as part of the construction mode work. Files: `construction_controller.gd`, `blueprint_renderer.gd`, `sim_bridge.rs`.

## Step 4: Adjustable Platform Size (GDScript) ✓ DONE

Click-and-drag to designate rectangular platforms of arbitrary size. All voxels on the same Y level, ghost mesh updates in real-time, color reflects validity of the entire rectangle.

Done — large platform blueprints can be created and are displayed in the UI.

## Step 5: Blueprint → Build Job (sim)

**This is the next step.** When a blueprint is designated, create a `Build` task in the task system so an elf can be assigned to construct it.

**Sub-steps:**

### 5a: Instant placement (cheat mode)

When a blueprint is designated, immediately place `GrownPlatform` voxels in the world. Rebuild the nav graph. Mark the blueprint as `Complete`. This temporary shortcut lets us verify that:
- Voxels appear in the world correctly
- Nav graph updates and creatures can walk on platforms
- Save/load works with placed platforms

### 5b: Build task creation

Replace instant placement with a `Build` task kind in the task system:
- `DesignateBuild` creates the blueprint AND a `Build` task at the blueprint location
- The task has a target position (adjacent walkable voxel near the blueprint)

### 5c: Elf assignment + pathfinding

- An idle elf claims the `Build` task
- Elf pathfinds to the build site (nearest walkable voxel adjacent to the blueprint)
- Verify the elf walks to the correct location

### 5d: Construction work

- On arrival, the elf does work (progress increments per activation tick)
- When progress reaches total_cost, voxels are placed and nav graph rebuilds
- Blueprint transitions to `Complete`

### 5e: Mana cost (deferred)

- Mana is deducted from the tree per work unit
- Insufficient mana pauses construction
- (Can be deferred until the mana economy exists)

## Step 6: Incremental Nav Graph Update

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
