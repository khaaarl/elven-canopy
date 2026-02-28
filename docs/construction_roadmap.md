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

When a blueprint is designated, create a `Build` task in the task system so an elf can be assigned to construct it.

**Sub-steps:**

### 5a: Instant placement (cheat mode)

Skipped — went directly to incremental construction.

### 5b: Build task creation ✓ DONE

`DesignateBuild` creates the blueprint AND a `Build { project_id }` task at the nearest nav node. The task's `total_cost = build_work_ticks_per_voxel * num_voxels`. Blueprint stores `task_id: Option<TaskId>` for linkage.

Done on branch `feature/construction-build-tasks`. New `TaskKind::Build` variant in `task.rs`, `build_work_ticks_per_voxel` config field, `placed_voxels` on `SimState` for persistence.

### 5c: Elf assignment + pathfinding ✓ DONE

An idle elf claims the Build task (elf-only via `required_species`), pathfinds to the build site using the existing A* task system. No new code needed — the existing `execute_task_behavior` walk-toward-location logic handles it.

### 5d: Construction work ✓ DONE

`do_build_work()` increments progress by 1.0 per activation. Every `build_work_ticks_per_voxel` units, `materialize_next_build_voxel()` picks an adjacency-valid voxel (preferring unoccupied ones), places it as solid, updates the nav graph, and resnaps displaced creatures. When all voxels are placed, the blueprint transitions to `Complete` and the elf is freed. `cancel_build()` reverts materialized voxels, removes the task, and unassigns workers.

Done on branch `feature/construction-build-tasks`. 13 new tests (155 total).

### 5e: Mana cost (deferred)

- Mana is deducted from the tree per work unit
- Insufficient mana pauses construction
- (Can be deferred until the mana economy exists)

## Step 6: Incremental Nav Graph Update ✓ DONE

Replace the full `build_nav_graph()` call in `materialize_next_build_voxel()` with an incremental update that touches only ~7 affected positions instead of scanning the entire 8.4M-voxel world.

Done on branch `feature/incremental-nav-update`. Changes:
- `NavGraph.nodes` refactored to `Vec<Option<NavNode>>` for stable IDs across incremental updates.
- Persistent `spatial_index` (flat voxel index → node slot) for O(1) coord→node lookup.
- `update_after_voxel_solidified(world, coord)` adds/removes/updates nodes at the changed coord + 6 face neighbors, then recomputes edges for the dirty set + their 26-neighbors.
- `resnap_removed_nodes()` only resnaps creatures whose specific node was removed.
- `pathfinding.rs` uses `node_slot_count()` for A* array sizing (accounts for dead slots).
- 3 new tests (158 total).

## Future Steps (not yet detailed)

- Platform rendering (tree_renderer or new renderer for GrownPlatform voxels)
- Multiple workers on one build project
- Priority system for build queue ordering
- Mana economy: elves generate mana, tree stores it, construction spends it
- Structural adjacency warnings in the UI
- Circular/oval platform designation (design doc preference)
- Other build types: bridges, stairs, walls, enclosures
