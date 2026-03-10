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

**CLI tool:** Query and mutate this file via the CLI:
```
python3 scripts/tracker.py <command> [args]
```
Commands: `list`, `show`, `search`, `change-state`, `add`, `edit-title`,
`edit-description`, `block`, `unblock`, `relate`, `unrelate`, `fix`.
All mutation commands auto-run `fix` at the end, which enforces alphabetical
ordering, removes `Blocks`/`Blocked by` fields from done items, strips
references to done items from other items' `Blocked by` fields, and ensures
`Blocks`/`Blocked by` and `Related` pairs are symmetric. Use `--dry-run` on
any mutation to preview changes.

## Summary

Condensed single-line-per-item view. Grouped by status: in progress first, then
todo, then done. Every item here MUST have a corresponding entry in [Detailed
Items](#detailed-items) and vice versa.

**Format:** Each line is `[status] ID` padded to 26 characters, then a short
title. Example: `[ ] F-example-name         Short title here`. When an item
changes status, update the marker AND move the line to the correct section.

**Ordering:** Items are sorted alphabetically by ID within each section (In
Progress, Todo, Done) and within each topic group in the detailed section.
This reduces merge conflicts when parallel work streams add items.

### In Progress

```
[~] F-enemy-ai             Hostile creature AI (goblin/orc/troll behavior)
[~] F-fruit-variety        Procedural fruit variety and processing
[~] F-multiplayer          Relay-coordinator multiplayer networking
[~] F-notifications        Player-visible event notifications
[~] F-projectiles          Projectile physics system (arrows)
[~] F-rts-selection        RTS box selection and multi-creature commands
```

### Todo

```
[ ] F-adventure-mode       Control individual elf (RPG-like)
[ ] F-ai-sprites           AI-generated sprite art pipeline
[ ] F-apprentice           Skill transfer via proximity
[ ] F-armor                Wearable armor system
[ ] F-arrow-durability     Arrow durability and recovery
[ ] F-attack-move          Attack-move task (walk + fight en route)
[ ] F-audio-sampled        Sampled vocal syllables from conlang
[ ] F-audio-vocal          Continuous vocal synthesis
[ ] F-batch-blueprint      Batch blueprinting with dependency order
[ ] F-binding-conflicts    Binding conflict detection
[ ] F-bldg-concert         Concert hall
[ ] F-bldg-dining          Dining hall
[ ] F-bldg-storehouse      Storehouse (item storage)
[ ] F-bldg-transparency    Toggle building roof/wall transparency to see inside
[ ] F-bldg-workshop        Craftself's workshop
[ ] F-blueprint-mode       Layer-based blueprint selection UI
[ ] F-branch-growth        Grow branches for photosynthesis/fruit
[ ] F-bridges              Bridge construction between tree parts
[ ] F-build-queue-ui       Construction queue/progress UI
[ ] F-building-door        Player-controlled building door orientation
[ ] F-cascade-fail         Cascading structural failure
[ ] F-choir-build          Choir-based construction singing
[ ] F-choir-harmony        Ensemble harmony in construction singing
[ ] F-civ-knowledge        Civilization knowledge system (fruit tiers, discovery)
[ ] F-clothing             Wearable clothing system
[ ] F-combat               Combat and invader threat system
[ ] F-controls-config      Centralized controls config with rebinding and persistence
[ ] F-controls-config-A    ControlsConfig autoload and handler migration
[ ] F-controls-config-B    Controls persistence and sensitivity settings
[ ] F-controls-config-C    Controls settings screen with rebinding UI
[ ] F-crafting             Non-construction jobs and crafting
[ ] F-creature-death       Basic creature death (starvation)
[ ] F-cultural-drift       Inter-tree cultural divergence
[ ] F-day-night            Day/night cycle and pacing
[ ] F-defense-struct       Defensive structures (ballista, wards)
[ ] F-demolish             Structure demolition
[ ] F-elf-assign           Elf-to-building assignment UI
[ ] F-elf-leave            Devastated elves permanently leave
[ ] F-elf-weapons          Bows, spears, clubs for elf combat
[ ] F-elfcyclopedia-know   Elfcyclopedia civ/fruit knowledge pages
[ ] F-elfcyclopedia-srv    Embedded localhost HTTP elfcyclopedia server
[ ] F-emotions             Multi-dimensional emotional state
[ ] F-engagement-style     Unified engagement style (species + military group combat tactics)
[ ] F-fire-advanced        Heat accumulation and ignition thresholds
[ ] F-fire-basic           Fire spread and voxel destruction
[ ] F-fire-ecology         Fire as ecological force, firefighting
[ ] F-fire-structure       Fire x structural integrity cascades
[ ] F-flying-nav           3D flight navigation system
[ ] F-fog-of-war           Visibility via tree and root network
[ ] F-food-chain           Food production/distribution pipeline
[ ] F-fruit-prod           Basic fruit production and harvesting
[ ] F-fruit-sprite-ui      Fruit sprites in inventory/logistics/selection UI
[ ] F-hedonic-adapt        Asymmetric hedonic adaptation
[ ] F-instinctual-flee     Instinctual flee thresholds (species-level fear overrides)
[ ] F-jobs                 Elf job/role specialization
[ ] F-lod-sprites          LOD sprites (chibi / detailed)
[ ] F-magic-items          Magic item personalities and crafting
[ ] F-mana-mood            Mana generation tied to elf mood
[ ] F-mana-system          Mana generation, storage, and spending
[ ] F-mass-conserve        Wood mass tracking and conservation
[ ] F-military-armor       Military group armor policy
[ ] F-military-campaign    Send elves on world expeditions
[ ] F-military-equip       Military group equipment acquisition
[ ] F-military-groups      Military group data model and configuration
[ ] F-military-org         Squad management and organization
[ ] F-minimap              Minimap with tree silhouette and creature positions
[ ] F-modding              Scripting layer for modding support
[ ] F-modifier-keybinds    Modifier key combinations in bindings
[ ] F-mp-chat              Multiplayer in-game chat
[ ] F-mp-reconnect         Multiplayer reconnection after disconnect
[ ] F-multi-tree           NPC trees with personalities
[ ] F-narrative-log        Events and narrative log
[ ] F-partial-struct       Structural checks on incomplete builds
[ ] F-personality          Personality axes affecting behavior
[ ] F-poetry-reading       Social gatherings and poetry readings
[ ] F-population           Natural population growth/immigration
[ ] F-proc-poetry          Procedural poetry via simulated annealing
[ ] F-root-network         Root network expansion and diplomacy
[ ] F-rope-retract         Retractable rope ladders (furl/unfurl)
[ ] F-rust-mesh-complex    Rust mesh gen for buildings/ladders
[ ] F-rust-sprites         Investigate moving sprite generation to Rust
[ ] F-seasons              Seasonal visual and gameplay effects
[ ] F-social-graph         Relationships and social contagion
[ ] F-soul-mech            Death, soul passage, resurrection
[ ] F-sound-effects        Basic ambient and action sound effects
[ ] F-stairs               Stairs and ramps for vertical movement
[ ] F-stress-heatmap       Stress visualization in blueprint mode
[ ] F-struct-upgrade       Structure expansion/upgrade
[ ] F-tab-change-track     Change tracking (insert/update/delete diffs)
[ ] F-tab-joins            Join iterators across tables
[ ] F-tab-parent-pk        Tabulosity: allow parent PK as child table PK for 1:1 relations
[ ] F-tab-schema-evol      Schema evolution: custom migrations
[ ] F-task-assign-opt      Event-driven bidirectional task assignment
[ ] F-task-priority        Priority queue and auto-assignment
[ ] F-tree-capacity        Per-tree carrying capacity limits
[ ] F-tree-memory          Ancient tree knowledge/vision system
[ ] F-tree-species         Multiple tree species with properties
[ ] F-undo-designate       Undo last construction designation
[ ] F-unfurnish            Unfurnish/refurnish a building
[ ] F-vaelith-expand       Expand Vaelith language for runtime use
[ ] F-visual-smooth        Smooth voxel surface rendering
[ ] F-voxel-exclusion      Creatures cannot enter voxels occupied by hostile creatures
[ ] F-weather              Weather within seasons
[ ] F-wireframe-ghost      Wireframe ghost for overlap preview
[ ] F-world-boundary       World boundary visualization
[ ] F-worldgen-framework   Worldgen generator framework
[ ] F-zlevel-vis           Z-level visibility (cutaway/toggle)
```

### Done

```
[x] B-dead-node-panic      Panic on dead nav node in pathfinding
[x] B-dirt-not-pinned      Dirt unpinned in fast structural validator
[x] B-preview-blueprints   Preview treats blueprints as complete
[x] B-tab-serde-tests      Fix tabulosity test compilation under feature unification
[x] F-attack-task          AttackCreature task (player-directed target pursuit)
[x] F-audio-synth          Waveform synthesis for audio rendering
[x] F-bldg-dormitory       Dormitory (unassigned elf sleep)
[x] F-bldg-home            Home (single elf dwelling)
[x] F-bldg-kitchen         Kitchen (cooking from ingredients)
[x] F-bldg-workshop        Craftself's workshop
[x] F-bread                Bread items and elf food management
[x] F-building             Building construction (paper-thin walls)
[x] F-cam-follow           Camera follow mode for creatures
[x] F-capybara             Capybara species
[x] F-carve-holes          Remove material (doors, storage hollows)
[x] F-civilizations        Procedural civilization generation and diplomacy
[x] F-construction         Platform construction (designate/build/cancel)
[x] F-core-types           VoxelCoord, IDs, SimCommand, GameConfig
[x] F-crate-structure      Two-crate sim/gdext structure
[x] F-creature-actions     Creature action system: typed duration-bearing actions
[x] F-creature-info        Creature info panel with follow button
[x] F-creature-tooltip     Hover tooltips for world objects
[x] F-debug-menu           Move spawn/summon into debug menu
[x] F-dynamic-pursuit      Dynamic repathfinding for moving-target tasks
[x] F-elf-acquire          Elf personal item acquisition
[x] F-elf-names            Elf name generation from conlang rules
[x] F-elf-needs            Hunger and rest self-direction
[x] F-elf-sprite           Billboard elf sprite rendering
[x] F-elfcyclopedia-srv    Embedded localhost HTTP elfcyclopedia server
[x] F-emotions-basic       Mood score from thought weights
[x] F-event-loop           Event-driven tick loop (priority queue)
[x] F-flee                 Flee behavior for civilians
[x] F-food-gauge           Creature food gauge with decay
[x] F-fruit-naming         Fruit naming overhaul
[x] F-fruit-sprites        Procedural fruit sprites
[x] F-fruit-yields         Fruit yield model overhaul
[x] F-furnish              Building furnishing framework (dormitories)
[x] F-game-session         Game session autoload singleton
[x] F-gdext-bridge         gdext compilation and Rust bridge
[x] F-godot-setup          Godot 4 project setup
[x] F-hauling              Item hauling task type
[x] F-hilly-terrain        Hilly forest floor with dirt voxels
[x] F-hostile-detection    Hostile detection and faction logic
[x] F-hostile-species      Goblin, Orc, and Troll species
[x] F-hp-death             HP, VitalStatus, and creature death handling
[x] F-hp-ui                HP bars in creature UI
[x] F-items                Items and inventory system
[x] F-keybind-help         Keyboard shortcuts help overlay
[x] F-ladders              Rope/wood ladders as cheap connectors
[x] F-lang-crate           Shared Vaelith language crate
[x] F-large-nav-tolerance  1-voxel height tolerance for large nav
[x] F-large-pathfind       2x2 footprint nav grid
[x] F-logistics            Spatial resource flow (Kanban-style)
[x] F-logistics-filter     Logistics material filter
[x] F-main-menu            Main menu UI
[x] F-manufacturing        Item schema expansion + workshop manufacturing
[x] F-melee-action         Melee attack action
[x] F-mood-system          Mood with escalating consequences
[x] F-move-interp          Smooth creature movement interpolation
[x] F-mp-checksums         Multiplayer state checksums for desync detection
[x] F-mp-integ-test        Multiplayer integration test harness
[x] F-mp-mid-join          Mid-game join with state snapshot
[x] F-music-gen            Palestrina-style music generator (standalone)
[x] F-music-runtime        Integrate music generator into game
[x] F-music-use-lang       Migrate music crate to shared lang crate
[x] F-nav-graph            Navigation graph construction
[x] F-nav-incremental      Incremental nav graph updates
[x] F-new-game-ui          New game screen with tree presets
[x] F-orbital-cam          Orbital camera controller
[x] F-pathfinding          A* pathfinding over nav graph
[x] F-pause-menu           In-game pause overlay
[x] F-pile-gravity         Ground pile gravity and merging
[x] F-placement-ui         Revamp construction placement UX
[x] F-preemption           Task priority and preemption system
[x] F-projectiles          Projectile physics system (arrows)
[x] F-recipes              Recipe system for crafting/cooking
[x] F-rust-mesh-gen        Rust-side voxel mesh gen with face culling
[x] F-save-load            Save/load to JSON with versioning
[x] F-select-struct        Selectable structures with interaction UI
[x] F-selection            Click-to-select creatures
[x] F-serde                Serialization for all sim types
[x] F-session-sm           Formal session & sim state machines
[x] F-shared-prng          Shared PRNG crate across all Rust crates
[x] F-shoot-action         Ranged attack action (shooting arrows)
[x] F-sim-commands         SimCommand pipeline
[x] F-sim-db-impl          Tabulosity typed in-memory relational store
[x] F-sim-speed            Simulation speed controls UI
[x] F-sim-tab-migrate      Migrate sim entity storage to tabulosity SimDb
[x] F-spatial-index        Creature spatial index for voxel-level position queries
[x] F-spawn-toolbar        Spawn toolbar and placement UI
[x] F-status-bar           Persistent status bar (population, idle count, active tasks)
[x] F-struct-basic         Basic structural integrity (flood fill)
[x] F-struct-names         User-editable structure names
[x] F-structure-reg        Completed structure registry + UI panel
[x] F-tab-auto-pk          Auto-generated primary keys
[x] F-tab-cascade-del      Cascade/nullify on delete
[x] F-tab-compound-idx     Compound indexes with prefix queries
[x] F-tab-filter-idx       Filtered/partial indexes
[x] F-tab-modify-unchk     Closure-based row mutation (modify_unchecked)
[x] F-tab-query-opts       Query options struct for index queries
[x] F-tab-schema-ver       Schema versioning fundamentals
[x] F-tab-unique-idx       Unique index enforcement
[x] F-task-interruption    Unified task interruption and cleanup
[x] F-task-panel-groups    Task panel grouped by origin + creature names
[x] F-task-proximity       Proximity-based task assignment (Dijkstra nearest)
[x] F-thoughts             Creature thoughts (DF-style event reactions)
[x] F-tree-gen             Procedural tree generation (trunk+branches)
[x] F-tree-info            Tree stats/info panel
[x] F-tree-overlap         Construction overlap with tree geometry
[x] F-voxel-fem            Voxel FEM structural analysis
[x] F-voxel-textures       Per-face Perlin noise voxel textures
[x] F-worldgen-framework   Worldgen generator framework
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

#### F-bldg-concert — Concert hall
**Status:** Todo · **Phase:** 4

Furnished building where elves gather for musical performances. Exact
mechanics uncertain — may involve assigned musician elves, scheduled
performances, audience satisfaction, or ties to the music system.
Details to be worked out in a design doc.

**Related:** F-bldg-dining, F-music-runtime

#### F-bldg-dining — Dining hall
**Status:** Todo · **Phase:** 4

Communal dining building where elves eat together. Provides a social
eating bonus compared to eating alone.

**Related:** F-bldg-concert, F-bldg-kitchen, F-food-chain

#### F-bldg-dormitory — Dormitory (unassigned elf sleep)
**Status:** Done · **Phase:** 3

Communal sleeping building for elves without assigned homes. Dormitories are
built as Building type, then furnished with beds. Tired elves autonomously
find the nearest unoccupied bed via Dijkstra pathfinding and sleep to restore
rest. If no beds are available, elves fall back to sleeping on the ground.

**Related:** F-bldg-home, F-elf-needs, F-furnish

#### F-bldg-home — Home (single elf dwelling)
**Status:** Done · **Phase:** 3

Personal dwelling for a single elf (families in the future). The player
assigns which elf lives in each home. Provides rest and comfort need
satisfaction.

**Related:** F-bldg-dormitory, F-elf-assign, F-elf-needs

#### F-bldg-kitchen — Kitchen (cooking from ingredients)
**Status:** Done · **Phase:** 4

Building where elves convert raw ingredients into processed foods (e.g.,
one large fruit into many shelf-stable breads). Kitchens receive fruit
via logistics and cook it into bread. Cooking is controlled via the
structure info panel (enable/disable, bread target). **Draft:**
`docs/drafts/kitchen_cooking.md`

**Related:** F-bldg-dining, F-bread, F-elf-acquire, F-elf-assign, F-food-chain, F-fruit-variety, F-jobs, F-manufacturing, F-recipes

#### F-bldg-storehouse — Storehouse (item storage)
**Status:** Todo · **Phase:** 4

Building for storing items and resources. Items placed inside persist
and are accessible to elves for retrieval.

**Related:** F-food-chain, F-logistics

#### F-bldg-workshop — Craftself's workshop
**Status:** Done · **Phase:** 4

Workshop where craftself elves create tools and equipment (bows, spears,
and other gear).

**Related:** F-crafting, F-elf-assign, F-elf-weapons, F-jobs, F-recipes

#### F-blueprint-mode — Layer-based blueprint selection UI
**Status:** Todo · **Phase:** 2 · **Refs:** §12

Full blueprint mode with layer-based (Y-level) selection, ghost previews for
arbitrary shapes, and structural warnings. Currently only rectangular platform
designation exists via `construction_controller.gd`. This item covers the
general-purpose blueprint UI that supports all build types and freeform shapes.

**Related:** F-batch-blueprint, F-construction, F-placement-ui, F-stress-heatmap, F-tree-overlap, F-wireframe-ghost

#### F-branch-growth — Grow branches for photosynthesis/fruit
**Status:** Todo · **Phase:** 3 · **Refs:** §8, §13

Player-directed branch/bough growth to extend the tree for more
photosynthesis capacity and fruit production. Uses the existing tree
generation algorithm with player-chosen growth direction.

**Related:** F-fruit-prod, F-mana-system, F-mass-conserve, F-tree-species

#### F-bridges — Bridge construction between tree parts
**Status:** Todo · **Phase:** 3 · **Refs:** §11

Bridges and walkways connecting different parts of the tree. Requires new
build type UI for specifying start/end anchor points and path.

**Related:** F-struct-basic, F-tree-overlap

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

**Related:** F-construction, F-furnish, F-placement-ui, F-structure-reg

#### F-building-door — Player-controlled building door orientation
**Status:** Todo · **Phase:** 2

**Related:** F-placement-ui

#### F-carve-holes — Remove material (doors, storage hollows)
**Status:** Done · **Phase:** 3 · **Refs:** §11

Remove material from existing tree or construction geometry to create
doorways, windows, storage hollows. The inverse of construction.
Implemented as a third build mode (Carve) in the construction panel
with structural integrity validation (blocks disconnecting carves,
warns on stress). Supports rectangular prism carving with width/depth/height.

**Related:** F-demolish

#### F-choir-build — Choir-based construction singing
**Status:** Todo · **Phase:** 2 · **Refs:** §11, §21

Elves assemble into choirs to sing the tree into growing. Construction speed
and quality depend on choir composition and harmony. Ties into the music
system.

**Related:** F-choir-harmony, F-mana-system, F-music-runtime

#### F-construction — Platform construction (designate/build/cancel)
**Status:** Done · **Phase:** 2 · **Refs:** §11, §12

Basic construction loop: player designates rectangular platforms via the
construction controller UI, sim validates (all voxels Air, at least one
adjacent to solid), creates a blueprint + Build task, elves claim the task
and incrementally materialize voxels. Cancellation reverts placed voxels.
Incremental nav graph updates keep pathfinding current during construction.

**Related:** F-blueprint-mode, F-build-queue-ui, F-building, F-demolish, F-placement-ui, F-struct-upgrade, F-structure-reg, F-tree-overlap, F-undo-designate

#### F-demolish — Structure demolition
**Status:** Todo · **Phase:** 3

Player selects a completed structure and orders it demolished. Elves
perform the demolition as a task, reverting the structure's voxels to Air
and removing it from the structure registry. Nav graph updates
incrementally as voxels are removed. Cancel-build already handles
reverting incomplete structures; this covers intentional teardown of
finished ones. Needs to consider structural consequences — demolishing a
load-bearing structure could affect structures above it (warn or block).

**Related:** F-carve-holes, F-cascade-fail, F-construction, F-select-struct, F-struct-upgrade, F-unfurnish

#### F-furnish — Building furnishing framework (dormitories)
**Status:** Done · **Phase:** 3 · **Refs:** §11

Framework for assigning purpose to generic building shells. Buildings
start as empty enclosed spaces (from F-building) and are furnished to
become specific building types. An elf is dispatched to furnish the
building, placing furniture one at a time as a `Furnish` task. Supports
all 7 furnishing types: Concert Hall (benches), Dining Hall (tables),
Dormitory (beds), Home (single bed), Kitchen (counters), Storehouse
(shelves), Workshop (workbenches). Each type has its own placement
density and visually distinct MultiMesh rendering (per-kind box size and
color via `furniture_renderer.gd`). Furniture count is proportional to
floor area (density varies by type). Auto-renames the building to e.g.
"Dormitory #N" unless the player has set a custom name.

**New files:** `furniture_renderer.gd`

**Related:** F-bldg-dormitory, F-building, F-unfurnish

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

**Related:** F-branch-growth, F-mana-system

#### F-placement-ui — Revamp construction placement UX
**Status:** Done · **Phase:** 2

Revamp the construction placement UI with mode-specific interaction models
and a five-state state machine (INACTIVE → ACTIVE → HOVER → DRAGGING →
PREVIEW). Four placement modes:

- **Platforms:** height-slice wireframe grid at camera Y-level, click-drag
  rectangle on horizontal plane, structural integrity preview.
- **Buildings:** surface raycast to solid ground, click-drag footprint (min
  3x3, flat terrain only), height +/- in preview.
- **Ladders:** surface raycast for start/end, vertical 1x1 column, auto
  orientation, wood/rope toggle.
- **Carve:** height-slice grid, click-drag rectangle + camera height sweep
  for 3D prism, height +/- in preview.

All modes: hover highlight, confirm via Enter/button only (no left-click
confirm), cancel via Escape/button, structural integrity warnings.
Bridges and stairs deferred.

**Draft:** `docs/drafts/placement_ui.md`

**Related:** F-blueprint-mode, F-building, F-building-door, F-construction

#### F-rope-retract — Retractable rope ladders (furl/unfurl)
**Status:** Todo · **Phase:** 3 · **Refs:** §11

Rope ladders can be furled (retracted) and unfurled by elves as a task/job.
Furled ladders are impassable and visually show their rolled-up state.
Player clicks a rope ladder (via F-select-struct) to see its furled/unfurled
status and request a state change. The structure's selection UI should
display any ongoing or queued furling/unfurling tasks.

**Related:** F-ladders, F-select-struct

#### F-stairs — Stairs and ramps for vertical movement
**Status:** Todo · **Phase:** 3 · **Refs:** §11

Stairs and ramps for connecting vertical levels. Requires nav graph edges
with appropriate movement cost (climb speed vs walk speed).

**Related:** F-struct-basic, F-tree-overlap

#### F-struct-names — User-editable structure names
**Status:** Done · **Phase:** 3

Structures get user-editable names. Default names are bland sequential
labels (e.g., "Building #5", "Platform #12"). Each structure tracks
whether its name was set by the player or is still the auto-generated
default. Player can rename via the structure info panel. Renamed
structures show their custom name in the structure list and info panel.

**Related:** F-select-struct, F-struct-upgrade, F-structure-reg

#### F-struct-upgrade — Structure expansion/upgrade
**Status:** Todo · **Phase:** 4

Expand or upgrade existing structures in place — e.g., extend a 3x3
building to 5x5, upgrade a platform's material, add a second story.
Distinct from demolish-and-rebuild: preserves structure identity, name,
and assignments. Requires structural validation of the expanded footprint.

**Related:** F-construction, F-demolish, F-struct-names

#### F-task-priority — Priority queue and auto-assignment
**Status:** Todo · **Phase:** 2 · **Refs:** §11, §15

Task queue with Low/Normal/High/Urgent priorities, auto-assignment of idle
elves to highest-priority available tasks. Priority is already in the data
model but not yet used for scheduling.

**Blocks:** F-task-assign-opt
**Related:** F-build-queue-ui, F-elf-needs, F-jobs, F-preemption, F-task-assign-opt

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

**Related:** F-blueprint-mode, F-bridges, F-construction, F-stairs, F-wireframe-ghost

#### F-undo-designate — Undo last construction designation
**Status:** Todo · **Phase:** 2

Undo the most recent construction designation (Ctrl+Z or similar). Currently
players can cancel in-progress builds, but a misclicked designation requires
manually selecting and cancelling. A simple undo stack (last-in-first-out)
for designations would prevent frustration from placement mistakes.

**Related:** F-construction

#### F-unfurnish — Unfurnish/refurnish a building
**Status:** Todo · **Phase:** 3

Remove a building's furnishing, reverting it to an empty shell. Also
enables refurnishing — changing a dormitory into a workshop, for example.
Should remove placed furniture objects (beds, etc.) and reset the
building's furnishing type. May require an Unfurnish task where an elf
walks to the building and removes furniture incrementally.

**Related:** F-demolish, F-furnish

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

**Related:** F-bldg-transparency, F-blueprint-mode, F-tree-overlap

### Structural Integrity & Fire

#### B-dead-node-panic — Panic on dead nav node in pathfinding
**Status:** Done

Creature pathfinding panics (`unwrap()` on `None`) when a task's `location`
nav node has been removed by an incremental nav graph update (e.g.
construction solidifying a voxel). Fix: guard `execute_task_behavior` and
`process_creature_activation` to check node liveness before pathfinding,
resnapping or abandoning the task if the node is dead.

#### B-dirt-not-pinned — Dirt unpinned in fast structural validator
**Status:** Done

`build_network_from_set()` (used by `validate_blueprint_fast()`) only pins
`ForestFloor` voxels, not `Dirt`. Since Dirt has density 999, unpinned Dirt
acts as massive dead weight in the weight-flow analysis, causing all
structures near hilly terrain to fail validation. One-line fix: add
`|| vt == VoxelType::Dirt` to match the full solver's pinning logic.

#### B-preview-blueprints — Preview treats blueprints as complete
**Status:** Done · **Phase:** 2

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

**Blocks:** F-fire-structure
**Related:** F-demolish

#### F-fire-advanced — Heat accumulation and ignition thresholds
**Status:** Todo · **Phase:** 5 · **Refs:** §16

Fire Stage 2: heat accumulation model, per-material ignition thresholds,
green wood vs dry wood distinction.

**Blocked by:** F-fire-basic
**Blocks:** F-fire-ecology

#### F-fire-basic — Fire spread and voxel destruction
**Status:** Todo · **Phase:** 5 · **Refs:** §16

Fire simulation Stage 1: basic probabilistic spread between adjacent
flammable voxels, voxel destruction when fully burned.

**Blocks:** F-fire-advanced, F-fire-structure

#### F-fire-ecology — Fire as ecological force, firefighting
**Status:** Todo · **Phase:** 7 · **Refs:** §16

Fire Stages 3-4: environmental factors (wind, rain), organized
firefighting by elves, fire as an ecological renewal force.

**Blocked by:** F-fire-advanced
**Related:** F-weather

#### F-fire-structure — Fire x structural integrity cascades
**Status:** Todo · **Phase:** 5 · **Refs:** §9, §16
**Draft:** `docs/drafts/structural_integrity.md` §11

Burning supports trigger structural collapse cascades. Performance concern:
fire destroying load-bearing voxels triggers spring-mass solver
recalculation during an already-expensive fire tick (§27). Tree voxels have
very high but finite strength (draft §6), so fire can theoretically bring
down branches.

**Blocked by:** F-cascade-fail, F-fire-basic

#### F-partial-struct — Structural checks on incomplete builds
**Status:** Todo · **Phase:** 8+ · **Refs:** §9
**Draft:** `docs/drafts/structural_integrity.md` §12.3

Detect and handle structurally unsound partial construction — e.g., a player
designates a structurally sound arch, then cancels mid-construction leaving
an unsound cantilever remnant. Possible mitigations: structural check on
cancellation, periodic structural heartbeat for incomplete structures, or
limits on how far construction can extend from support before the next
anchor is in place.

#### F-stress-heatmap — Stress visualization in blueprint mode
**Status:** Todo · **Phase:** 5 · **Refs:** §9, §12
**Draft:** `docs/drafts/structural_integrity.md` §7, §14-F

Overlay showing per-voxel stress levels during blueprint planning. Color-map
from green (safe) through yellow (moderate) to red (failure). Uses reduced
solver iterations (~20–30) for responsive preview during placement, full
iterations on confirm. See draft §7.2 for `BlueprintValidation` data
structure and §7.4 for performance budget.

**Related:** F-blueprint-mode

#### F-struct-basic — Basic structural integrity (flood fill)
**Status:** Done · **Phase:** 3 · **Refs:** §9
**Draft:** `docs/drafts/structural_integrity.md` §8

Connectivity flood fill: can every solid voxel reach a grounded voxel
(ForestFloor or trunk-to-ground) via face-adjacent solid voxels? Disconnected
clusters are flagged. Used as a fast pre-filter in F-voxel-fem blueprint
validation (draft §7.3). The `flood_fill_connected()` function is shared
between this feature and the FEM system. Implemented as part of F-voxel-fem
in `structural.rs`.

**Related:** F-batch-blueprint, F-bridges, F-stairs, F-voxel-fem

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

**Related:** B-preview-blueprints, F-struct-basic

#### F-voxel-textures — Per-face Perlin noise voxel textures
**Status:** Done · **Phase:** 2

Per-face procedural textures for bark and ground voxels using 3D Perlin noise
sampled at world coordinates. Each visible face gets a 16×16 texture tile
packed into a per-chunk atlas. The 3D noise ensures seamless edges between
adjacent faces regardless of orientation. Bark noise uses anisotropic scaling
(vertical grain) and domain warping (organic wobble). Ground uses isotropic
fractal noise. Leaf voxels retain their shared alpha-scissor texture.

Key files: `texture_gen.rs` (Perlin noise + atlas generation), `mesh_gen.rs`
(atlas UV computation, bark/ground surface split), `sim_bridge.rs` (atlas
data passing to Godot), `tree_renderer.gd` (per-chunk material creation).

### Navigation & Pathfinding

#### F-dynamic-pursuit — Dynamic repathfinding for moving-target tasks
**Status:** Done

The current task system assumes every task has a static `location: NavNodeId`.
When a creature claims a task, `walk_toward_task()` computes an A* path once
and the creature walks it step by step. If the path is exhausted or absent, a
new path is computed to the same fixed location. This works for all existing
task kinds (Build, Haul, Cook, Sleep, etc.) because their destinations don't
move.

Combat introduces tasks whose destination is another creature — AttackTarget
needs to pathfind toward a moving target. This requires changes to the
task/movement infrastructure.

**Task base table stays unchanged.** `location` remains `NavNodeId` (not
optional). For AttackTarget, `location` is set to the target's position at
task creation time and updated when repathfinding. Target creature references
live in extension tables (`TaskAttackTargetData.target`,
`TaskAttackMoveData.current_target`), preserving the decomposition pattern.

**Dynamic repathfinding in `walk_toward_task()`:**
- When a task tracks a moving target (via its extension table), the cached
  `CreaturePath` becomes stale as the target moves.
- Repathfinding policy: repath when the target has moved beyond some threshold
  distance from the path's original goal, or when the cached path is exhausted.
  Full repath every activation is too expensive for large nav graphs.
- On repath, update `task.location` to the target's current position so
  UI/rendering code that reads `task.location` stays correct.
- Possible heuristic: store the `NavNodeId` the path was computed toward; on
  each activation, compare to target's current node. If different, repath.
  Could also add a tick-based cooldown (repath at most once every N ticks).

**Path invalidation and edge cases:**
- Target dies mid-pursuit: `target`'s vital status must be polled each
  activation. If dead (or row missing from future cleanup), task completes.
- Target becomes unreachable (e.g., enters a disconnected nav region): must
  handle gracefully — abandon pursuit after failed pathfinding, not panic.
- Target is already adjacent: skip pathfinding, proceed to combat actions.

**Integration with activation chain:**
- `execute_task_behavior()` currently reads `task.location` unconditionally
  and passes it to `walk_toward_task()`. For pursuit tasks, the activation
  logic must check path validity against the target's current position before
  walking.
- Existing task kinds must continue to work unchanged — this is a backward-
  compatible extension, not a rewrite.

**Design questions to resolve during implementation:**
- Repathfinding frequency vs. cost tradeoff (every activation? every N ticks?
  only when target node changes?).
- Whether `walk_toward_task()` should accept a `NavNodeId` (already resolved)
  or do the resolution itself.
- How AttackMove (which has both a fixed destination AND an optional moving
  target) interacts with this system.

Identified during combat design review (docs/drafts/combat_military.md §5).
This is a prerequisite for combat task kinds but is a separable infrastructure
change that benefits from independent implementation and testing.

**Related:** F-creature-actions, F-task-interruption

#### F-flying-nav — 3D flight navigation system
**Status:** Todo · **Phase:** 8+ · **Refs:** §10

Full 3D movement for birds and winged elves. Separate from the
surface-based nav graph — likely a volumetric approach.

#### F-large-nav-tolerance — 1-voxel height tolerance for large nav
**Status:** Done · **Phase:** 8+

Allow up to 1 voxel of height variation within a large creature's 2x2
footprint and between adjacent large nav nodes. Fixes elephant navigation
on hilly terrain (F-hilly-terrain) where variable-height dirt voxels
fragmented the large nav graph.

#### F-large-pathfind — 2x2 footprint nav grid
**Status:** Done · **Phase:** 8+ · **Refs:** §10

Separate pre-baked `NavGraph` for 2x2x2 footprint creatures (elephants).
Nodes only where a 2x2x2 volume is clear and all 4 ground cells are solid.
Includes `Species::Elephant`, `graph_for_species()` dispatch, incremental
updates, SimBridge queries, GDScript spawn/render/placement, and sprite.

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

#### F-bread — Bread items and elf food management
**Status:** Done · **Phase:** 3

Elves carry bread as a portable food source. Each elf starts with a varying
amount of bread. Bread is an item in the items system — elves carry it, it
can be dropped, and kitchens produce it from fruit. Eating bread adds to
the existing food gauge (the gauge remains as the creature's internal
hunger/satiation state; bread is the concrete item that fills it).

**Related:** F-bldg-kitchen, F-elf-acquire, F-elf-needs, F-food-chain, F-food-gauge, F-manufacturing

#### F-capybara — Capybara species
**Status:** Done · **Refs:** §15

Capybara species with ground-only movement restriction, own sprite renderer,
and species-specific speed config.

#### F-creature-actions — Creature action system: typed duration-bearing actions
**Status:** Done

Formalize creature actions as first-class typed, duration-bearing operations.
Every creature activity (move, build, eat, sleep, etc.) is an explicit Action
with a kind, duration, and completion effect. Shared action state (ActionKind +
next_available_tick) lives inline on the Creature row. MoveAction detail table
stores render interpolation data (moved from old Creature move_* fields).

**Implementation status:** Core action system complete — all 13 ActionKind
variants implemented (NoAction, Move, Build, Furnish, Cook, Craft, Sleep, Eat,
Harvest, AcquireItem, PickUp, DropOff, Mope). All do_* functions converted to
start/resolve action pairs. New config fields for per-action durations.
Remaining: additional test coverage audit per design doc.

**Draft:** docs/drafts/creature_actions.md

**Related:** F-dynamic-pursuit, F-preemption, F-task-interruption

#### F-creature-death — Basic creature death (starvation)
**Status:** Todo · **Phase:** 3 · **Refs:** §13, §15

When a creature's food gauge reaches zero, it dies (vital_status → Dead,
creature row kept in DB). Basic death mechanic without the spiritual
dimension (soul passage, resurrection) covered by F-soul-mech. Needs:
starvation trigger at food=0, death via F-hp-death handler, UI notification.
A prerequisite for food scarcity having real consequences.

Superseded by F-hp-death which covers the general death system including
combat death. F-creature-death covers the starvation trigger specifically.

**Related:** F-elf-needs, F-food-gauge, F-hp-death, F-soul-mech

#### F-elf-assign — Elf-to-building assignment UI
**Status:** Todo · **Phase:** 3

Reusable UI for assigning elves to buildings. Click a building → see a
list of elves → assign one (or more, depending on building type). Click
an elf → see their current assignment. Used by homes (which elf lives
here), kitchens (which elf is the cook), workshops (which elf is the
craftself), etc. A shared pattern rather than reimplemented per building
type.

**Related:** F-bldg-home, F-bldg-kitchen, F-bldg-workshop, F-jobs, F-select-struct

#### F-elf-leave — Devastated elves permanently leave
**Status:** Todo · **Phase:** 4 · **Refs:** §18

When an elf has been deeply unhappy for a sustained period, they permanently
leave the tree. Requires both short-term mood (from F-mood-system) and a
longer-term "resentment" accumulator (from multi-dimensional emotions) to be
critically low — brief bad days don't trigger departure. The elf gets a
`TaskKind::Leave`, pathfinds to the map edge, and despawns. Inventory drops
to the ground, assigned home becomes unassigned. Requires a visible player
notification so the event isn't missed.

**Blocked by:** F-emotions, F-notifications

#### F-elf-needs — Hunger and rest self-direction
**Status:** Done · **Phase:** 3 · **Refs:** §13, §15

Elves autonomously seek food (eat fruit from trees) and rest (find sleeping
spots) when needs are low. Self-directed behavior that interrupts idle
wandering when needs are critical. Hunger takes priority over tiredness.

**Hunger:** Idle creatures with food below `food_hunger_threshold_pct`
(default 50%) get an `EatFruit` task created at heartbeat time, pathfind to
the nearest fruit voxel, eat it (restoring `food_restore_pct`% of food_max),
and remove the fruit from the world.

**Rest/sleep:** Idle creatures with rest below `rest_tired_threshold_pct`
(default 50%) get a `Sleep` task. The heartbeat finds the nearest unoccupied
dormitory bed (via Dijkstra) or falls back to ground sleep. Sleep is a
multi-activation task: each activation restores `rest_per_sleep_tick` rest,
completing when progress reaches `total_cost` (bed: `sleep_ticks_bed`, ground:
`sleep_ticks_ground`) or rest reaches `rest_max`. Rest gauge and food gauge
are both shown in the creature info panel.

**Related:** F-bldg-dormitory, F-bldg-home, F-bread, F-creature-death, F-food-gauge, F-fruit-prod, F-task-priority

#### F-elf-sprite — Billboard elf sprite rendering
**Status:** Done · **Phase:** 1 · **Refs:** §24

Billboard chibi elf sprites using pool pattern. Procedurally generated from
seed via `sprite_factory.gd`. Offset +0.48 Y for visual centering.

#### F-food-gauge — Creature food gauge with decay
**Status:** Done · **Refs:** §13

Food level per creature, decaying over time. Displayed in creature info
panel and as overhead bar.

**Related:** F-bread, F-creature-death, F-elf-needs, F-fruit-prod

#### F-hostile-species — Goblin, Orc, and Troll species
**Status:** Done

#### F-hp-death — HP, VitalStatus, and creature death handling
**Status:** Done

Add hp, hp_max, vital_status fields to Creature. VitalStatus enum (Alive, Dead, future: Ghost, SpiritInTree, Undead). hp_max in SpeciesData. Death transition: vital_status → Dead, creature row NOT deleted (supports future states). On death: call unified task interruption (F-task-interruption), drop inventory as ground pile, clear assigned_home, remove from spatial index, emit CreatureDied event, terminate activation/heartbeat chains (no rescheduling). All existing queries that iterate creatures must filter by vital_status == Alive (rendering, task assignment, logistics, heartbeat processing). #[indexed] on vital_status for efficient filtering. #[serde(default)] on new fields for save compat. Supersedes F-creature-death (which only covered starvation — this is the general death system). Debug "kill creature" command for testing.

**Draft:** docs/drafts/combat_military.md (§3)

**Related:** F-creature-death, F-hp-ui

#### F-melee-action — Melee attack action
**Status:** Done

Melee strike as a creature ACTION (not a task). Uses the standard ActionKind / next_available_tick mechanism for cooldown — set action_kind = MeleeStrike and next_available_tick = current_tick + melee_interval_ticks, same as every other duration-bearing action (no separate last_melee_tick field). Apply melee_damage (from SpeciesData) to target, emit CreatureDamaged event, trigger death if HP ≤ 0. New ActionKind variant: MeleeStrike. New SpeciesData fields: melee_damage, melee_interval_ticks, melee_range_sq.

**Melee range uses closest-point-of-footprint distance**, NOT nav-edge adjacency or anchor-to-anchor. For multi-voxel creatures, clamp target coords to attacker's footprint bounds and vice versa, then check squared distance ≤ melee_range_sq. Specifically: for a creature at anchor `pos` with footprint `[fx, fy, fz]`, closest point to a target coord is `(clamp(target.x, pos.x, pos.x + fx - 1), ...)`. Squared euclidean distance between closest points of both footprints must be ≤ melee_range_sq.

`melee_range_sq: i64` in SpeciesData (default 2). Covers face-adjacent offsets (dist²=1) and 2D diagonal like (1,1,0) (dist²=2). Intentionally excludes the pure 3D corner diagonal (1,1,1) (dist²=3) — 3D corner adjacency feels like too much reach for melee. Nav edges are for pathfinding, not melee range; this sidesteps the cross-graph problem for multi-voxel creatures entirely.

**Draft:** docs/drafts/combat_military.md (§5 "Melee Attack Action")


**Draft:** docs/drafts/combat_military.md (§5 "Melee Attack Action")

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

**Related:** F-fruit-prod, F-mana-system, F-tree-capacity

#### F-preemption — Task priority and preemption system
**Status:** Done

PreemptionLevel enum with explicit level() method (NOT derived Ord). 8 levels: Idle(0), Autonomous(1), PlayerDirected(2), Survival(3), Mood(4), AutonomousCombat(5), PlayerCombat(6), Flee(7). Computed from (TaskKindTag, TaskOrigin) → PreemptionLevel via preemption_level() — NOT stored on Task. Exhaustive match on both enums (no wildcard) so new variants cause compile errors. check_mope() refactored to use can_preempt() instead of ad-hoc mope_can_interrupt_task config flag.

**Preemption rules:**
- Standard rule: can preempt if new level > current level.
- PlayerDirected commands explicitly override AutonomousCombat (player is ultimate authority). Does NOT override PlayerCombat.
- Mope-Survival hardcoded exception: Mood never preempts Survival. Prevents death spiral.
- Same-level replacement: same level does NOT preempt by default. Exceptions: PlayerDirected replaces PlayerDirected, PlayerCombat replaces PlayerCombat.

**Config deprecation:** mope_can_interrupt_task field retained for serde backward compatibility but is no longer consulted by check_mope(). The preemption system fully supersedes it.

**Test coverage:** 22 unit tests in preemption.rs (level ordering, all mapping paths, all can_preempt rules/exceptions, serde roundtrip) + updated integration tests in sim.rs (mope preempts Build regardless of config flag, mope doesn't preempt Sleep/Mope).

**Draft:** docs/drafts/combat_military.md (§8)

**Related:** F-creature-actions, F-task-priority

#### F-shoot-action — Ranged attack action (shooting arrows)
**Status:** Done

Ranged attack as a creature ACTION. Uses the standard ActionKind / next_available_tick mechanism for cooldown — set action_kind = Shoot and next_available_tick = current_tick + shoot_cooldown_ticks (no separate last_shoot_tick field). Requires: LOS to target (voxel ray march / DDA, multi-voxel targets check any occupied voxel), range check (archer_range_sq in SpeciesData), ammo in inventory, cooldown elapsed. On shoot: compute aim velocity via iterative guess-and-simulate (same integer physics as real projectiles, max 5 iterations), consume arrow from inventory, spawn Projectile entity, emit ProjectileLaunched event. Aim skill tiers (novice/skilled/expert) for future. New ActionKind variant: Shoot. New config fields: shoot_cooldown_ticks in GameConfig, archer_range_sq in SpeciesData. LOS and aim computation are pure algorithms, unit-testable independently.

**Requires both arrows AND a bow in inventory.** No bow = no shooting, even with arrows. Bow presence is checked via inventory item_kind. This ties into the WeaponPolicy system (§1 of combat doc), which determines whether a creature should acquire and carry a bow. Creatures without WeaponPolicy allowing bows will never have one and thus never shoot.

**Draft:** docs/drafts/combat_military.md (§5)


**Draft:** docs/drafts/combat_military.md (§5)

#### F-task-interruption — Unified task interruption and cleanup
**Status:** Done

Task cleanup on interruption is currently scattered across five per-kind
functions (`cleanup_haul_task`, `cleanup_cook_task`, `cleanup_craft_task`,
`cleanup_harvest_task`, `cleanup_acquire_item_task`) called ad-hoc at each
interruption site. There is no single entry point that correctly handles all
task kinds, and the call sites are inconsistent — the mope preemption code
(sim.rs ~3546) calls four of the five cleanup functions but omits
`cleanup_craft_task`, meaning a creature interrupted from crafting by moping
will leak item reservations at the workshop.

**What needs to exist:** A single `interrupt_task(creature_id, task_id)` (or
similar) function that:

1. Dispatches to per-kind cleanup based on `TaskKindTag`.
2. Handles ALL 12 task kinds correctly:
   - **GoTo:** No cleanup needed. Cancel task.
   - **Build:** Resumable — return to `Available` for another creature.
   - **Furnish:** Resumable — return to `Available`.
   - **Haul:** Phase-dependent. GoingToSource: clear source reservations,
     cancel. GoingToDestination: drop carried items as ground pile at
     creature's current position, cancel.
   - **Cook:** Clear reservations at kitchen inventory. Cancel task.
   - **Craft:** Clear reservations at workshop inventory. Cancel task.
   - **Harvest:** Cancel task (no reservations to clear).
   - **EatBread:** Cancel task (personal, no reservations).
   - **EatFruit:** Cancel task (personal, no reservations).
   - **Sleep:** Cancel task (personal, no reservations).
   - **AcquireItem:** Clear reservations at source inventory. Cancel task.
   - **Mope:** Cancel task (personal, no reservations).
3. Clears the creature's `current_task` and `path`.
4. Works for ANY interruption source — death, flee, preemption, player
   cancel, nav graph invalidation, or any future source. Callers should
   not need to know which cleanup functions to call.

**Current call sites that would use this function:**
- Nav graph invalidation abandonment (sim.rs ~1870): calls all 5 cleanup
  functions plus manual creature field clearing.
- Mope preemption (sim.rs ~3546): calls 4 of 5 cleanup functions (MISSING
  `cleanup_craft_task`), then calls `unassign_creature_from_task`.
- `unassign_creature_from_task` (sim.rs ~3822): only clears creature fields
  and returns task to Available — does NOT call any per-kind cleanup.
- Future: death handler (combat_military.md section 3), flee behavior
  (section 7), task preemption system (section 8).

**Existing per-kind cleanup functions and what they do:**
- `cleanup_haul_task`: Phase-aware. GoingToSource clears source inventory
  reservations. GoingToDestination removes items from creature inventory
  and drops them as a ground pile.
- `cleanup_cook_task`: Clears reservations at kitchen inventory, marks
  task Complete.
- `cleanup_craft_task`: Clears reservations at workshop inventory, marks
  task Complete.
- `cleanup_harvest_task`: Marks task Complete (no reservations).
- `cleanup_acquire_item_task`: Clears reservations at source inventory
  (ground pile or building), marks task Complete.

**Known bug:** Mope preemption omits `cleanup_craft_task`, so interrupting
a creature mid-craft leaks workshop item reservations.

**Design note:** The combat design doc (docs/drafts/combat_military.md)
distinguishes resumable tasks (Build, Furnish — return to Available) from
non-resumable tasks (everything else — cancel outright). The unified
function should encode this distinction. The preemption system (section 8)
also needs this function as a prerequisite.

**Draft:** docs/drafts/combat_military.md (sections 3, 7, 8)

**Related:** F-creature-actions, F-dynamic-pursuit

### Economy & Logistics

#### F-clothing — Wearable clothing system
**Status:** Todo

Creatures can wear clothing items in defined body slots (e.g., head, torso, legs, feet). Clothing is crafted at workshops, stored in inventories, and equipped by creatures. Many details TBD: slot system design (fixed slots vs. layering), how clothing affects mood/comfort/thoughts, crafting recipes and material requirements, visual representation (sprite overlays? color tinting?), clothing durability and wear, species-specific clothing (elf vs. other species body plans), and whether clothing provides any mechanical benefits beyond mood. This is the base wearable-item infrastructure that armor builds on.

**Blocks:** F-armor

#### F-crafting — Non-construction jobs and crafting
**Status:** Todo · **Phase:** 8+ · **Refs:** §11

Jobs beyond construction: woodworking, weaving, cooking, enchanting.
Crafting system for tools, furniture, and magical items.

**Blocks:** F-elf-weapons
**Related:** F-bldg-workshop, F-items, F-magic-items, F-recipes

#### F-elf-acquire — Elf personal item acquisition
**Status:** Done · **Phase:** 4

Idle elves check a personal `wants` list (same `LogisticsWant` type used by
buildings) during heartbeat Phase 2c. When owned inventory is below the target,
the elf creates an `AcquireItem` task to pick up unowned items from any ground
pile or building — ignoring logistics priority. Items are reserved at creation
to prevent double-claiming. On arrival, items transfer to the creature's
inventory with ownership. Default want: `[Bread: 2]`.

**Related:** F-bldg-kitchen, F-bread, F-hauling, F-logistics

#### F-food-chain — Food production/distribution pipeline
**Status:** Todo · **Phase:** 3

Design and implement the basic food logistics chain: fruit is harvested
from trees, carried to a storehouse, kitchen converts fruit into
shelf-stable bread, bread stored in storehouse or carried to dining hall
for communal meals. Defines how items flow between buildings and how
elves decide where to deliver resources. A focused near-term subset of
the general F-logistics system, scoped to food only. Needs a draft
design doc before implementation to work out pickup/delivery task
creation, building input/output slots, and elf decision-making.

**Related:** F-bldg-dining, F-bldg-kitchen, F-bldg-storehouse, F-bread, F-fruit-prod, F-fruit-variety, F-hauling, F-logistics, F-recipes

#### F-fruit-prod — Basic fruit production and harvesting
**Status:** Todo · **Phase:** 2 · **Refs:** §13

Tree produces fruit at Leaf voxels over time. Elves harvest fruit to refill
their food gauge. Production rate depends on number of Leaf voxels
(photosynthesis capacity). Basic version: fruit spawns periodically at
random Leaf-adjacent positions, elves pathfind to harvest. Bridges the gap
between the existing food decay mechanic (F-food-gauge) and the advanced
food system (F-fruit-variety).

**Related:** F-branch-growth, F-elf-needs, F-food-chain, F-food-gauge, F-fruit-variety, F-population

#### F-fruit-variety — Procedural fruit variety and processing
**Status:** In Progress · **Phase:** 7 · **Refs:** §13

Procedural fruit generation system: worldgen creates 20-40+ unique fruit
species per game from composable parts (flesh, rind, seed, fiber, sap,
resin) and properties (starchy, sweet, fibrous, luminescent, pigmented,
etc.). Processing paths emerge from part properties — recipes match on
properties, not fruit IDs. Coverage constraints guarantee every world has
enough fruits for food, fiber, dye, alchemy, medicine, and other chains.
Vaelith names generated from property-derived morphemes. Greenhouses
cultivate a single chosen species. Some fruits are wild-only.

**Done so far:** Fruit species types and procedural generation with
coverage-biased algorithm. FruitSpecies tabulosity table in SimDb.
Worldgen integration (species generation + voxel-to-species mapping).
Elfcyclopedia server with /fruits list and detail pages. Greenhouse
furnishing type with species picker UI, autonomous production during
logistics heartbeat. Fruit-specific item display names in all inventory
UI (Vaelith name + shape noun, e.g. "Shinethúni Fruit x3"). Material
enum with FruitSpecies variant and serde support.

**Still TODO:** Property-based recipe matching, fruit processing paths,
wild-only species restrictions, fruit part rendering/visual
differentiation, deeper integration with food chain and cooking.

**Draft:** `docs/drafts/fruit_variety.md`

**Related systems:** F-fruit-prod (production mechanics), F-recipes
(property-based recipe matching), F-food-chain (logistics pipeline),
item schema (FruitSpeciesId references).

**Blocks:** F-civ-knowledge
**Related:** F-bldg-kitchen, F-civ-knowledge, F-civilizations, F-food-chain, F-fruit-naming, F-fruit-prod, F-fruit-sprite-ui, F-fruit-sprites, F-fruit-yields, F-logistics-filter, F-recipes

#### F-hauling — Item hauling task type
**Status:** Done · **Phase:** 3

Multi-phase Haul task: creature walks to source (ground pile or building),
picks up reserved items, walks to destination building, deposits them.
Includes item reservation system to prevent double-claiming, cleanup on
task abandonment (clear reservations or drop carried items as ground pile).

**Related:** F-elf-acquire, F-food-chain, F-logistics

#### F-items — Items and inventory system
**Status:** Done · **Phase:** 3

Core item/inventory system. Elves can carry items on their person. Items
can pile on the ground at a location (a generic heap of stuff). Later,
buildings (especially storehouses) can hold items. Each item has a type,
quantity, and location (carried by creature, on ground at coord, or in
building). Foundation for food management, crafting, and logistics.

**Related:** F-crafting, F-logistics, F-manufacturing

#### F-jobs — Elf job/role specialization
**Status:** Todo · **Phase:** 3

Elves have assigned roles (cook, craftself, builder, hauler, etc.) that
determine which tasks they will claim. Currently all elves are
interchangeable — any idle elf claims any available task. A job system
restricts task claiming by role and lets the player manage workforce
allocation. The existing `required_species` field on tasks is a precedent
for this filtering pattern.

**Related:** F-bldg-kitchen, F-bldg-workshop, F-elf-assign, F-task-priority

#### F-logistics — Spatial resource flow (Kanban-style)
**Status:** Done · **Phase:** 7 · **Refs:** §14

Buildings have logistics config: a priority (1-10) and a list of item
wants (kind + target quantity). A periodic LogisticsHeartbeat scans
buildings, counts current + in-transit items, and creates Haul tasks to
fill shortfalls. Sources are ground piles first, then lower-priority
buildings. UI in the structure info panel allows enabling logistics,
setting priority, and configuring wants.

**Related:** F-bldg-storehouse, F-elf-acquire, F-food-chain, F-hauling, F-items

#### F-logistics-filter — Logistics material filter
**Status:** Done · **Phase:** 4

Extend logistics wants with material filtering. Currently wants store only
ItemKind; this adds a MaterialFilter enum (Any / Specific(Material)) so
storehouses can request specific fruit species or any material. Dynamic
two-step UI picker replaces hardcoded item list. Exposes all item kinds
(not just Bread/Fruit) and all material variants. Single-material
reservation under Any filter, hauled_material tracking for precise
in-transit counting, additive overlapping want semantics.

**Draft:** `docs/drafts/logistics_material_filter.md`

**Related:** F-fruit-variety

#### F-mana-system — Mana generation, storage, and spending
**Status:** Todo · **Phase:** 2 · **Refs:** §11, §13

Core mana economy: tree stores mana, elves generate it (flat rate initially),
construction and growth spend it. The central feedback loop — happy elves
produce more mana, mana enables growth, growth makes elves happier.

**Blocks:** F-mana-mood, F-root-network
**Related:** F-branch-growth, F-choir-build, F-mass-conserve, F-population, F-tree-info

#### F-manufacturing — Item schema expansion + workshop manufacturing
**Status:** Done

Item schema expansion (quality, materials, subcomponents, enchantments),
data-driven recipe system, and workshop manufacturing pipeline.

**Done:** Item schema (ItemKind: Bow/Arrow/Bowstring, Material enum,
enchantment tables, subcomponent table), recipe system in GameConfig
(bowstring/bow/arrow recipes), workshop furnishing activation, Craft
task + workshop monitor (process_workshop_monitor/do_craft), bridge
methods (set_workshop_config/get_recipes/get_structure_info workshop
fields), SetWorkshopConfig command, logistics wants from recipes.

**TODO:** GDScript crafting UI section in structure info panel (display
workshop_enabled, recipe list, craft_status — mirrors kitchen cooking
section).

**Related:** F-bldg-kitchen, F-bread, F-items

#### F-pile-gravity — Ground pile gravity and merging
**Status:** Done · **Phase:** 4

Ground piles that are not physically on a solid surface (e.g., after the
platform beneath them is deconstructed) should fall until they reach a
surface. If a falling pile lands on a voxel that already has a ground pile,
the two piles merge their inventories into one.

#### F-recipes — Recipe system for crafting/cooking
**Status:** Done · **Phase:** 3

Shared recipe abstraction for kitchens and workshops: input items +
processing time → output items. Kitchens use recipes to convert fruit
to bread; workshops use recipes to convert wood to bows. Data-driven
via GameConfig so recipes can be added/tuned without code changes.
Avoids hardcoding conversion logic per building type.

**Related:** F-bldg-kitchen, F-bldg-workshop, F-crafting, F-food-chain, F-fruit-variety

#### F-task-assign-opt — Event-driven bidirectional task assignment
**Status:** Todo · **Phase:** 4

Trigger task assignment at two events: (1) when an elf becomes idle, and
(2) when a new task is added to the DB. In either case, run a bidirectional
matching pass that can assign multiple idle elves to multiple available tasks
simultaneously, preferring proximity (Dijkstra nav-graph distance). Not
globally optimal (that would be computationally prohibitive), but significantly
better than the current first-found pull model.

Supersedes F-task-proximity (pull-side Dijkstra nearest, already implemented).

**Blocked by:** F-task-priority
**Related:** F-task-priority, F-task-proximity

#### F-task-proximity — Proximity-based task assignment (Dijkstra nearest)
**Status:** Done · **Phase:** 4

**Related:** F-task-assign-opt

#### F-tree-capacity — Per-tree carrying capacity limits
**Status:** Todo · **Phase:** 7 · **Refs:** §13

Each tree has a carrying capacity limiting how many elves/structures it can
support. Encourages distributed village design across multiple trees.

**Related:** F-multi-tree, F-population

### Social & Emotional

#### F-apprentice — Skill transfer via proximity
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elves learn skills by working near skilled elves. Apprenticeship as an
emergent social/economic system.

#### F-emotions — Multi-dimensional emotional state
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Emotions as multiple simultaneous dimensions: joy, fulfillment, sorrow,
stress, pain, fear, anxiety. Not a single "happiness" number.

**Blocks:** F-elf-leave, F-hedonic-adapt, F-mana-mood
**Related:** F-social-graph

#### F-emotions-basic — Mood score from thought weights
**Status:** Done · **Phase:** 4 · **Refs:** §18

Derived mood score: sum of configurable per-ThoughtKind weights across a
creature's active thoughts. Seven-tier label (Devastated → Elated). Computed
on demand, never stored. Lays groundwork for full F-emotions.

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

**Blocked by:** F-emotions, F-mana-system

#### F-mood-system — Mood with escalating consequences
**Status:** Done · **Phase:** 4 · **Refs:** §18

Unhappy elves mope instead of working. At each creature heartbeat, compute a
Poisson-like mope probability using integer math: `roll % mean < elapsed`
where `mean` is a per-MoodTier config value (0 for Content+, scaling down
through Unhappy / Miserable / Devastated). When triggered, elf gets a
`TaskKind::Mope` — walks home (if assigned) or stays at current node, idles
for a configurable duration, then resumes normal behavior. Moping replaces
normal task pickup and can also interrupt in-progress player-directed tasks
at Miserable/Devastated tiers (autonomous tasks like sleep/eat are never
interrupted). All rates and durations in `MoodConsequencesConfig`. No work
speed reduction, no social contagion, no personality modifiers — those build
on top later.

#### F-narrative-log — Events and narrative log
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Sim emits narrative events (arguments, friendships formed, dramatic moments).
Log viewable by player, drives emergent storytelling.

#### F-notifications — Player-visible event notifications
**Status:** In Progress · **Phase:** 4

Toast-style notification system for important sim events.

**Done so far:**
- Toast display UI (notification_display.gd): toasts appear in bottom-right,
  stay 4s, fade out over 1s, max 8 visible, mouse-transparent.
- Debug "Test Notif" button in toolbar debug row (goes through full sim
  command pipeline, multiplayer-aware).
- Sim-side notification table in SimDb (tick, message, auto-increment ID).
  Notifications persist across saves. Bridge methods:
  get_notifications_after(id), get_max_notification_id(),
  send_debug_notification(msg).
- Moping creates a notification with elf name and mood tier.
- Load-game initializes notification cursor to max existing ID so
  historical notifications aren't replayed as toasts.

**Still needed:**
- Notification history panel: a bell icon button (bottom-right, near where
  toasts appear) that opens a scrollable log of all past notifications.
  Unread indicator (count badge or color) until panel is opened.
- Wire more sim events to push notifications (construction complete,
  creature idle, structure collapsed, elf left, etc.).

**Blocks:** F-elf-leave
**Related:** F-status-bar

#### F-personality — Personality axes affecting behavior
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Multi-axis personality model affecting task preferences, social behavior,
stress responses, and creative output.

**Blocks:** F-cultural-drift
**Related:** F-social-graph

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

**Related:** F-weather

#### F-social-graph — Relationships and social contagion
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elf-to-elf relationships: friendships, rivalries, romantic bonds, mentorship.
Emotional contagion spreads mood through social connections.

**Related:** F-emotions, F-personality

#### F-thoughts — Creature thoughts (DF-style event reactions)
**Status:** Done · **Phase:** 4 · **Refs:** §18
**Draft:** `docs/drafts/thoughts.md`

Dwarf Fortress-inspired thought system. Creatures accumulate thoughts in response
to events (sleeping in own home, enduring a low ceiling, eating a meal). Each
thought has a `ThoughtKind` enum (data in variants), a tick timestamp, and
per-kind dedup cooldown and expiry durations. `Vec<Thought>` per creature,
hard-capped at 200, with periodic expiry cleanup. Displayed on the creature info
panel. Later feeds into emotional dimensions when `F-emotions` lands.

### Culture, Language & Music

#### F-audio-sampled — Sampled vocal syllables from conlang
**Status:** Todo · **Phase:** 8+ · **Refs:** §21

Phase 2 audio: pre-recorded or AI-generated vocal syllables from the Vaelith
phoneme inventory, concatenated for singing.

**Blocked by:** F-vaelith-expand
**Blocks:** F-audio-vocal

#### F-audio-synth — Waveform synthesis for audio rendering
**Status:** Done · **Phase:** 6 · **Refs:** §21

Phase 1 audio: generate waveforms from MIDI-like note data for playback in
Godot. Debugging and validation tool, placeholder for richer audio later.

**Related:** F-sound-effects

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
**Status:** Done · **Phase:** 6 · **Refs:** §20

Generate elf names using Vaelith phonotactic rules. Names are compounds of
meaningful roots (e.g., *Thíraleth* = "star-tree"), genderless, with given
name + surname structure. Names should sound consistent with the conlang and
be deterministic given the same PRNG state. Adds a `name` field to the
`Creature` struct, assigned at spawn time.

**Related:** F-creature-tooltip, F-vaelith-expand

#### F-lang-crate — Shared Vaelith language crate
**Status:** Done · **Phase:** 6 · **Refs:** §20

Create `elven_canopy_lang`, a pure-Rust crate providing the Vaelith language
as a programmatic resource shared by the sim and music crates. Includes:
data-driven lexicon (`data/vaelith_lexicon.json`) with part-of-speech, tones,
vowel class, and name tags; core language types (`Tone`, `VowelClass`,
`Syllable`, `LexEntry`) migrated from the music crate; phonotactic rules;
and a deterministic name generator.

#### F-music-gen — Palestrina-style music generator (standalone)
**Status:** Done · **Phase:** 6 · **Refs:** §21
**Crate:** `elven_canopy_music`

Complete standalone generator: Palestrina-style SATB counterpoint with
Vaelith lyrics, Markov melodic models trained on Renaissance corpus,
simulated annealing optimization, MIDI + LilyPond output, CLI with
batch/mode-scan.

#### F-music-runtime — Integrate music generator into game
**Status:** Done · **Phase:** 6 · **Refs:** §21

Bridge the standalone music crate into the Godot runtime. Generate music
in response to game events (construction, celebrations, idle time). Requires
audio output path (see F-audio-synth).

**Related:** F-bldg-concert, F-choir-build, F-choir-harmony

#### F-music-use-lang — Migrate music crate to shared lang crate
**Status:** Done · **Phase:** 6

Migrate `elven_canopy_music` to depend on `elven_canopy_lang` for Vaelith
types and lexicon data instead of maintaining its own hardcoded vocabulary.
The music crate keeps its phrase-generation templates, brightness-biased
selection, and SA text-swap logic, but delegates to the lang crate for
vocabulary lookup, core types (`Tone`, `VowelClass`, `Syllable`), and
phonotactic rules.

#### F-proc-poetry — Procedural poetry via simulated annealing
**Status:** Todo · **Phase:** 6 · **Refs:** §20

Generate Vaelith-language poetry using simulated annealing (similar to the
music generator's approach). Poetry quality varies by elf skill, affects
social events and mana.

**Blocked by:** F-vaelith-expand
**Related:** F-poetry-reading

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

**Blocks:** F-audio-sampled, F-proc-poetry
**Related:** F-elf-names, F-poetry-reading

### Combat & Defense

#### F-armor — Wearable armor system
**Status:** Todo

Armor items that can be worn in clothing slots, providing damage reduction in combat. Builds on the clothing/wearable system (F-clothing) for slot mechanics and equip/unequip flow. Many details TBD: armor types and their stats (leather, chain, plate?), how damage reduction is calculated (flat reduction? percentage? per-damage-type?), armor durability and repair, crafting recipes and material requirements, how armor interacts with movement speed or other stats, visual representation, whether armor and clothing can be worn simultaneously (layering), and species-specific armor availability.

**Blocked by:** F-clothing
**Blocks:** F-military-armor

#### F-arrow-durability — Arrow durability and recovery
**Status:** Todo · **Phase:** 3

Arrow durability system: arrows lose durability on impact and may break. Recoverable arrows that survive impact are placed on the ground for pickup. Extracted from F-projectiles as a separate concern — not needed for the first pass at combat.

#### F-attack-move — Attack-move task (walk + fight en route)
**Status:** Todo

TaskKindTag::AttackMove — hotkey A + click on ground. Extension table TaskAttackMoveData with destination (VoxelCoord) and current_target (Option<CreatureId>, plain ID). Walk toward destination; on each activation scan for hostiles. If hostile detected, set current_target and engage (melee/ranged actions). Poll current_target vital_status — if Dead or missing, nullify and resume walking. On arrival at destination with no active target, task completes.

**Draft:** docs/drafts/combat_military.md (§2 "Attack-Move")

**Blocks:** F-combat
**Related:** F-attack-task

#### F-attack-task — AttackCreature task (player-directed target pursuit)
**Status:** Done

TaskKindTag::AttackTarget — player right-clicks a hostile creature. Creates task with TaskOrigin::PlayerDirected, PreemptionLevel::PlayerCombat(6). Extension table TaskAttackTargetData with target: CreatureId (plain ID, not FK). Behavior: pathfind toward target via dynamic pursuit, when adjacent perform melee actions, when in range with LOS perform shoot actions. Poll target vital_status each activation — if Dead or row missing, task completes. Works with melee-only initially; ranged is additive. Autonomous combat tasks (created by hostile detection) are immediately claimed by the detecting creature — NOT left in Available state.

**Failed pathfinding:** If pathfinding fails (target unreachable), retry on the next activation. After N consecutive failures (configurable, e.g., attack_path_retry_limit = 3), cancel the task. Creature returns to normal behavior (idle/wander) and may re-detect the target on a subsequent activation if the target has moved to a reachable location.

**Draft:** docs/drafts/combat_military.md (§5 "Attack Tasks")

**Related:** F-attack-move

#### F-combat — Combat and invader threat system
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Invader types, threat mechanics, and basic combat resolution. Ties into
fog of war for surprise attacks.

**Blocked by:** F-attack-move, F-enemy-ai, F-military-groups, F-rts-selection
**Blocks:** F-defense-struct, F-elf-weapons, F-military-campaign, F-military-org
**Related:** F-engagement-style, F-fog-of-war

#### F-defense-struct — Defensive structures (ballista, wards)
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Ballista turrets, magic wards, and other defensive construction. Requires
the construction system to support these build types.

**Blocked by:** F-combat

#### F-elf-weapons — Bows, spears, clubs for elf combat
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Weapon types with different ranges, damage, and crafting requirements.

**Blocked by:** F-combat, F-crafting
**Related:** F-bldg-workshop

#### F-enemy-ai — Hostile creature AI (goblin/orc/troll behavior)
**Status:** In Progress

Simple aggression AI for non-civ hostile creatures. This is the first "it all comes together" milestone — debug-spawn a goblin and watch it chase and attack an elf.

**Done so far:** Simplified hostile AI via the wander path (no tasks, no formal detection/preemption). `Species::is_hostile()` gates behavior for Goblin/Orc/Troll. On each activation with no task, `hostile_pursue()` collects living elf nav nodes, runs Dijkstra to find the nearest reachable elf, A* to get a path, and moves one edge toward it. When in melee range, auto-calls `try_melee_strike()`. On cooldown, waits in place and re-activates when cooldown expires. Falls back to random wander if no elf reachable. Events threaded through the activation chain so combat events (CreatureDamaged, CreatureDied) are properly emitted. Refactored `wander()` into `hostile_pursue()`, `random_wander()`, and shared `move_one_step()`. Formal hostile detection system (F-hostile-detection) with configurable detection range, CombatAI enum on SpeciesData, and faction-based hostility. Task-driven attack (F-attack-task) with AttackTarget task kind and dynamic pursuit. Preemption system (F-preemption) so combat can interrupt lower-priority tasks.

**Not yet done:** Two-phase proximity optimization (squared distance filter before pathfinding). Target selection by closest distance rather than first-by-ID. Path caching to avoid Dijkstra+A* on every activation.

**Draft:** docs/drafts/combat_military.md (§6 "Initial Behavior")

**Draft:** docs/drafts/combat_military.md (§6 "Initial Behavior")

**Blocks:** F-combat
**Related:** F-engagement-style

#### F-engagement-style — Unified engagement style (species + military group combat tactics)
**Status:** Todo

A single `EngagementStyle` struct that governs how a creature uses its weapons in combat. Replaces the current split between `CombatAI` (species-level, coarse) and `HostileResponse` (military group, binary Fight/Flee). The same struct lives on both `SpeciesData` (species defaults for non-civ creatures) and `MilitaryGroup` (player-configurable per-group overrides for civ creatures), using identical code paths.

**Fields (draft — refine during design):**

- **Weapon preference:** Prefer ranged / prefer melee / mixed (ranged at distance, melee when close).
- **Ammo exhaustion behavior:** Switch to melee / flee / hold position and wait.
- **Engagement initiative:** Aggressive (pursue on detection) / defensive (fight only when attacked or when hostiles enter short range) / passive (never initiate).
- **Melee confidence:** Willing to melee / reluctant (flee if forced into melee). Captures "I'm an archer, don't make me swing a sword."
- **Disengage threshold:** Optional HP% below which the creature breaks off and flees (distinct from F-instinctual-flee which is involuntary panic).

Species defaults should make intuitive sense (goblins: aggressive melee; orc archers: prefer ranged, switch to melee on ammo out; deer: passive). Military group config lets the player override for their civ creatures ("Archers" group: prefer ranged, flee on ammo out; "Vanguard": prefer melee, aggressive).

Supersedes `CombatAI` enum on `SpeciesData` and `HostileResponse` on `MilitaryGroup` — both collapse into `EngagementStyle`. The `should_flee()` / `hostile_pursue()` / `wander()` combat decision logic is rewritten against the unified struct.

**Blocks:** F-instinctual-flee
**Related:** F-combat, F-enemy-ai, F-military-groups

#### F-flee — Flee behavior for civilians
**Status:** Done

Creatures with Flee response (civilian military group default, or FleeOnly combat_ai) detect hostile within range, preempt current task, and perform greedy retreat. At each activation, pick nav neighbor maximizing squared euclidean distance from threat (anchor voxel for multi-voxel threats). Ties broken by NavNodeId. Continue fleeing while hostile is in detection range. Dead-end trapping is acceptable (mirrors panic behavior, motivates escape route construction). Future: cornered behavior, bounded A* instead of greedy.

**Done so far:** Flee behavior implemented in `sim.rs` via `should_flee()` / `flee_step()`. Civ creatures (elves) flee by default — no military groups yet, so all civ creatures are treated as civilians. Non-civ creatures with `CombatAI::FleeOnly` also flee. Elf `hostile_detection_range_sq` set to 225 (15-voxel radius). Flee check runs before the decision cascade in `process_creature_activation` — detects threats via existing `detect_hostile_targets()`, interrupts current task, then greedy retreat (maximize squared distance from nearest threat, NavNodeId tie-breaking). Cornered creatures (no eligible edges) reschedule activation and wait. Flee stops immediately when threat leaves detection range. 8 tests covering: flee direction, task interruption, threat removal, FleeOnly species, passive species, multiple threats, cornered case, aggressive-doesn't-flee.

**Not yet done:** Military group hostile_response gating (depends on F-military-groups). `flee_cooldown_ticks` for persistence after threat leaves range. Bounded A* instead of greedy. Cornered behavior (desperate fighting). Flee toward friendly soldiers. Panic/fear thoughts.

**Draft:** docs/drafts/combat_military.md (§7)

#### F-hostile-detection — Hostile detection and faction logic
**Status:** Done

Activation-driven hostile scanning. On each creature activation, scan for hostiles within hostile_detection_range_sq (SpeciesData, squared euclidean voxels). Hostility determination: per-direction (not mutual). Civ creatures check CivOpinion::Hostile toward other civ. Non-civ creatures with combat_ai: AggressiveMelee/AggressiveRanged treat all civ creatures as hostile (except same-species exemption). Non-civ aggressors don't attack each other. CombatAI enum on SpeciesData (Passive, FleeOnly, AggressiveMelee, AggressiveRanged). Auto-escalation when attacked (design question: no target civ for non-civ attackers — may only apply to civ-vs-civ). Detection is O(n) scan over all creatures with squared-distance filter (BTreeMap spatial index doesn't support 3D range queries). Height makes detection range ineffective across tree levels — design decision needed on whether this is intentional.

**Draft:** docs/drafts/combat_military.md (§6, §7)

#### F-hp-ui — HP bars in creature UI
**Status:** Done

Display creature HP in the game UI. Two elements:

1. **Creature info panel health bar:** A reddish horizontal bar in the
   right-side creature info panel showing current/max HP as numbers within
   the bar (e.g. "87 / 100"). Uses the hp and hp_max fields already exposed
   by SimBridge's creature info dict.

2. **Overhead health bar on sprites:** When a creature's HP is below max,
   render a thin health bar hovering above its billboard sprite. No numbers,
   just a proportional fill bar. Hidden when HP is full (no visual clutter
   in peacetime). Needs hp/hp_max data piped to the sprite renderers.

**Related:** F-hp-death

#### F-instinctual-flee — Instinctual flee thresholds (species-level fear overrides)
**Status:** Todo

A per-species `FleeInstinct` struct on `SpeciesData` that defines involuntary panic responses — situations where a creature flees regardless of its engagement style, military group orders, or player commands. Fear as a biological override, not a tactical decision.

**Trigger conditions (draft — refine during design):**

- **HP threshold:** Flee when HP drops below X% (e.g., deer at 90%, orc at 20%, troll at 5%).
- **Outnumbered threshold:** Flee when hostile-to-ally ratio exceeds N:1 within detection range.
- **Ally death shock:** Flee for N ticks after witnessing an ally die within close range.
- **Fire proximity:** Flee when fire is within N voxels (once F-fire-basic exists).
- **Species-specific phobias:** Data-driven list of stimuli (e.g., elephants spooked by fire, prey animals by large predators).

**Interaction with EngagementStyle:** FleeInstinct overrides EngagementStyle. A soldier ordered to Fight with aggressive engagement will still panic-flee if their FleeInstinct triggers. The override is temporary — once the creature is out of the trigger zone or the duration expires, they resume their normal engagement behavior. Player cannot suppress instinctual flee (but species with high courage like trolls have very low thresholds, so it rarely fires).

**Distinct from EngagementStyle's disengage threshold:** The disengage threshold in EngagementStyle is a tactical, voluntary "I should retreat." FleeInstinct is involuntary panic — different movement behavior (possibly ignoring pathing efficiency, running in a random direction away from threat), different visual feedback (panic animation/particles), and cannot be overridden by orders.

**Blocked by:** F-engagement-style

#### F-military-armor — Military group armor policy
**Status:** Todo

Military groups can specify an armor policy — what armor, if any, members should wear. Extends the military group equipment system (F-military-equip) to handle armor specifically, using the wearable armor system (F-armor) for the actual equip mechanics. Many details TBD: how armor policy is specified (any available armor? specific armor type? minimum protection level?), how the policy interacts with armor availability (wait for crafting? use whatever's available?), priority of armor acquisition vs. weapon acquisition, UI for armor policy configuration within the military group detail panel, and how armor status is displayed per creature and per group.

**Blocked by:** F-armor, F-military-equip
**Related:** F-military-groups

#### F-military-campaign — Send elves on world expeditions
**Status:** Todo · **Phase:** 8+ · **Refs:** §26

Send elf parties on expeditions in the wider world with direct tactical
control (unlike Dwarf Fortress's hands-off approach).

**Blocked by:** F-combat, F-military-org

#### F-military-equip — Military group equipment acquisition
**Status:** Todo

The player configures equipment policies on military groups (e.g., "members should carry a bow and 10 arrows"). The system automatically generates logistics-style wants for group members, causing them to seek out and acquire the specified items — similar to personal item acquisition but driven by group policy rather than individual creature wants. Unlike personal wants, group equipment wants do NOT confer ownership — items are held for the group's purpose, not the creature's personal use. Many details TBD: how equipment policies are specified (item kind + quantity pairs? equipment slots?), how wants interact with existing personal wants (priority? separate queue?), what happens when equipment is unavailable (partial fulfillment? queuing?), how re-equipment works after item loss (death drops, combat breakage), whether equipment policies generate tasks immediately or on a heartbeat cycle, and how the UI surfaces equipment status per creature and per group. Depends on F-military-groups for the group data model and UI.

**Blocks:** F-military-armor
**Related:** F-military-groups

#### F-military-groups — Military group data model and configuration
**Status:** Todo

MilitaryGroup table in SimDb with civ_id FK (cascade on civ delete). Auto-increment PK. Fields: name, is_default_civilian (bool, invariant: exactly one per civ), hostile_response (Fight/Flee). Two default groups per civ during worldgen (Civilians with Flee, Soldiers with Fight). Implicit civilian membership: creature `military_group: None` means civilian (governed by civ's default civilian group settings), `Some(group_id)` means explicitly assigned. Civilian count computed as total civ creatures minus assigned creatures. Commands: CreateMilitaryGroup, DeleteMilitaryGroup (reject for civilian group, nullify members), ReassignMilitaryGroup, RenameMilitaryGroup, SetGroupHostileResponse. `should_flee()` updated to check group hostile_response.

UI: Military panel opened via existing Units [U] button. Summary page lists groups with member counts, click to navigate to detail. Detail page: left column with scrollable member list + reassign buttons, right column with hostile_response toggle (Fight/Flee) and delete button. Reassignment overlay (modal) lists groups for quick reassignment. Creature info panel shows military group name as clickable link to the group's detail view. Group configuration UI included in initial implementation (not deferred to polish).

**Draft:** docs/drafts/military_groups.md

**Draft:** docs/drafts/combat_military.md (§1)

**Blocks:** F-combat
**Related:** F-engagement-style, F-military-armor, F-military-equip

#### F-military-org — Squad management and organization
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Organize elves into military squads with patrol routes, defensive
positions, and alert levels.

**Blocked by:** F-combat
**Blocks:** F-military-campaign

#### F-projectiles — Projectile physics system (arrows)
**Status:** Done

SubVoxelCoord type (i64 per axis, 2^30 sub-units per voxel). Projectile entity table in SimDb with inventory-based payload (FK nullify on shooter, FK cascade on inventory). Ballistic trajectory with symplectic Euler integration (velocity updated before position). ProjectileTick batched event — one event per tick while any projectiles are in flight, advances all projectiles. Per-tick: save prev_voxel, apply gravity to velocity, apply velocity to position, check voxel collision (solid → surface impact, ground pile at prev_voxel), check creature collision (spatial index), check bounds (out of world → despawn). Momentum-based damage formula computed at impact time from velocity + item properties (linear in speed, not quadratic). Rendering: projectile_renderer.gd (pool pattern, thin elongated CylinderMesh oriented along velocity vector), SimBridge returns packed position+velocity arrays, interpolation via position + velocity * fractional_offset.

**Bounds check must be performed on i64 sub-voxel coordinates BEFORE converting to VoxelCoord via `as i32`**, to prevent silent truncation.

**ProjectileTick scheduling guard:** Schedule a ProjectileTick event if and only if the projectile table was empty before this spawn (count went from 0 → 1). Prevents duplicate scheduling when multiple archers fire on the same tick.

**Draft:** docs/drafts/combat_military.md (§4)

**Related:** F-spatial-index

#### F-spatial-index — Creature spatial index for voxel-level position queries
**Status:** Done

BTreeMap<VoxelCoord, Vec<CreatureId>> on SimState, #[serde(skip)], rebuilt on load from Alive creatures. Maintained at every position mutation point (wander, walk_toward_task, handle_creature_movement_complete, resnap_creatures, spawn, death). Centralized update_creature_position() helper. Multi-voxel creatures (trolls 2x2x2) register at all occupied voxels. Used by projectile hit detection and hostile detection scanning. Note: BTreeMap with VoxelCoord lexicographic ordering does NOT support efficient 3D range queries — detection scans are O(n) over all creatures with a squared-distance filter, not range queries.

**Rebuild ordering:** The spatial index rebuild must run AFTER species_table is populated (footprint data comes from SpeciesData in config). The species_table is #[serde(skip)] and rebuilt from config after deserialization. If the spatial index rebuild runs before species_table is populated, the footprint lookup for large creatures will fail. Same ordering constraint as the nav graph rebuild — both depend on config-derived data being available.

**Draft:** docs/drafts/combat_military.md (§4 "Creature Spatial Index")

**Related:** F-projectiles

#### F-voxel-exclusion — Creatures cannot enter voxels occupied by hostile creatures
**Status:** Todo · **Phase:** 3

Creatures should not be able to enter a voxel already occupied by a hostile creature (and vice versa). Currently multiple creatures freely share voxels regardless of faction. Needs pathfinding and/or movement-step checks to enforce. Edge case: if creatures are already sharing a voxel when hostility begins, behavior is TBD (push apart, allow temporary overlap, etc.).

### World Expansion & Ecology

#### F-civ-knowledge — Civilization knowledge system (fruit tiers, discovery)
**Status:** Todo

Civilization knowledge system: CivFruitKnowledge table with three tiers
(Awareness → Properties → Cultivation). Worldgen distributes fruit
knowledge across civs biased by species/culture. Player civ starts with
4-5 fruits at Cultivation, 5-10 at Properties, most at Awareness.
DiscoverCiv, SetCivOpinion, and LearnFruit SimAction commands (initially
exercised only by worldgen and debug). Knowledge is monotonically
increasing (no forgetting).

**Draft:** `docs/drafts/elfcyclopedia_civs.md` §Knowledge System

**Blocked by:** F-fruit-variety
**Blocks:** F-elfcyclopedia-know
**Related:** F-elfcyclopedia-know, F-fruit-variety

#### F-civilizations — Procedural civilization generation and diplomacy
**Status:** Done

Procedural civilization generation during worldgen: ~10 civs with
CivSpecies (Elf/Human/Dwarf/Goblin/Orc/Troll), culture tags, asymmetric
diplomacy graph. Civilization table in SimDb, CivRelationship table with
directed opinion pairs, player_controlled flag, Creature.civ_id column.
Session-side player→civ assignment (not persisted). Placeholder naming
for non-elf civs, Vaelith names for elf civs.

**Draft:** `docs/drafts/elfcyclopedia_civs.md` §Civilizations

**Related:** F-fruit-variety

#### F-cultural-drift — Inter-tree cultural divergence
**Status:** Todo · **Phase:** 7 · **Refs:** §7, §18

Elves on different trees develop distinct traditions, art styles, and
social norms over time.

**Blocked by:** F-multi-tree, F-personality

#### F-fruit-naming — Fruit naming overhaul
**Status:** Done · **Phase:** 7

Overhaul fruit naming to eliminate collisions and produce meaningful,
varied names. Temperature-weighted root assignment from an expanded
polysemous lexicon pool, with world-naming fallback (historical figures,
locations) for less distinctive fruits. Zero number suffixes.

**Draft:** `docs/drafts/fruit_naming.md`

**Related:** F-fruit-variety, F-fruit-yields

#### F-fruit-yields — Fruit yield model overhaul
**Status:** Done · **Phase:** 7

Replaced `FruitPart.yield_percent: u8` (percentage of fruit mass,
parts summing to 100) with `component_units: u16` (independent per-part
unit count, typical range 10-100). Each part independently specifies how
many units it produces when the fruit is processed. The fruit's overall
"size" is the sum of all parts' units. Recipes consume a fixed number of
same-species component units (e.g., 10 starchy units → 1 loaf), so a
fruit with more starchy pulp produces more bread.

Updated generation (independent per-part allocation replaces the
breakpoint algorithm), naming intensity (sums component_units instead of
yield_percent), appearance derivation, elfcyclopedia display, and all
tests and documentation.

**Related:** F-fruit-naming, F-fruit-variety

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

**Blocks:** F-cultural-drift, F-root-network
**Related:** F-multiplayer, F-tree-capacity, F-tree-species

#### F-root-network — Root network expansion and diplomacy
**Status:** Todo · **Phase:** 7 · **Refs:** §2

Player grows roots toward other trees. Diplomacy phase: mana offerings
convince trees to join the network. Expands buildable space and perception
radius.

**Blocked by:** F-mana-system, F-multi-tree
**Related:** F-fog-of-war

#### F-tree-memory — Ancient tree knowledge/vision system
**Status:** Todo · **Phase:** 7 · **Refs:** §2

The player's tree surfaces ancient memories: hints about threats, lost
construction techniques, forest history. Journal or vision system.

#### F-tree-species — Multiple tree species with properties
**Status:** Todo · **Refs:** §8

Different tree species with distinct properties. Needs a detailed design —
scope, gameplay implications, and interaction with existing tree generation
are not yet specified.

**Related:** F-branch-growth, F-multi-tree

#### F-worldgen-framework — Worldgen generator framework
**Status:** Done

Worldgen entry point called during StartGame that runs generators in
defined order (tree → fruits → civs → knowledge). Dedicated worldgen
PRNG seeded from world seed. WorldgenConfig subsection of GameConfig
grouping FruitConfig and CivConfig. Small plumbing feature — establishes
the pattern for generator sequencing.

**Draft:** `docs/drafts/elfcyclopedia_civs.md` §Worldgen Framework

### Soul Mechanics & Magic

#### F-magic-items — Magic item personalities and crafting
**Status:** Todo · **Phase:** 8+ · **Refs:** §22

Magic items with emergent personalities from their crafting circumstances
and the souls/emotions imbued in them.

**Related:** F-crafting, F-soul-mech

#### F-soul-mech — Death, soul passage, resurrection
**Status:** Todo · **Phase:** 8+ · **Refs:** §19

Elf death, soul passage into trees, possible resurrection, and
soul-powered constructs (golems, animated defenses).

**Related:** F-creature-death, F-magic-items

### UI & Presentation

#### F-ai-sprites — AI-generated sprite art pipeline
**Status:** Todo · **Phase:** 8+ · **Refs:** §24

Replace placeholder sprites with AI-generated layered art: base body
templates + composited clothing/hair/face layers for visual variety.

#### F-binding-conflicts — Binding conflict detection
**Status:** Todo · **Phase:** 2

Full binding conflict detection beyond the basic debug-build assertion
in F-controls-config-A. Bindings organized by context scopes with
defined overlap rules (e.g., gameplay + construction can be active
simultaneously, but gameplay and main_menu cannot).

Same-context conflicts flagged as warnings at startup. Cross-context
overlaps between non-overlapping scopes are allowed. Visual indicator
in the settings screen (Phase C) when a player-created conflict exists
via rebinding.

Depends on F-controls-config-A (bindings must be centralized first).

**Blocked by:** F-controls-config-A
**Related:** F-controls-config

#### F-bldg-transparency — Toggle building roof/wall transparency to see inside
**Status:** Todo · **Phase:** 2

A toggle (toolbar button or hotkey) that makes building roofs and walls
nearly fully transparent so the player can see elves and furniture inside
enclosed structures. Applies to completed Building/Enclosure voxels only —
platforms, bridges, and tree voxels remain opaque. Rendering-side change
using material alpha override.

**Related:** F-wireframe-ghost, F-zlevel-vis

#### F-build-queue-ui — Construction queue/progress UI
**Status:** Todo · **Phase:** 2

UI panel showing all pending and in-progress construction projects: blueprint
name/type, progress bar, assigned workers, and option to cancel or reprioritize.
Currently players can see individual blueprints in the world but have no
overview of the construction pipeline. Small overlay or sidebar panel.

**Related:** F-construction, F-keybind-help, F-task-priority

#### F-cam-follow — Camera follow mode for creatures
**Status:** Done · **Phase:** 2 · **Refs:** §23

Lock camera focal point to a selected creature. Toggled via creature info
panel button.

#### F-controls-config — Centralized controls config with rebinding and persistence
**Status:** Todo · **Phase:** 2

Centralized input configuration system replacing scattered KEY_* checks
across ~15 GDScript files. ControlsConfig autoload owns all bindings as
data. Player overrides persisted to user://controls.json (delta from
defaults). Includes invert-X/Y, invert scroll zoom, mouse sensitivity.

Three sub-features track the phases:
- F-controls-config-A: Centralize bindings, migrate all handlers
- F-controls-config-B: Persistence, sensitivity/invert settings
- F-controls-config-C: Full settings screen with rebinding UI

When complete, deletes keybind_help.gd and replaces "? Help" toolbar
button with "Controls" button.

**Draft:** docs/drafts/controls_config.md

**Related:** F-binding-conflicts, F-controls-config-A, F-controls-config-B, F-controls-config-C, F-keybind-help, F-modifier-keybinds

#### F-controls-config-A — ControlsConfig autoload and handler migration
**Status:** Todo · **Phase:** 2

Create ControlsConfig autoload with all bindings defined as data.
Each binding has key, category, label, context, and optional alt_key,
physical flag, hidden flag. API: is_action(event, name) for event
callbacks, is_pressed(name) for polling (delegates to InputMap for
movement actions), get_label_suffix(name) for dynamic button labels.

Migrate every input handler to query ControlsConfig: action_toolbar,
orbital_camera, construction_controller, selection_controller,
placement_controller, pause_menu, main_menu, multiplayer_menu,
save/load dialogs, tree_info_panel, task_panel, units_panel,
structure_list_panel. Toolbar and construction buttons use
get_label_suffix() so labels reflect current bindings.

Movement bindings (WASD, arrows) use physical keycodes for non-QWERTY
layout support. ESC unified as single ui_cancel action across all
handlers. Debug-build startup assertion checks for duplicate keys
within overlapping context scopes.

keybind_help.gd keeps hardcoded content (no visible behavior change
during refactoring — replacement happens in Phase C).

**Draft:** docs/drafts/controls_config.md (Phase A)

**Blocks:** F-binding-conflicts, F-controls-config-B
**Related:** F-controls-config

#### F-controls-config-B — Controls persistence and sensitivity settings
**Status:** Todo · **Phase:** 2

Load/save player overrides from user://controls.json (delta from
defaults, schema-versioned). Add non-keybind settings: invert-X,
invert-Y, invert scroll zoom, mouse orbit sensitivity, mouse zoom
sensitivity, key zoom speed. Plumb settings into orbital_camera.gd
for immediate effect. Save triggered from settings screen (Phase C)
or a temporary mechanism.

**Draft:** docs/drafts/controls_config.md (Phase B)

**Blocked by:** F-controls-config-A
**Blocks:** F-controls-config-C
**Related:** F-controls-config

#### F-controls-config-C — Controls settings screen with rebinding UI
**Status:** Todo · **Phase:** 2

Full settings screen replacing keybind_help.gd. Categorized list of
all bindings (excluding hidden) in collapsible sections with defined
display order. Each row: action name, current binding, Rebind button.
Alt-key bindings shown and independently rebindable.

"Press a key" capture: fully modal, 5-second timeout with visual
countdown, visible Cancel button. During capture, if key is already
bound, shows "Already bound to [Action Name]" warning (binding still
set — full conflict prevention is F-binding-conflicts).

Per-binding reset-to-default (icon per row). "Reset All to Defaults"
with confirmation dialog. Non-keybind settings (invert toggles,
sensitivity sliders) in own section. Menu bindings category last.

? key kept as shortcut for opening the settings screen. Delete
keybind_help.gd, replace "? Help" toolbar button with "Controls".

**Draft:** docs/drafts/controls_config.md (Phase C)

**Blocked by:** F-controls-config-B
**Blocks:** F-modifier-keybinds
**Related:** F-controls-config, F-keybind-help

#### F-creature-info — Creature info panel with follow button
**Status:** Done · **Refs:** §26

Right-side panel showing creature details (species, food level, task,
position). Follow button to lock camera.

**Related:** F-creature-tooltip, F-tree-info

#### F-creature-tooltip — Hover tooltips for world objects
**Status:** Done · **Phase:** 2

Floating tooltip on mouse hover over any world object. Covers:

- **Creatures:** Species + name + current activity (e.g., "Elf: Vaelindra — Eating")
- **Buildings/furniture:** Type + custom name (e.g., "Kitchen: Hearthglow")
- **Ground piles:** Up to 3 item stacks shown inline (e.g., "Apple x3, Wood x2").
  If more stacks exist, append summary (e.g., "and 5 more stacks of 23 items").
- **Fruit:** Item name (e.g., "Apple")

Broadened from creature-only tooltip to cover all hoverable world objects.

**Related:** F-creature-info, F-elf-names, F-selection, F-status-bar

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

#### F-elfcyclopedia-know — Elfcyclopedia civ/fruit knowledge pages
**Status:** Todo

Adds knowledge-gated pages to the elfcyclopedia web server: Civilizations
tab (known civs with asymmetric opinions) and Fruits tab (tier-gated
detail — Awareness shows name/appearance, Properties shows parts and
processing paths, Cultivation shows growing info). Queries sim state
through the same read handle as the base elfcyclopedia server.

**Draft:** `docs/drafts/elfcyclopedia_civs.md` §Elfcyclopedia (Web-Based)

**Blocked by:** F-civ-knowledge
**Related:** F-civ-knowledge

#### F-elfcyclopedia-srv — Embedded localhost HTTP elfcyclopedia server
**Status:** Done

Embedded HTTP server on localhost (127.0.0.1, configurable port) serving
the elfcyclopedia as HTML pages in the player's web browser. Server runs
on a dedicated thread with read-only access to sim state. Species
bestiary from JSON data file. In-game toolbar button shows URL and opens
browser on click. Server-rendered HTML templates, no JavaScript required.
Auto-refresh via meta tag. Independent of all sim/worldgen features.

**Draft:** `docs/drafts/elfcyclopedia_civs.md` §Elfcyclopedia (Web-Based)

#### F-fruit-sprite-ui — Fruit sprites in inventory/logistics/selection UI
**Status:** Todo

Show the procedural fruit sprite (from SpriteFactory.create_fruit) next to
fruit items wherever they appear in the UI: creature inventory panels,
ground pile tooltips, logistics want lists, greenhouse species picker,
kitchen/workshop recipe ingredient lists, and any other surface that
displays fruit item names. Currently fruit items show as text only
(Vaelith name + shape noun). The sprite textures are already cached
per species in tree_renderer.gd; this feature needs to make them
accessible to other UI scripts and add img/TextureRect elements
alongside item text labels.

**Related:** F-fruit-sprites, F-fruit-variety

#### F-fruit-sprites — Procedural fruit sprites
**Status:** Done · **Phase:** 7

Procedural billboarded sprites for fruit species, generated from species
appearance data (shape, colors, size). Used on fruit voxels in the world
(replacing plain colored blocks) and as icons in inventory/logistics UI.
Each species gets a unique sprite derived from its FruitAppearance struct.
Follows the existing sprite_factory.gd pattern used for creature sprites.

**Related:** F-fruit-sprite-ui, F-fruit-variety, F-rust-sprites

#### F-godot-setup — Godot 4 project setup
**Status:** Done · **Phase:** 0 · **Refs:** §3

Godot 4 project with GDExtension configuration.

#### F-keybind-help — Keyboard shortcuts help overlay
**Status:** Done · **Phase:** 2

A help panel (toggled via toolbar button or ? key) showing all keyboard
shortcuts and mouse controls: camera orbit/zoom/pan, speed controls, ESC
chain, construction mode keys, etc. Pure GDScript UI — no sim changes.

**Related:** F-build-queue-ui, F-controls-config, F-controls-config-C

#### F-lod-sprites — LOD sprites (chibi / detailed)
**Status:** Todo · **Phase:** 8+ · **Refs:** §24

High-detail anime sprites at close zoom, low-detail chibi at far zoom.
Deferred until camera zoom range demands it.

#### F-main-menu — Main menu UI
**Status:** Done · **Refs:** §26

Main menu with New Game, Load, and Quit buttons.

#### F-minimap — Minimap with tree silhouette and creature positions
**Status:** Todo · **Phase:** 2

A small top-down minimap in a screen corner showing the tree silhouette,
creature positions (colored dots by species), construction sites, and the
camera's current viewport frustum. Clicking the minimap jumps the camera
to that position. Pure rendering/UI — reads existing sim data.

**Related:** F-zlevel-vis

#### F-modifier-keybinds — Modifier key combinations in bindings
**Status:** Todo · **Phase:** 2

Support modifier key combinations (Ctrl+X, Shift+Click, etc.) in
ControlsConfig bindings and the rebinding UI. Data model already
supports modifiers array from F-controls-config-A; this feature
adds the UI for capturing and displaying modifier combos.

Depends on F-controls-config-C (rebinding UI must exist first).

**Blocked by:** F-controls-config-C
**Related:** F-controls-config

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

#### F-rts-selection — RTS box selection and multi-creature commands
**Status:** In Progress

Godot-side UI work. Box selection (click-drag rectangle) in selection_controller.gd. Multi-creature selection state (client-local, not sim state). Group info panel (portraits/icons, count by species). Right-click context commands: ground → GoTo, hostile creature → AttackCreature. Attack-move hotkey (A + click). All commands dispatch SimAction variants for each selected creature. Selection state not saved, not synced in multiplayer.

**Done so far:** Stable creature ID addressing — replaced fragile (species, index) with CreatureId UUID strings throughout the full pipeline (SimBridge, selection_controller, creature_info_panel, main.gd, tooltip_controller, units_panel, task_panel). New SimBridge APIs: `get_creature_positions_with_ids()`, `get_creature_info_by_id()`, `is_hostile_by_id()`. Box selection with click-drag rectangle overlay (CanvasLayer ColorRect, screen-space projection). Multi-creature selection state (Array of creature IDs). Shift+click/drag for additive selection toggle. Group info panel: scrollable list of selected creatures with sprites, names, species, and activity; clicking a row selects just that creature; mutual exclusion with single-creature info panel. Box select filters to player-civ creatures (RTS convention). Dead creature pruning from selection (single and multi). Right-click context commands: attack hostile, move-to friendly/ground (ported from main's species/index API to UUID-based, works for multi-select).

**Not yet done:** Attack-move hotkey. Visual highlight on selected creature sprites. Selection count indicator.

**Draft:** docs/drafts/combat_military.md (§2)

**Blocks:** F-combat

#### F-rust-mesh-complex — Rust mesh gen for buildings/ladders
**Status:** Todo · **Phase:** 3

Move building interior and ladder geometry generation from GDScript renderers
(`building_renderer.gd`, `ladder_renderer.gd`) into the Rust chunk mesh system.
These use oriented face quads and thin panels rather than full cubes, so they
need special handling in `mesh_gen.rs`.

#### F-rust-mesh-gen — Rust-side voxel mesh gen with face culling
**Status:** Done · **Phase:** 2

Move voxel mesh generation from GDScript MultiMesh to Rust with per-face
culling. Chunk-based (16x16x16) with caching and incremental dirty updates.
Opaque faces between adjacent solid voxels are culled, reducing triangle count.
Covers tree voxels (Trunk, Branch, Root, Leaf, Dirt) and construction voxels
(GrownPlatform, GrownWall, GrownStairs, Bridge). Fruit uses separate
billboard Sprite3D rendering with per-species procedural textures.

#### F-rust-sprites — Investigate moving sprite generation to Rust
**Status:** Todo

Investigate moving procedural sprite generation from GDScript
(sprite_factory.gd) into Rust. Currently all creature and fruit sprites
are drawn pixel-by-pixel in GDScript; fruit sprites were duplicated in
Rust for the elfcyclopedia server. Moving to Rust would eliminate the
duplication and keep rendering logic closer to the sim data.

Questions to resolve: Which crate should own it? (sim is Godot-free,
gdext is a thin bridge — may need a new crate or a non-Godot module in
gdext.) How to pass pixel data to Godot efficiently? (PackedByteArray →
Image → ImageTexture, or gdext Image bindings.) Impact on iteration
speed vs compile times. Whether the existing sprite_factory.gd drawing
helpers (circle, ellipse, rect) are easy to port. Scope: all 10 creature
species + fruit, ~1500 lines of GDScript drawing code.

**Related:** F-fruit-sprites

#### F-select-struct — Selectable structures with interaction UI
**Status:** Done · **Phase:** 3

Click-to-select completed structures (platforms, buildings, ladders, etc.)
with an info panel showing structure type, dimensions, health/stress, and
structure-specific actions. Extends the existing creature selection system
to handle structure entities. Foundation for per-structure interaction like
rope ladder furling, building furnishing, and structure demolition.

**Related:** F-demolish, F-elf-assign, F-rope-retract, F-selection, F-struct-names, F-structure-reg

#### F-selection — Click-to-select creatures
**Status:** Done · **Refs:** §26

Ray-based selection with billboard sprite hit detection. ESC to deselect.
Input precedence chain with placement and pause systems.

**Related:** F-creature-tooltip, F-select-struct

#### F-sim-speed — Simulation speed controls UI
**Status:** Done · **Phase:** 4

Pause/1x/2x/5x speed controls for the simulation. The sim architecture
already supports variable tick rates (time-based accumulator in `main.gd`).
This adds UI buttons and keyboard shortcuts (e.g., Space for pause, +/-
for speed) to control the tick multiplier. Essential for both slow
observation and fast-forwarding through idle periods. Important note: the
speed must be tracked in _rust_, and the speed change must be sync'd
across multiplayer, in the same way that user actions are sync'd.

**Related:** F-event-loop

#### F-spawn-toolbar — Spawn toolbar and placement UI
**Status:** Done · **Refs:** §26

Toolbar with creature spawn buttons and keyboard shortcuts. Placement
controller handles click-to-place with nav node highlighting.

**Related:** F-debug-menu

#### F-status-bar — Persistent status bar (population, idle count, active tasks)
**Status:** Done · **Phase:** 2

A persistent bar (top or bottom of screen) showing at-a-glance stats:
total population, idle elf count, active task count, current sim speed.
DF-style "7 Elves, 2 Idle" display. Reads existing bridge data each
frame — no sim changes needed.

**Related:** F-creature-tooltip, F-notifications

#### F-structure-reg — Completed structure registry + UI panel
**Status:** Done · **Phase:** 2

Registry of completed structures in the sim (`SimState.structures`) with a
browsable UI panel. Tracks all build types (Platform, Bridge, Stairs, Wall,
Enclosure, Building) with sequential IDs (#0, #1, ...) and bounding box
for zoom-to-location.

**New files:** `structure_list_panel.gd`

**Related:** F-building, F-construction, F-select-struct, F-struct-names

#### F-task-panel-groups — Task panel grouped by origin + creature names
**Status:** Done · **Phase:** 2

Group task panel cards into three sections by origin: Player Directives
(build, goto, furnish), Automated Management (future), and Autonomous
Decisions (eat, sleep). Show creature Vaelith names on assignee zoom
buttons instead of hex IDs. Adds `TaskOrigin` enum to `task.rs` with
`PlayerDirected`, `Autonomous`, and `Automated` variants.

**Modified files:** `task.rs`, `sim.rs`, `sim_bridge.rs`, `task_panel.gd`

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

**Related:** F-bldg-transparency, F-minimap

### Infrastructure & Multiplayer

#### B-tab-serde-tests — Fix tabulosity test compilation under feature unification
**Status:** Done

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

**Related:** F-sim-speed

#### F-fog-of-war — Visibility via tree and root network
**Status:** Todo · **Phase:** 8+ · **Refs:** §17

World hidden except where observed by elves or sensed through tree/root
network. Strongest near trunk, weaker at root edges, absent beyond.

**Related:** F-combat, F-root-network

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
**Status:** Done · **Phase:** 8+ · **Refs:** §4

Periodic sim-state checksums sent via the relay to detect desync between
clients. `SimState::state_checksum()` serializes state to JSON and hashes with
FNV-1a 64-bit (`checksum.rs`). `NetClient::send_checksum()` sends the hash to
the relay, which compares per-player hashes and broadcasts `DesyncDetected` on
mismatch. The GDExtension bridge (`sim_bridge.rs`) sends checksums
automatically every `CHECKSUM_INTERVAL_TICKS` (1000 ticks = 1 sim-second)
after applying turns.

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
**Status:** Done · **Phase:** 8+ · **Refs:** §4

Allow players to join a multiplayer session that has already started.
The relay requests a full sim state snapshot from the host, pauses turn
flushing during the transfer, and forwards it to the joining player.
Pending joiner is excluded from checksum comparisons. Only one mid-game
join can be in flight at a time.

**Related:** F-mp-reconnect, F-multiplayer, F-save-load

#### F-mp-reconnect — Multiplayer reconnection after disconnect
**Status:** Todo · **Phase:** 8+ · **Refs:** §4

Graceful handling of temporary disconnections in multiplayer. When a client
disconnects, preserve their player slot for a timeout period and allow
reconnection with state catchup (replaying missed turns or requesting a
snapshot). Not yet designed in detail.

**Related:** F-mp-mid-join, F-multiplayer

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

**Related:** F-mp-chat, F-mp-checksums, F-mp-integ-test, F-mp-mid-join, F-mp-reconnect, F-multi-tree, F-save-load, F-session-sm

#### F-save-load — Save/load to JSON with versioning
**Status:** Done · **Phase:** 2 · **Refs:** §4, §5

Full sim state serialized to JSON in `user://saves/`. Save versioning
for schema migration.

**Related:** F-mp-mid-join, F-multiplayer, F-tab-schema-evol, F-tab-schema-ver

#### F-serde — Serialization for all sim types
**Status:** Done · **Phase:** 0 · **Refs:** §5

All sim types derive Serialize/Deserialize for save/load and future
network sync.

#### F-session-sm — Formal session & sim state machines
**Status:** Done · **Phase:** 2 · **Refs:** §4
**Draft:** `docs/drafts/session_state_machine_v4.md`

Formalized the multiplayer session and simulator into explicit state machines.
GameSession owns the sim and all session metadata (lobby, pause, speed,
players). Single-player and multiplayer both use the same GameSession, differing
only in whether messages are relayed. LocalRelay handles SP tick pacing.
GDScript simplified to call `bridge.frame_update(delta)`. SimSpeed removed
from sim crate (speed is session-layer only). Initial creatures spawn from
GameConfig.

**Related:** F-multiplayer

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

#### F-sim-commands — SimCommand pipeline
**Status:** Done · **Phase:** 1 · **Refs:** §4

All mutations go through SimCommand for determinism and future multiplayer.

#### F-sim-db-impl — Tabulosity typed in-memory relational store
**Status:** Done
**Design:** `docs/tabulosity.md` (old drafts: `docs/drafts/sim_db_v9.md`, `docs/drafts/tabulosity_advanced_indexes_v5.md`)
**Schema:** `docs/drafts/sim_db_schema_v4.md`

Typed in-memory relational database library (`tabulosity` + `tabulosity_derive`)
for the sim crate. Derive macros: `Bounded` (newtype min/max), `Table` (companion
struct with BTreeMap storage, secondary indexes, CRUD), `Database` (FK validation
on insert/update/upsert, restrict-on-delete, serde support with cross-table
validation collecting all errors). Feature-gated serde: `Serialize`/`Deserialize`
generated by both Table and Database derives. Integrated into `elven_canopy_sim`
as `SimDb` (16 tables) — see F-sim-tab-migrate.

**Related:** F-tab-auto-pk, F-tab-cascade-del, F-tab-change-track, F-tab-compound-idx, F-tab-filter-idx, F-tab-joins, F-tab-modify-unchk, F-tab-query-opts, F-tab-schema-evol, F-tab-schema-ver, F-tab-unique-idx

#### F-sim-tab-migrate — Migrate sim entity storage to tabulosity SimDb
**Status:** Done

#### F-tab-auto-pk — Auto-generated primary keys
**Status:** Done

`#[primary_key(auto_increment)]` with a monotonic counter so callers don't need to generate IDs manually. Table gets `insert_auto_no_fk()` and `next_id()`. Database gets `insert_{singular}_auto()` with FK validation. Serde serializes auto tables as `{"next_id": N, "rows": [...]}` with defensive correction on deserialize.

**Related:** F-sim-db-impl

#### F-tab-cascade-del — Cascade/nullify on delete
**Status:** Done

`on_delete cascade` or `on_delete nullify` in the `fks()` syntax, extending
the current restrict-on-delete behavior. Cascade removes dependent rows;
nullify sets the FK field to `None`. Medium complexity.

**Related:** F-sim-db-impl

#### F-tab-change-track — Change tracking (insert/update/delete diffs)
**Status:** Todo

Tables emit insert/update/delete diffs per tick, enabling event-driven
rendering. The rendering layer can subscribe to changes rather than polling
the full table each frame. Medium complexity.

**Related:** F-sim-db-impl

#### F-tab-compound-idx — Compound indexes with prefix queries
**Status:** Done
**Design:** `docs/tabulosity.md` (old draft: `docs/drafts/tabulosity_advanced_indexes_v5.md`)

`BTreeSet<(F1, F2, ..., PK)>` compound indexes supporting prefix queries
(e.g., query by first field, or first two fields). Unified `#[index(...)]`
attribute with `IntoQuery` trait for ergonomic queries. Uses tracked bounds
(runtime min/max) instead of `Bounded` trait, enabling `String` PKs and
index fields. High complexity due to derive macro codegen for arbitrary
field tuples.

**Related:** F-sim-db-impl, F-tab-filter-idx

#### F-tab-filter-idx — Filtered/partial indexes
**Status:** Done
**Design:** `docs/tabulosity.md` (old draft: `docs/drafts/tabulosity_advanced_indexes_v5.md`)

Index only rows matching a predicate (e.g., only active tasks). Composes
with compound indexes via unified `#[index(name, fields, filter)]`
attribute. Four-case update maintenance handles filter result transitions.
High complexity.

**Related:** F-sim-db-impl, F-tab-compound-idx

#### F-tab-joins — Join iterators across tables
**Status:** Todo

`db.tasks.join_assignee()` returns an iterator of `(&Task, &Creature)`,
following FK relationships. High complexity due to lifetime management
and derive macro codegen.

**Related:** F-sim-db-impl

#### F-tab-modify-unchk — Closure-based row mutation (modify_unchecked)
**Status:** Done
**Draft:** `docs/drafts/modify_unchecked_v1.md`

Closure-based in-place mutation for tabulosity tables. Three methods:
`modify_unchecked` (single row by PK), `modify_unchecked_range` (PK range
via `BTreeMap::range_mut`), and `modify_unchecked_all` (sugar for full
range). All bypass index maintenance and FK validation. In debug builds,
each snapshots PK + indexed fields before the closure and asserts they are
unchanged after; in release builds, zero overhead beyond the map lookup +
closure call. Database-level wrappers delegate to the table methods.

**Related:** F-sim-db-impl, F-tab-query-opts

#### F-tab-parent-pk — Tabulosity: allow parent PK as child table PK for 1:1 relations
**Status:** Todo

#### F-tab-query-opts — Query options struct for index queries
**Status:** Done

`QueryOpts` struct passed as a required parameter to all index query methods
(`by_*`, `iter_by_*`, `count_by_*`). Controls ordering (ascending/descending
via `BTreeSet::range().rev()`), offset (skip first N results), and mutable
iteration (`modify_each_by_*` with debug-build index-integrity checks). The
struct is trivially small (enum + usize) with a `Default` impl; the compiler
inlines and eliminates default-path branches at even `opt-level = 1`. Avoids a
full query builder pattern for performance — limits are handled via iterator
combinators (`.take(n)`).

**Related:** F-sim-db-impl, F-tab-modify-unchk

#### F-tab-schema-evol — Schema evolution: custom migrations
**Status:** Todo

Custom migration support for breaking schema changes across save-file versions.
Two tiers of migration code: (1) typed post-deserialize migrations that operate
on the current Rust structs (for simple transforms like populating new fields from
old data), and (2) low-level migrations that operate on a format-agnostic
SchemaSnapshot (untyped table-of-rows representation) for structural changes that
can't be expressed in current types (table renames, merges, splits, field moves
between tables). The SchemaSnapshot path is slower and only used when a migration
explicitly requires it; otherwise skipped. High complexity — defer until closer
to beta. **Draft:** `docs/drafts/schema_migrations.md`

