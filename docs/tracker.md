# Elven Canopy — Project Tracker

Central tracker for all features and bugs. This is the single source of truth
for what needs doing, what's in progress, and what's done. The design doc
(`docs/design_doc.md`) describes *what* each feature is and *why*; this file
tracks *status*, *priority*, and *blocking relationships*.

## Conventions

**IDs:** `F-kebab-name` for features, `B-kebab-name` for bugs. Max 20 chars
total. IDs are stable — never reused or renumbered. Pick something short and
descriptive. Check existing IDs before adding to avoid duplicates.

**Status markers:** `[ ]` todo · `[~]` in progress · `[x]` done

**Cross-references:** Design doc sections as `§N`. Other tracker items by ID.
Draft docs by repo-relative path. Blocking relationships in the detailed entry
use `Blocked by:` (things that must finish first) and `Blocks:` (things waiting
on this).

## Summary

Condensed single-line-per-item view. Grouped by status: in progress first, then
todo, then done. Every item here MUST have a corresponding entry in [Detailed
Items](#detailed-items) and vice versa.

**Format:** Each line is `[status] ID` padded to 23 characters, then a short
title. Example: `[ ] F-example-name       Short title here`. When an item
changes status, update the marker AND move the line to the correct section.

**Ordering:** Items are sorted alphabetically by ID within each section (In
Progress, Todo, Done) and within each topic group in the detailed section.
This reduces merge conflicts when parallel work streams add items.

### In Progress

```
[~] F-elf-needs            Hunger and rest self-direction
[~] F-multiplayer          Relay-coordinator multiplayer networking
```

### Todo

```
[ ] B-preview-blueprints   Preview treats blueprints as complete
[ ] F-adventure-mode       Control individual elf (RPG-like)
[ ] F-ai-sprites           AI-generated sprite art pipeline
[ ] F-apprentice           Skill transfer via proximity
[ ] F-audio-sampled        Sampled vocal syllables from conlang
[ ] F-audio-synth          Waveform synthesis for audio rendering
[ ] F-audio-vocal          Continuous vocal synthesis
[ ] F-batch-blueprint      Batch blueprinting with dependency order
[ ] F-blueprint-mode       Layer-based blueprint selection UI
[ ] F-branch-growth        Grow branches for photosynthesis/fruit
[ ] F-bridges              Bridge construction between tree parts
[ ] F-build-queue-ui       Construction queue/progress UI
[ ] F-carve-holes          Remove material (doors, storage hollows)
[ ] F-cascade-fail         Cascading structural failure
[ ] F-choir-build          Choir-based construction singing
[ ] F-choir-harmony        Ensemble harmony in construction singing
[ ] F-combat               Combat and invader threat system
[ ] F-crafting             Non-construction jobs and crafting
[ ] F-creature-death       Basic creature death (starvation)
[ ] F-creature-tooltip     Creature hover tooltip
[ ] F-cultural-drift       Inter-tree cultural divergence
[ ] F-day-night            Day/night cycle and pacing
[ ] F-defense-struct       Defensive structures (ballista, wards)
[ ] F-elf-names            Elf name generation from conlang rules
[ ] F-elf-weapons          Bows, spears, clubs for elf combat
[ ] F-emotions             Multi-dimensional emotional state
[ ] F-fire-advanced        Heat accumulation and ignition thresholds
[ ] F-fire-basic           Fire spread and voxel destruction
[ ] F-fire-ecology         Fire as ecological force, firefighting
[ ] F-fire-structure       Fire x structural integrity cascades
[ ] F-flying-nav           3D flight navigation system
[ ] F-fog-of-war           Visibility via tree and root network
[ ] F-fruit-prod           Basic fruit production and harvesting
[ ] F-fruit-variety        Food storage, cooking, magical brewing
[ ] F-furnishing           Building geometry + purpose furnishing
[ ] F-hedonic-adapt        Asymmetric hedonic adaptation
[ ] F-lang-crate           Shared Vaelith language crate
[ ] F-lod-sprites          LOD sprites (chibi / detailed)
[ ] F-logistics            Spatial resource flow (Kanban-style)
[ ] F-magic-items          Magic item personalities and crafting
[ ] F-mana-mood            Mana generation tied to elf mood
[ ] F-mana-system          Mana generation, storage, and spending
[ ] F-mass-conserve        Wood mass tracking and conservation
[ ] F-military-campaign    Send elves on world expeditions
[ ] F-military-org         Squad management and organization
[ ] F-modding              Scripting layer for modding support
[ ] F-mood-system          Mood with escalating consequences
[ ] F-mp-chat              Multiplayer in-game chat
[ ] F-mp-checksums         Multiplayer state checksums for desync detection
[ ] F-mp-mid-join          Mid-game join with state snapshot
[ ] F-mp-reconnect         Multiplayer reconnection after disconnect
[ ] F-multi-tree           NPC trees with personalities
[ ] F-music-runtime        Integrate music generator into game
[ ] F-music-use-lang       Migrate music crate to shared lang crate
[ ] F-narrative-log        Events and narrative log
[ ] F-partial-struct       Structural checks on incomplete builds
[ ] F-personality          Personality axes affecting behavior
[ ] F-poetry-reading       Social gatherings and poetry readings
[ ] F-population           Natural population growth/immigration
[ ] F-proc-poetry          Procedural poetry via simulated annealing
[ ] F-root-network         Root network expansion and diplomacy
[ ] F-rope-retract         Retractable rope ladders (furl/unfurl)
[ ] F-seasons              Seasonal visual and gameplay effects
[ ] F-select-struct        Selectable structures with interaction UI
[ ] F-sim-speed            Simulation speed controls UI
[ ] F-social-graph         Relationships and social contagion
[ ] F-sound-effects        Basic ambient and action sound effects
[ ] F-soul-mech            Death, soul passage, resurrection
[ ] F-stairs               Stairs and ramps for vertical movement
[ ] F-stress-heatmap       Stress visualization in blueprint mode
[ ] F-struct-basic         Basic structural integrity (flood fill)
[ ] F-task-priority        Priority queue and auto-assignment
[ ] F-tree-capacity        Per-tree carrying capacity limits
[ ] F-tree-memory          Ancient tree knowledge/vision system
[ ] F-tree-species         Multiple tree species with properties
[ ] F-undo-designate       Undo last construction designation
[ ] F-vaelith-expand       Expand Vaelith language for runtime use
[ ] F-visual-smooth        Smooth voxel surface rendering
[ ] F-weather              Weather within seasons
[ ] F-wireframe-ghost      Wireframe ghost for overlap preview
[ ] F-world-boundary       World boundary visualization
[ ] F-zlevel-vis           Z-level visibility (cutaway/toggle)
```

### Done

```
[x] B-dead-node-panic      Panic on dead nav node in pathfinding
[x] B-dirt-not-pinned      Dirt unpinned in fast structural validator
[x] F-building             Building construction (paper-thin walls)
[x] F-cam-follow           Camera follow mode for creatures
[x] F-capybara             Capybara species
[x] F-construction         Platform construction (designate/build/cancel)
[x] F-core-types           VoxelCoord, IDs, SimCommand, GameConfig
[x] F-crate-structure      Two-crate sim/gdext structure
[x] F-creature-info        Creature info panel with follow button
[x] F-debug-menu           Move spawn/summon into debug menu
[x] F-elf-sprite           Billboard elf sprite rendering
[x] F-event-loop           Event-driven tick loop (priority queue)
[x] F-food-gauge           Creature food gauge with decay
[x] F-game-session         Game session autoload singleton
[x] F-gdext-bridge         gdext compilation and Rust bridge
[x] F-godot-setup          Godot 4 project setup
[x] F-hilly-terrain        Hilly forest floor with dirt voxels
[x] F-ladders              Rope/wood ladders as cheap connectors
[x] F-large-nav-tolerance  1-voxel height tolerance for large nav
[x] F-large-pathfind       2x2 footprint nav grid
[x] F-main-menu            Main menu UI
[x] F-move-interp          Smooth creature movement interpolation
[x] F-mp-integ-test        Multiplayer integration test harness
[x] F-music-gen            Palestrina-style music generator (standalone)
[x] F-nav-graph            Navigation graph construction
[x] F-nav-incremental      Incremental nav graph updates
[x] F-new-game-ui          New game screen with tree presets
[x] F-orbital-cam          Orbital camera controller
[x] F-pathfinding          A* pathfinding over nav graph
[x] F-pause-menu           In-game pause overlay
[x] F-save-load            Save/load to JSON with versioning
[x] F-selection            Click-to-select creatures
[x] F-serde                Serialization for all sim types
[x] F-shared-prng          Shared PRNG crate across all Rust crates
[x] F-sim-commands         SimCommand pipeline
[x] F-spawn-toolbar        Spawn toolbar and placement UI
[x] F-structure-reg        Completed structure registry + UI panel
[x] F-tree-gen             Procedural tree generation (trunk+branches)
[x] F-tree-info            Tree stats/info panel
[x] F-tree-overlap         Construction overlap with tree geometry
[x] F-voxel-fem            Voxel FEM structural analysis
```

---

## Detailed Items

Full descriptions grouped by area. Each item includes design doc references,
draft docs, and blocking relationships where relevant.

### Construction

#### F-batch-blueprint — Batch blueprinting with dependency order
**Status:** Todo · **Phase:** 3 · **Refs:** §12

Queue multiple blueprints with automatic dependency ordering (e.g., build
the platform before the walls on top of it). Structural warnings for
blueprints that would create unsupported geometry.

**Related:** F-blueprint-mode, F-struct-basic

#### F-blueprint-mode — Layer-based blueprint selection UI
**Status:** Todo · **Phase:** 2 · **Refs:** §12

Full blueprint mode with layer-based (Y-level) selection, ghost previews for
arbitrary shapes, and structural warnings. Currently only rectangular platform
designation exists via `construction_controller.gd`. This item covers the
general-purpose blueprint UI that supports all build types and freeform shapes.

**Related:** F-construction, F-batch-blueprint, F-stress-heatmap

#### F-branch-growth — Grow branches for photosynthesis/fruit
**Status:** Todo · **Phase:** 3 · **Refs:** §8, §13

Player-directed branch/bough growth to extend the tree for more
photosynthesis capacity and fruit production. Uses the existing tree
generation algorithm with player-chosen growth direction.

**Related:** F-mana-system, F-mass-conserve

#### F-bridges — Bridge construction between tree parts
**Status:** Todo · **Phase:** 3 · **Refs:** §11

Bridges and walkways connecting different parts of the tree. Requires new
build type UI for specifying start/end anchor points and path.

**Related:** F-tree-overlap, F-struct-basic

#### F-building — Building construction (paper-thin walls)
**Status:** Done · **Phase:** 2 · **Refs:** §11

Buildings with paper-thin walls using per-face restrictions on passable
`BuildingInterior` voxels. Unlike platforms (solid cubes), building walls
don't consume voxel space — each face of a `BuildingInterior` voxel can
be Wall, Window, Door, Ceiling, Floor, or Open. Exterior sides are
windows, one auto-placed door at center of +Z edge, floor on bottom,
ceiling on top. Min footprint 3x3, height 1-5.

Nav graph is face-aware: walls/windows block movement, doors allow
passage, creatures can climb exterior walls and walk on roofs. Rendered
as oriented quads per face type with MultiMesh batching. Construction UI
adds Building [G] mode alongside Platform [P]. Full construction
lifecycle: designate, build (incremental voxel materialization by elves),
cancel (reverts voxels and face data). Save/load preserves buildings.

**New files:** `building.rs`, `building_renderer.gd`
**Related:** F-construction, F-furnishing

#### F-carve-holes — Remove material (doors, storage hollows)
**Status:** Todo · **Phase:** 3 · **Refs:** §11

Remove material from existing tree or construction geometry to create
doorways, windows, storage hollows. The inverse of construction — needs
structural integrity checks to prevent catastrophic removal.

**Blocked by:** F-struct-basic

#### F-choir-build — Choir-based construction singing
**Status:** Todo · **Phase:** 2 · **Refs:** §11, §21

Elves assemble into choirs to sing the tree into growing. Construction speed
and quality depend on choir composition and harmony. Ties into the music
system.

**Related:** F-choir-harmony, F-music-runtime, F-mana-system

#### F-construction — Platform construction (designate/build/cancel)
**Status:** Done · **Phase:** 2 · **Refs:** §11, §12

Basic construction loop: player designates rectangular platforms via the
construction controller UI, sim validates (all voxels Air, at least one
adjacent to solid), creates a blueprint + Build task, elves claim the task
and incrementally materialize voxels. Cancellation reverts placed voxels.
Incremental nav graph updates keep pathfinding current during construction.

**Related:** F-building

#### F-furnishing — Building geometry + purpose furnishing
**Status:** Todo · **Phase:** 3 · **Refs:** §11

Separate building geometry (walls, floors, roofs) from furnishing (beds,
tables, workshops). Generic enclosed spaces are built first, then furnished
to give them purpose.

#### F-ladders — Rope/wood ladders as cheap connectors
**Status:** Done · **Phase:** 3 · **Refs:** §11

Wood and rope ladders as lightweight vertical connectors. Non-solid voxels
with per-face orientation (FaceData). Wood ladders require adjacent solid;
rope ladders require top anchor. Species-specific traversal speeds. Full
construction lifecycle (designate/build/cancel) with structural validation,
tree overlap support, incremental nav graph updates, and oriented thin-panel
rendering.

**New files:** `ladder_renderer.gd`
**Related:** F-rope-retract

#### F-mass-conserve — Wood mass tracking and conservation
**Status:** Todo · **Phase:** 2 · **Refs:** §11

Tree tracks stored wood material. Construction consumes wood mass. Growth
produces it. Conservation of mass prevents infinite building.

**Related:** F-mana-system, F-branch-growth

#### F-rope-retract — Retractable rope ladders (furl/unfurl)
**Status:** Todo · **Phase:** 3 · **Refs:** §11

Rope ladders can be furled (retracted) and unfurled by elves as a task/job.
Furled ladders are impassable and visually show their rolled-up state.
Requires selectable structures so the player can click a rope ladder to see
its furled/unfurled status and request a state change. The structure's
selection UI should display any ongoing or queued furling/unfurling tasks.

**Blocked by:** F-select-struct
**Related:** F-ladders

#### F-stairs — Stairs and ramps for vertical movement
**Status:** Todo · **Phase:** 3 · **Refs:** §11

Stairs and ramps for connecting vertical levels. Requires nav graph edges
with appropriate movement cost (climb speed vs walk speed).

**Related:** F-tree-overlap, F-struct-basic

#### F-task-priority — Priority queue and auto-assignment
**Status:** Todo · **Phase:** 2 · **Refs:** §11, §15

Task queue with Low/Normal/High/Urgent priorities, auto-assignment of idle
elves to highest-priority available tasks. Priority is already in the data
model but not yet used for scheduling.

**Related:** F-elf-needs

#### F-tree-overlap — Construction overlap with tree geometry
**Status:** Done · **Phase:** 2 · **Refs:** §11, §12
**Draft:** `docs/drafts/construction_tree_overlap.md`

Structural build types (platforms, bridges, stairs) should be allowed to
overlap tree geometry. Voxels that are already wood (Trunk/Branch/Root) get
no blueprint voxel. Leaf/Fruit voxels get blueprinted and converted to wood
during construction. Ghost voxels inside existing solid material render as
wireframe edges. Invalid if 0% of voxels are exterior. Adds
`BuildType::allows_tree_overlap()` flag to distinguish structural types from
future furniture/decoration types. See draft doc for full plan.

**Related:** F-construction, F-blueprint-mode

#### F-undo-designate — Undo last construction designation
**Status:** Todo · **Phase:** 2

Undo the most recent construction designation (Ctrl+Z or similar). Currently
players can cancel in-progress builds, but a misclicked designation requires
manually selecting and cancelling. A simple undo stack (last-in-first-out)
for designations would prevent frustration from placement mistakes.

**Related:** F-construction

#### F-visual-smooth — Smooth voxel surface rendering
**Status:** Todo · **Phase:** 2 · **Refs:** §8

Platforms and construction should render with smoothed surfaces rather than
raw cubes. Exact technique TBD (marching cubes variant, mesh smoothing, or
shader-based rounding).

#### F-wireframe-ghost — Wireframe ghost for overlap preview
**Status:** Todo · **Phase:** 2

During placement preview, voxels that overlap existing tree wood
(Trunk/Branch/Root) should render as wireframe edges instead of solid
translucent cubes. Requires splitting the blueprint renderer's ghost
MultiMesh into two layers (solid for buildable voxels, wireframe for
already-wood voxels) and a wireframe shader. The sim bridge needs to
expose per-voxel overlap classification to GDScript. See section 4 of
`docs/drafts/construction_tree_overlap.md` for rendering design notes.

**Related:** F-tree-overlap, F-blueprint-mode

### Structural Integrity & Fire

#### B-dirt-not-pinned — Dirt unpinned in fast structural validator
**Status:** Done

`build_network_from_set()` (used by `validate_blueprint_fast()`) only pins
`ForestFloor` voxels, not `Dirt`. Since Dirt has density 999, unpinned Dirt
acts as massive dead weight in the weight-flow analysis, causing all
structures near hilly terrain to fail validation. One-line fix: add
`|| vt == VoxelType::Dirt` to match the full solver's pinning logic.

#### B-dead-node-panic — Panic on dead nav node in pathfinding
**Status:** Done

Creature pathfinding panics (`unwrap()` on `None`) when a task's `location`
nav node has been removed by an incremental nav graph update (e.g.
construction solidifying a voxel). Fix: guard `execute_task_behavior` and
`process_creature_activation` to check node liveness before pathfinding,
resnapping or abandoning the task if the node is dead.

#### B-preview-blueprints — Preview treats blueprints as complete
**Status:** Todo · **Phase:** 2

Structural preview during placement (`validate_platform_preview`,
`validate_building_preview`) currently only considers the voxels being placed.
It should also treat any ongoing blueprints and in-progress construction as if
they were already complete, so the player sees the cumulative structural impact
of all planned builds — not just the one currently under the cursor.

**Related:** F-voxel-fem

#### F-cascade-fail — Cascading structural failure
**Status:** Todo · **Phase:** 5 · **Refs:** §9
**Draft:** `docs/drafts/structural_integrity.md` §11

When overloaded voxels fail, load redistributes to neighbors, potentially
causing chain failures. Disconnected chunks fall as rigid bodies. Requires
fall physics, impact damage, and creature displacement on top of the
spring-mass solver from F-voxel-fem. See draft §11 for scoping notes.

**Blocked by:** F-voxel-fem

#### F-fire-advanced — Heat accumulation and ignition thresholds
**Status:** Todo · **Phase:** 5 · **Refs:** §16

Fire Stage 2: heat accumulation model, per-material ignition thresholds,
green wood vs dry wood distinction.

**Blocked by:** F-fire-basic

#### F-fire-basic — Fire spread and voxel destruction
**Status:** Todo · **Phase:** 5 · **Refs:** §16

Fire simulation Stage 1: basic probabilistic spread between adjacent
flammable voxels, voxel destruction when fully burned.

#### F-fire-ecology — Fire as ecological force, firefighting
**Status:** Todo · **Phase:** 7 · **Refs:** §16

Fire Stages 3-4: environmental factors (wind, rain), organized
firefighting by elves, fire as an ecological renewal force.

**Blocked by:** F-fire-advanced

#### F-fire-structure — Fire x structural integrity cascades
**Status:** Todo · **Phase:** 5 · **Refs:** §9, §16
**Draft:** `docs/drafts/structural_integrity.md` §11

Burning supports trigger structural collapse cascades. Performance concern:
fire destroying load-bearing voxels triggers spring-mass solver
recalculation during an already-expensive fire tick (§27). Tree voxels have
very high but finite strength (draft §6), so fire can theoretically bring
down branches.

**Blocked by:** F-voxel-fem, F-fire-basic, F-cascade-fail

#### F-partial-struct — Structural checks on incomplete builds
**Status:** Todo · **Phase:** 8+ · **Refs:** §9
**Draft:** `docs/drafts/structural_integrity.md` §12.3

Detect and handle structurally unsound partial construction — e.g., a player
designates a structurally sound arch, then cancels mid-construction leaving
an unsound cantilever remnant. Possible mitigations: structural check on
cancellation, periodic structural heartbeat for incomplete structures, or
limits on how far construction can extend from support before the next
anchor is in place.

**Blocked by:** F-voxel-fem

#### F-stress-heatmap — Stress visualization in blueprint mode
**Status:** Todo · **Phase:** 5 · **Refs:** §9, §12
**Draft:** `docs/drafts/structural_integrity.md` §7, §14-F

Overlay showing per-voxel stress levels during blueprint planning. Color-map
from green (safe) through yellow (moderate) to red (failure). Uses reduced
solver iterations (~20–30) for responsive preview during placement, full
iterations on confirm. See draft §7.2 for `BlueprintValidation` data
structure and §7.4 for performance budget.

**Blocked by:** F-voxel-fem
**Related:** F-blueprint-mode

#### F-struct-basic — Basic structural integrity (flood fill)
**Status:** Todo · **Phase:** 3 · **Refs:** §9
**Draft:** `docs/drafts/structural_integrity.md` §8

Connectivity flood fill: can every solid voxel reach a grounded voxel
(ForestFloor or trunk-to-ground) via face-adjacent solid voxels? Disconnected
clusters are flagged. Used as a fast pre-filter in F-voxel-fem blueprint
validation (draft §7.3). The `flood_fill_connected()` function is shared
between this feature and the FEM system.

**Blocks:** F-carve-holes
**Related:** F-voxel-fem

#### F-voxel-fem — Voxel FEM structural analysis
**Status:** Done · **Phase:** 5 · **Refs:** §9
**Draft:** `docs/drafts/structural_integrity.md`

Spring-mass network structural solver (iterative relaxation, not classical
FEM matrices — avoids DOF mismatch with building shell elements). Solid
voxels are mass-spring nodes; building faces (Wall, Window, Door, Floor,
Ceiling) generate shell-like springs with per-face-type stiffness/strength.
Tree voxels participate with very high but finite strength. Material
properties are data-driven via `StructuralConfig` in GameConfig.

Key deliverables: spring-mass solver (`structural.rs`), tree generation
validation (retry up to 4 times if tree fails under own weight), tiered
blueprint validation (OK / Warning / Blocked based on stress thresholds),
bridge method for GDScript stress heatmap data. Construction intermediate
states are exempt from checks (draft §12).

**Related:** F-struct-basic, F-stress-heatmap, F-cascade-fail,
F-partial-struct

### Navigation & Pathfinding

#### F-flying-nav — 3D flight navigation system
**Status:** Todo · **Phase:** 8+ · **Refs:** §10

Full 3D movement for birds and winged elves. Separate from the
surface-based nav graph — likely a volumetric approach.

#### F-large-pathfind — 2x2 footprint nav grid
**Status:** Done · **Phase:** 8+ · **Refs:** §10

Separate pre-baked `NavGraph` for 2x2x2 footprint creatures (elephants).
Nodes only where a 2x2x2 volume is clear and all 4 ground cells are solid.
Includes `Species::Elephant`, `graph_for_species()` dispatch, incremental
updates, SimBridge queries, GDScript spawn/render/placement, and sprite.

#### F-large-nav-tolerance — 1-voxel height tolerance for large nav
**Status:** Done · **Phase:** 8+

Allow up to 1 voxel of height variation within a large creature's 2x2
footprint and between adjacent large nav nodes. Fixes elephant navigation
on hilly terrain (F-hilly-terrain) where variable-height dirt voxels
fragmented the large nav graph.

#### F-nav-graph — Navigation graph construction
**Status:** Done · **Phase:** 1 · **Refs:** §10

26-connectivity nav graph built from voxel world. Nav node at every air
voxel adjacent to solid. Duplicate edges avoided via 13-offset trick.

#### F-nav-incremental — Incremental nav graph updates
**Status:** Done · **Phase:** 2 · **Refs:** §10

`update_after_voxel_solidified()` updates ~7 affected positions after each
voxel placement instead of full graph rebuild. Returns removed NodeIds for
creature resnapping.

#### F-pathfinding — A* pathfinding over nav graph
**Status:** Done · **Phase:** 1 · **Refs:** §10

A* search with euclidean heuristic over the nav graph. Movement cost
computed from edge distance and per-species speed config.

### Creatures & Needs

#### F-capybara — Capybara species
**Status:** Done · **Refs:** §15

Capybara species with ground-only movement restriction, own sprite renderer,
and species-specific speed config.

#### F-creature-death — Basic creature death (starvation)
**Status:** Todo · **Phase:** 3 · **Refs:** §13, §15

When a creature's food gauge reaches zero, it dies and is removed from the
simulation. Basic death mechanic without the spiritual dimension (soul
passage, resurrection) covered by F-soul-mech. Needs: death event, creature
removal, corpse cleanup, UI notification. A prerequisite for food scarcity
having real consequences.

**Related:** F-food-gauge, F-soul-mech, F-elf-needs

#### F-elf-needs — Hunger and rest self-direction
**Status:** In Progress · **Phase:** 3 · **Refs:** §13, §15

Elves autonomously seek food (eat fruit from trees) and rest (find sleeping
spots) when needs are low. Self-directed behavior that interrupts assigned
tasks when needs are critical.

**Hunger (done):** Idle creatures with food below `food_hunger_threshold_pct`
(default 50%) get an `EatFruit` task created at heartbeat time, pathfind to
the nearest fruit voxel, eat it (restoring `food_restore_pct`% of food_max),
and remove the fruit from the world.

**Rest/sleep:** Not yet implemented.

**Related:** F-food-gauge, F-task-priority

#### F-elf-sprite — Billboard elf sprite rendering
**Status:** Done · **Phase:** 1 · **Refs:** §24

Billboard chibi elf sprites using pool pattern. Procedurally generated from
seed via `sprite_factory.gd`. Offset +0.48 Y for visual centering.

#### F-food-gauge — Creature food gauge with decay
**Status:** Done · **Refs:** §13

Food level per creature, decaying over time. Displayed in creature info
panel and as overhead bar.

#### F-move-interp — Smooth creature movement interpolation
**Status:** Done · **Refs:** §10

Creatures glide between nav nodes instead of teleporting. Each creature
stores `move_from`/`move_to`/`move_start_tick`/`move_end_tick` as rendering
metadata. `interpolated_position()` lerps based on `render_tick`.

#### F-population — Natural population growth/immigration
**Status:** Todo · **Phase:** 3 · **Refs:** §13, §15

Elves arrive naturally rather than only via the debug spawn toolbar.
Immigration attracted by tree quality (size, fruit production, shelter,
mana level). Possible birth mechanic for established populations. Rate
limited by tree carrying capacity and available food/shelter.

**Related:** F-mana-system, F-tree-capacity, F-fruit-prod

### Economy & Logistics

#### F-crafting — Non-construction jobs and crafting
**Status:** Todo · **Phase:** 8+ · **Refs:** §11

Jobs beyond construction: woodworking, weaving, cooking, enchanting.
Crafting system for tools, furniture, and magical items.

#### F-fruit-prod — Basic fruit production and harvesting
**Status:** Todo · **Phase:** 2 · **Refs:** §13

Tree produces fruit at Leaf voxels over time. Elves harvest fruit to refill
their food gauge. Production rate depends on number of Leaf voxels
(photosynthesis capacity). Basic version: fruit spawns periodically at
random Leaf-adjacent positions, elves pathfind to harvest. Bridges the gap
between the existing food decay mechanic (F-food-gauge) and the advanced
food system (F-fruit-variety).

**Related:** F-food-gauge, F-elf-needs, F-fruit-variety, F-branch-growth

#### F-fruit-variety — Food storage, cooking, magical brewing
**Status:** Todo · **Phase:** 7 · **Refs:** §13

Multiple fruit types, food storage infrastructure, cooking for better food
quality, and magical brewing from rare ingredients.

#### F-logistics — Spatial resource flow (Kanban-style)
**Status:** Todo · **Phase:** 7 · **Refs:** §14

Resources flow through spatial paths — stockpiles, workshops, delivery
routes. Kanban-inspired pull system rather than global resource pools.

#### F-mana-system — Mana generation, storage, and spending
**Status:** Todo · **Phase:** 2 · **Refs:** §11, §13

Core mana economy: tree stores mana, elves generate it (flat rate initially),
construction and growth spend it. The central feedback loop — happy elves
produce more mana, mana enables growth, growth makes elves happier.

**Related:** F-mana-mood, F-choir-build, F-mass-conserve

#### F-tree-capacity — Per-tree carrying capacity limits
**Status:** Todo · **Phase:** 7 · **Refs:** §13

Each tree has a carrying capacity limiting how many elves/structures it can
support. Encourages distributed village design across multiple trees.

**Related:** F-multi-tree

### Social & Emotional

#### F-apprentice — Skill transfer via proximity
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elves learn skills by working near skilled elves. Apprenticeship as an
emergent social/economic system.

#### F-emotions — Multi-dimensional emotional state
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Emotions as multiple simultaneous dimensions: joy, fulfillment, sorrow,
stress, pain, fear, anxiety. Not a single "happiness" number.

#### F-hedonic-adapt — Asymmetric hedonic adaptation
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elves adapt to good conditions faster than bad ones. A beautiful new
platform stops feeling special after a while, but a cold sleeping spot
never stops being miserable.

**Blocked by:** F-emotions

#### F-mana-mood — Mana generation tied to elf mood
**Status:** Todo · **Phase:** 4 · **Refs:** §11, §18

Replace flat-rate mana generation with mood-dependent rates. Happy elves
generate more mana, completing the core feedback loop.

**Blocked by:** F-mana-system, F-emotions

#### F-mood-system — Mood with escalating consequences
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Sustained emotional states become moods. Escalating consequences: mild
unhappiness reduces work speed, severe unhappiness causes task refusal,
critical states trigger dramatic actions.

**Blocked by:** F-emotions

#### F-narrative-log — Events and narrative log
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Sim emits narrative events (arguments, friendships formed, dramatic moments).
Log viewable by player, drives emergent storytelling.

#### F-personality — Personality axes affecting behavior
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Multi-axis personality model affecting task preferences, social behavior,
stress responses, and creative output.

#### F-poetry-reading — Social gatherings and poetry readings
**Status:** Todo · **Phase:** 4 · **Refs:** §18, §20

Elves gather for poetry readings, festivals, and social events. Quality of
poetry/music affects mood and mana generation.

**Related:** F-proc-poetry, F-vaelith-expand

#### F-seasons — Seasonal visual and gameplay effects
**Status:** Todo · **Phase:** 4 · **Refs:** §8, §18

Leaf color changes, snow, seasonal fruit production variation. Gameplay
effects: cold weather increases clothing need, leaf drop reduces canopy
shelter.

#### F-social-graph — Relationships and social contagion
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elf-to-elf relationships: friendships, rivalries, romantic bonds, mentorship.
Emotional contagion spreads mood through social connections.

**Related:** F-emotions, F-personality

### Culture, Language & Music

#### F-audio-sampled — Sampled vocal syllables from conlang
**Status:** Todo · **Phase:** 8+ · **Refs:** §21

Phase 2 audio: pre-recorded or AI-generated vocal syllables from the Vaelith
phoneme inventory, concatenated for singing.

**Blocked by:** F-audio-synth, F-vaelith-expand

#### F-audio-synth — Waveform synthesis for audio rendering
**Status:** Todo · **Phase:** 6 · **Refs:** §21

Phase 1 audio: generate waveforms from MIDI-like note data for playback in
Godot. Debugging and validation tool, placeholder for richer audio later.

#### F-audio-vocal — Continuous vocal synthesis
**Status:** Todo · **Phase:** 8+ · **Refs:** §21

Phase 3 audio: real-time continuous vocal synthesis. Far future.

**Blocked by:** F-audio-sampled

#### F-choir-harmony — Ensemble harmony in construction singing
**Status:** Todo · **Phase:** 6 · **Refs:** §11, §21

Multiple elves singing in harmony during construction. Choir composition
affects construction speed/quality. Ties music generation into the core
gameplay loop.

**Related:** F-choir-build, F-music-runtime

#### F-elf-names — Elf name generation from conlang rules
**Status:** Todo · **Phase:** 6 · **Refs:** §20

Generate elf names using Vaelith phonotactic rules. Names are compounds of
meaningful roots (e.g., *Thíraleth* = "star-tree"), genderless, with given
name + surname structure. Names should sound consistent with the conlang and
be deterministic given the same PRNG state. Adds a `name` field to the
`Creature` struct, assigned at spawn time.

**Blocked by:** F-lang-crate
**Related:** F-vaelith-expand

#### F-lang-crate — Shared Vaelith language crate
**Status:** Todo · **Phase:** 6 · **Refs:** §20
**Draft:** `docs/drafts/lang_crate.md`

Create `elven_canopy_lang`, a pure-Rust crate providing the Vaelith language
as a programmatic resource shared by the sim and music crates. Includes:
data-driven lexicon (`data/vaelith_lexicon.json`) with part-of-speech, tones,
vowel class, and name tags; core language types (`Tone`, `VowelClass`,
`Syllable`, `LexEntry`) migrated from the music crate; phonotactic rules;
and a deterministic name generator. See draft doc for full design including
lexicon schema, crate structure, and what moves vs stays in the music crate.

**Blocks:** F-elf-names, F-music-use-lang, F-vaelith-expand

#### F-music-gen — Palestrina-style music generator (standalone)
**Status:** Done · **Phase:** 6 · **Refs:** §21
**Crate:** `elven_canopy_music`

Complete standalone generator: Palestrina-style SATB counterpoint with
Vaelith lyrics, Markov melodic models trained on Renaissance corpus,
simulated annealing optimization, MIDI + LilyPond output, CLI with
batch/mode-scan.

#### F-music-runtime — Integrate music generator into game
**Status:** Todo · **Phase:** 6 · **Refs:** §21

Bridge the standalone music crate into the Godot runtime. Generate music
in response to game events (construction, celebrations, idle time). Requires
audio output path (see F-audio-synth).

**Blocked by:** F-audio-synth

#### F-music-use-lang — Migrate music crate to shared lang crate
**Status:** Todo · **Phase:** 6

Migrate `elven_canopy_music` to depend on `elven_canopy_lang` for Vaelith
types and lexicon data instead of maintaining its own hardcoded vocabulary.
The music crate keeps its phrase-generation templates, brightness-biased
selection, and SA text-swap logic, but delegates to the lang crate for
vocabulary lookup, core types (`Tone`, `VowelClass`, `Syllable`), and
phonotactic rules. Also switches from `rand` to `elven_canopy_prng` as part
of this migration (the `rand` → `elven_canopy_prng` migration is already done via F-shared-prng).

**Blocked by:** F-lang-crate

#### F-proc-poetry — Procedural poetry via simulated annealing
**Status:** Todo · **Phase:** 6 · **Refs:** §20

Generate Vaelith-language poetry using simulated annealing (similar to the
music generator's approach). Poetry quality varies by elf skill, affects
social events and mana.

**Blocked by:** F-vaelith-expand

#### F-sound-effects — Basic ambient and action sound effects
**Status:** Todo · **Phase:** 3

Basic audio feedback: ambient forest sounds (wind, birds, rustling leaves),
construction sounds (singing, wood growing), footstep sounds, UI feedback
sounds. Distinct from F-audio-synth (which renders music from note data) —
this covers simple sampled sound effects loaded and played through Godot's
AudioStreamPlayer. Placeholder sounds initially, replaceable later.

**Related:** F-audio-synth

#### F-vaelith-expand — Expand Vaelith language for runtime use
**Status:** Todo · **Phase:** 6 · **Refs:** §20

Expand the Vaelith lexicon and grammar rules beyond the rudimentary vocabulary
established by F-lang-crate. Larger dictionary with thematic domains, richer
morphology (case, aspect, evidentials), grammar sufficient for procedural
poetry and elf dialogue. Intersects with voice recording work (phoneme
inventory may still change). Builds on the `elven_canopy_lang` crate
infrastructure.

**Blocked by:** F-lang-crate
**Blocks:** F-proc-poetry
**Related:** F-elf-names, F-audio-sampled

### Combat & Defense

#### F-combat — Combat and invader threat system
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Invader types, threat mechanics, and basic combat resolution. Ties into
fog of war for surprise attacks.

**Related:** F-fog-of-war

#### F-defense-struct — Defensive structures (ballista, wards)
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Ballista turrets, magic wards, and other defensive construction. Requires
the construction system to support these build types.

**Blocked by:** F-combat

#### F-elf-weapons — Bows, spears, clubs for elf combat
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Weapon types with different ranges, damage, and crafting requirements.

**Blocked by:** F-combat, F-crafting

#### F-military-campaign — Send elves on world expeditions
**Status:** Todo · **Phase:** 8+ · **Refs:** §26

Send elf parties on expeditions in the wider world with direct tactical
control (unlike Dwarf Fortress's hands-off approach).

**Blocked by:** F-combat, F-military-org

#### F-military-org — Squad management and organization
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Organize elves into military squads with patrol routes, defensive
positions, and alert levels.

**Blocked by:** F-combat

### World Expansion & Ecology

#### F-cultural-drift — Inter-tree cultural divergence
**Status:** Todo · **Phase:** 7 · **Refs:** §7, §18

Elves on different trees develop distinct traditions, art styles, and
social norms over time.

**Blocked by:** F-multi-tree, F-personality

#### F-hilly-terrain — Hilly forest floor with dirt voxels
**Status:** Done · **Phase:** 2

Replace the flat 1-voxel ForestFloor with natural-looking hilly terrain made of
`Dirt` voxels, 1–4 voxels thick, generated with value noise + bilinear
interpolation. Dirt has voxel priority 0 (tree voxels overwrite it), maps to
`ForestFloor` for navigation (ground-only creatures walk on hills), and is
pinned in the structural solver. Large nav graph updated to handle variable
terrain height.

#### F-multi-tree — NPC trees with personalities
**Status:** Todo · **Phase:** 7 · **Refs:** §2, §7

Multiple trees in the world, each with personality traits (preferences,
aversions) that affect mana generation and elf morale. Also enables
**separate-tree multiplayer** — each player controls their own tree with
their own elves and mana, in cooperative or competitive configurations
(see `design_doc.md` §1 and `docs/drafts/multiplayer_relay.md` §4). Requires
per-player entity ownership, per-player command validation, and per-player
fog of war rendering.

**Related:** F-multiplayer

#### F-root-network — Root network expansion and diplomacy
**Status:** Todo · **Phase:** 7 · **Refs:** §2

Player grows roots toward other trees. Diplomacy phase: mana offerings
convince trees to join the network. Expands buildable space and perception
radius.

**Blocked by:** F-multi-tree, F-mana-system

#### F-tree-memory — Ancient tree knowledge/vision system
**Status:** Todo · **Phase:** 7 · **Refs:** §2

The player's tree surfaces ancient memories: hints about threats, lost
construction techniques, forest history. Journal or vision system.

### Soul Mechanics & Magic

#### F-magic-items — Magic item personalities and crafting
**Status:** Todo · **Phase:** 8+ · **Refs:** §22

Magic items with emergent personalities from their crafting circumstances
and the souls/emotions imbued in them.

**Related:** F-soul-mech, F-crafting

#### F-soul-mech — Death, soul passage, resurrection
**Status:** Todo · **Phase:** 8+ · **Refs:** §19

Elf death, soul passage into trees, possible resurrection, and
soul-powered constructs (golems, animated defenses).

### UI & Presentation

#### F-ai-sprites — AI-generated sprite art pipeline
**Status:** Todo · **Phase:** 8+ · **Refs:** §24

Replace placeholder sprites with AI-generated layered art: base body
templates + composited clothing/hair/face layers for visual variety.

#### F-build-queue-ui — Construction queue/progress UI
**Status:** Todo · **Phase:** 2

UI panel showing all pending and in-progress construction projects: blueprint
name/type, progress bar, assigned workers, and option to cancel or reprioritize.
Currently players can see individual blueprints in the world but have no
overview of the construction pipeline. Small overlay or sidebar panel.

**Related:** F-construction, F-task-priority

#### F-cam-follow — Camera follow mode for creatures
**Status:** Done · **Phase:** 2 · **Refs:** §23

Lock camera focal point to a selected creature. Toggled via creature info
panel button.

#### F-creature-info — Creature info panel with follow button
**Status:** Done · **Refs:** §26

Right-side panel showing creature details (species, food level, task,
position). Follow button to lock camera.

#### F-creature-tooltip — Creature hover tooltip
**Status:** Todo · **Phase:** 2

Quick info on mouse hover over a creature without requiring click-to-select.
Shows species, name (once F-elf-names exists), and current activity in a
small floating tooltip. Reduces friction compared to the full selection +
info panel flow for casual inspection.

**Related:** F-selection, F-creature-info, F-elf-names

#### F-debug-menu — Move spawn/summon into debug menu
**Status:** Done · **Phase:** 2

The top toolbar (`spawn_toolbar.gd`) currently has 11 buttons: 6 creature
spawn buttons (Elf, Capybara, Boar, Deer, Monkey, Squirrel), Summon Elf,
Build, Tasks, Structures, and Tree Info. Most of the bar is dev/debug tools
(spawning creatures on demand) that won't exist in the real game — they
clutter the toolbar and push gameplay buttons off to the side.

Move all 6 creature spawn buttons and the Summon Elf button into a toggleable
debug menu (collapsible panel, dropdown, or separate overlay triggered by a
"Debug" button or a key like F12). The main toolbar keeps only gameplay
actions: Build, Tasks, Structures, Tree Info (and future gameplay buttons
like speed controls). Rename `spawn_toolbar.gd` → `action_toolbar.gd` (and
update references in `main.gd`, `placement_controller.gd`, CLAUDE.md project
structure) to reflect that it's no longer spawn-centric.

The debug menu should be easy to hide entirely for non-dev builds later.

**Related:** F-spawn-toolbar

#### F-godot-setup — Godot 4 project setup
**Status:** Done · **Phase:** 0 · **Refs:** §3

Godot 4 project with GDExtension configuration.

#### F-lod-sprites — LOD sprites (chibi / detailed)
**Status:** Todo · **Phase:** 8+ · **Refs:** §24

High-detail anime sprites at close zoom, low-detail chibi at far zoom.
Deferred until camera zoom range demands it.

#### F-main-menu — Main menu UI
**Status:** Done · **Refs:** §26

Main menu with New Game, Load, and Quit buttons.

#### F-new-game-ui — New game screen with tree presets
**Status:** Done · **Refs:** §26

Seed input, tree parameter sliders, preset buttons for different tree
shapes.

#### F-orbital-cam — Orbital camera controller
**Status:** Done · **Phase:** 0 · **Refs:** §23

Orbit, zoom, pan. Smooth interpolation. Follow mode for creatures.

#### F-pause-menu — In-game pause overlay
**Status:** Done · **Refs:** §26

ESC-triggered pause menu with Resume, Save, Load, and Quit options.

#### F-selection — Click-to-select creatures
**Status:** Done · **Refs:** §26

Ray-based selection with billboard sprite hit detection. ESC to deselect.
Input precedence chain with placement and pause systems.

#### F-sim-speed — Simulation speed controls UI
**Status:** Todo · **Phase:** 2

Pause/1x/2x/3x speed controls for the simulation. The sim architecture
already supports variable tick rates (time-based accumulator in `main.gd`).
This adds UI buttons and keyboard shortcuts (e.g., Space for pause, 1/2/3
for speed) to control the tick multiplier. Essential for both slow
observation and fast-forwarding through idle periods.

**Related:** F-event-loop

#### F-select-struct — Selectable structures with interaction UI
**Status:** Todo · **Phase:** 3

Click-to-select completed structures (platforms, buildings, ladders, etc.)
with an info panel showing structure type, dimensions, health/stress, and
structure-specific actions. Extends the existing creature selection system
to handle structure entities. Foundation for per-structure interaction like
rope ladder furling, building furnishing, and structure demolition.

**Related:** F-selection, F-structure-reg, F-rope-retract

#### F-spawn-toolbar — Spawn toolbar and placement UI
**Status:** Done · **Refs:** §26

Toolbar with creature spawn buttons and keyboard shortcuts. Placement
controller handles click-to-place with nav node highlighting.

#### F-structure-reg — Completed structure registry + UI panel
**Status:** Done · **Phase:** 2

Registry of completed structures in the sim (`SimState.structures`) with a
browsable UI panel. Tracks all build types (Platform, Bridge, Stairs, Wall,
Enclosure, Building) with sequential IDs (#0, #1, ...) and bounding box
for zoom-to-location.

**New files:** `structure_list_panel.gd`
**Related:** F-construction, F-building

#### F-tree-info — Tree stats/info panel
**Status:** Done · **Phase:** 2

Panel showing the player's tree statistics: total voxels, height, branch
count, leaf count, fruit production rate, mana level (once F-mana-system
exists), and carrying capacity. The player *is* the tree but currently has
no introspective view of their own state. Could be a toggleable overlay
or a persistent sidebar element.

**Related:** F-creature-info, F-mana-system

#### F-world-boundary — World boundary visualization
**Status:** Todo · **Phase:** 2

Visual indication of the voxel world's finite boundaries. The world grid has
fixed dimensions but nothing shows the player where the edges are. Could be
subtle ground grid lines, edge fog, fading terrain, or a visible border
when the camera approaches the edge. Prevents confusion when placing
construction near world limits.

#### F-zlevel-vis — Z-level visibility (cutaway/toggle)
**Status:** Todo · **Refs:** §27

How to show lower platforms when upper ones occlude them. Transparency,
cutaway, or hide-upper-levels toggle. Open design question (§27).

### Infrastructure & Multiplayer

#### F-adventure-mode — Control individual elf (RPG-like)
**Status:** Todo · **Phase:** 8+ · **Refs:** §26

Control a single elf in first/third-person perspective within the
same simulation. RPG-like exploration mode.

#### F-core-types — VoxelCoord, IDs, SimCommand, GameConfig
**Status:** Done · **Phase:** 0 · **Refs:** §5, §7

Core data types with deterministic UUID generation from PRNG.

#### F-crate-structure — Two-crate sim/gdext structure
**Status:** Done · **Phase:** 0 · **Refs:** §3, §4

Sim crate has zero Godot dependencies. Compiler-enforced separation
enables headless testing, fast-forward, and replay verification.

#### F-day-night — Day/night cycle and pacing
**Status:** Todo · **Refs:** §27

Length of in-game day. Affects pacing, fruit production, sleep schedules.
Open design question (§27).

#### F-event-loop — Event-driven tick loop (priority queue)
**Status:** Done · **Phase:** 1 · **Refs:** §6

Discrete event simulation with priority queue. Empty ticks are free.
1000 ticks per simulated second.

#### F-fog-of-war — Visibility via tree and root network
**Status:** Todo · **Phase:** 8+ · **Refs:** §17

World hidden except where observed by elves or sensed through tree/root
network. Strongest near trunk, weaker at root edges, absent beyond.

**Related:** F-root-network, F-combat

#### F-game-session — Game session autoload singleton
**Status:** Done · **Refs:** §26

Godot autoload persisting seed and tree config across scene transitions.

#### F-gdext-bridge — gdext compilation and Rust bridge
**Status:** Done · **Phase:** 0 · **Refs:** §3

GDExtension bridge crate exposing sim to Godot. SimBridge node with
methods for commands, queries, and rendering data.

#### F-modding — Scripting layer for modding support
**Status:** Todo · **Refs:** §27

Plugin/scripting system for custom structures, elf behaviors, invader
types. Open design question (§27).

#### F-mp-chat — Multiplayer in-game chat
**Status:** Todo · **Phase:** 8+ · **Refs:** §4

Text chat between players in multiplayer sessions. Protocol support exists
(`Chat`/`ChatBroadcast` messages) but the GDScript UI for displaying and
sending chat messages is not yet implemented.

**Related:** F-multiplayer

#### F-mp-checksums — Multiplayer state checksums for desync detection
**Status:** Todo · **Phase:** 8+ · **Refs:** §4

Periodic sim-state checksums sent via the relay to detect desync between
clients. Protocol support exists (`Checksum`/`DesyncDetected` messages) but
the sim does not yet compute or send checksums. Needs a fast deterministic
hash over relevant sim state (creature positions, voxel grid, etc.).

**Related:** F-multiplayer

#### F-mp-integ-test — Multiplayer integration test harness
**Status:** Done · **Phase:** 8+ · **Refs:** §4

End-to-end integration tests for multiplayer workflows: hosting a game,
joining, issuing commands, verifying both sides see the same state. Should
run entirely in Rust (no Godot dependency) by exercising the relay, net
client, and sim directly. Consider moving UI-adjacent logic (button press
→ command dispatch) into testable Rust functions to maximize coverage
without requiring Godot automation. Goal: catch regressions in the full
host→relay→join→command→turn→apply pipeline.

**Related:** F-multiplayer

#### F-mp-mid-join — Mid-game join with state snapshot
**Status:** Todo · **Phase:** 8+ · **Refs:** §4

Allow players to join a multiplayer session that has already started.
Requires a snapshot protocol: the relay requests a full sim state from an
existing client and sends it to the joining player. Protocol support exists
(`SnapshotRequest`/`SnapshotResponse`/`SnapshotLoad` messages) but the
snapshot flow is not yet implemented.

**Related:** F-multiplayer, F-save-load

#### F-mp-reconnect — Multiplayer reconnection after disconnect
**Status:** Todo · **Phase:** 8+ · **Refs:** §4

Graceful handling of temporary disconnections in multiplayer. When a client
disconnects, preserve their player slot for a timeout period and allow
reconnection with state catchup (replaying missed turns or requesting a
snapshot). Not yet designed in detail.

**Related:** F-multiplayer, F-mp-mid-join

#### F-multiplayer — Relay-coordinator multiplayer networking
**Status:** In Progress · **Phase:** 8+ · **Refs:** §4
**Draft:** `docs/drafts/multiplayer_relay.md`

Multiplayer via a lightweight relay coordinator that determines canonical
command ordering and broadcasts turns to all clients. The relay can run as
a standalone headless binary (`elven_canopy_relay` crate) or embedded in a
player's game process. Clients connect outbound to the relay, avoiding NAT
traversal issues. Supersedes the Paxos-like model originally described in
§4 — simpler, same guarantees for 2–4 player scale. Steam integration
possible as a discovery mechanism (lobby browser, friend invites) without
replacing the relay architecture. Periodic state checksums detect desync.
Architecture foundations are ready (deterministic sim, command interface).
Initial multiplayer mode is shared-tree co-op (all players control one
tree). Separate-tree multiplayer (cooperative or competitive with per-player
trees) deferred to F-multi-tree. Draft doc covers relay architecture,
session management, and UI design (main menu flow, lobby, in-game controls,
ESC menu behavior, save/load semantics, sim speed policy).

**Related:** F-save-load, F-multi-tree

#### F-save-load — Save/load to JSON with versioning
**Status:** Done · **Phase:** 2 · **Refs:** §4, §5

Full sim state serialized to JSON in `user://saves/`. Save versioning
for schema migration.

#### F-serde — Serialization for all sim types
**Status:** Done · **Phase:** 0 · **Refs:** §5

All sim types derive Serialize/Deserialize for save/load and future
network sync.

#### F-shared-prng — Shared PRNG crate across all Rust crates
**Status:** Done · **Phase:** 6

Extract the xoshiro256++ PRNG from `elven_canopy_sim/src/prng.rs` into a new
`elven_canopy_prng` crate. Migrate `elven_canopy_music` from the `rand` crate
to use `GameRng` directly, removing the `rand` dependency entirely. This is a
full migration (~100 call sites across 6 music crate files using
`rng.random()`, `rng.random_range()`, `rng.random_bool()`, etc.) — add
corresponding convenience methods to `GameRng` as needed (`next_f64`,
`random_bool`, etc.). The sim crate re-exports or depends on the new prng
crate in place of its local `prng.rs`.

**Blocks:** F-lang-crate, F-music-use-lang

#### F-sim-commands — SimCommand pipeline
**Status:** Done · **Phase:** 1 · **Refs:** §4

All mutations go through SimCommand for determinism and future multiplayer.

#### F-tree-gen — Procedural tree generation (trunk+branches)
**Status:** Done · **Phase:** 1 · **Refs:** §8

Trunk is first branch — all segments use same growth algorithm with
different params. Cross-section bridging ensures 6-connectivity. Voxel
type priority prevents overwrites.

#### F-weather — Weather within seasons
**Status:** Todo · **Refs:** §27

Rain, wind, storms within seasons. Could affect mood, fire spread, and
construction difficulty. Open design question (§27).

**Related:** F-seasons, F-fire-ecology