**Related:** F-save-load, F-sim-db-impl, F-tab-schema-ver

#### F-tab-schema-ver — Schema versioning fundamentals
**Status:** Done

Schema versioning fundamentals for tabulosity: (1) version number on Database
(included in serialized output, checked on deserialization), (2) missing tables
deserialize as empty instead of erroring, (3) establish convention that new
fields on existing row types use `#[serde(default)]`. These changes make additive
schema changes (new tables, new columns with defaults) work without any migration
code.

**Related:** F-save-load, F-sim-db-impl, F-tab-schema-evol

#### F-tab-unique-idx — Unique index enforcement
**Status:** Done

`#[indexed(unique)]` enforced on insert and update — returns an error if
a duplicate value is found. Low complexity, builds on existing index
infrastructure.

**Related:** F-sim-db-impl

#### F-tree-gen — Procedural tree generation (trunk+branches)
**Status:** Done · **Phase:** 1 · **Refs:** §8

Trunk is first branch — all segments use same growth algorithm with
different params. Cross-section bridging ensures 6-connectivity. Voxel
type priority prevents overwrites.

#### F-weather — Weather within seasons
**Status:** Todo · **Refs:** §27

Rain, wind, storms within seasons. Could affect mood, fire spread, and
construction difficulty. Open design question (§27).

**Related:** F-fire-ecology, F-seasons

