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
[~] B-qem-deformation      QEM decimation visual artifacts
[~] F-creature-skills      Creature skill system (17 universal skills with path-gated advancement)
[~] F-enemy-ai             Hostile creature AI (goblin/orc/troll behavior)
[~] F-face-tint            Directional face tinting by normal (top warm, bottom cool)
[~] F-fruit-variety        Procedural fruit variety and processing
[~] F-multiplayer          Relay-coordinator multiplayer networking
[~] F-notifications        Player-visible event notifications
[~] F-parallel-dedup       Radix-partitioned parallel dedup (elven_canopy_utils)
[~] F-path-ui              Path management UI and notifications
[~] F-taming               Tame neutral creatures via Scout-path elves
```

### Todo

```
[ ] B-assembly-timeout     Activity assembly timeout not enforced
[ ] B-chamfer-nonmfld      Chamfer produces non-manifold edges for diagonally-adjacent voxels
[ ] B-dead-owner-items     Dead creature items retain ownership, becoming invisible to all systems
[ ] B-doubletap-groups     Double-tap selection group recall inconsistently triggers camera center
[ ] B-flying-flee          Flying creatures flee by random wander instead of directionally
[ ] B-fragile-tests        Audit and harden tests against PRNG stream shifts and worldgen changes
[ ] B-start-paused-ui      start_paused_on_load UI desync and missing new-game support
[ ] F-ability-hotkeys      RTS-style bindable ability hotkeys on creatures
[ ] F-activation-revamp    Replace manual event scheduling with automatic reactivation
[ ] F-adventure-mode       Control individual elf (RPG-like)
[ ] F-aggro-fauna          Neutral fauna with aggro triggers
[ ] F-ai-sprites           AI-generated sprite art pipeline
[ ] F-anatomy              DF-style hit location anatomy system
[ ] F-animal-bonds         Elf-animal relationships and bonded pairs
[ ] F-animal-breeding      Breed tamed animals
[ ] F-animal-husbandry     Tamed animal needs and care
[ ] F-apprentice           Skill transfer via proximity
[ ] F-async-sim            Async sim: decouple sim thread from render thread via delta channel
[ ] F-audio-sampled        Sampled vocal syllables from conlang
[ ] F-audio-vocal          Continuous vocal synthesis
[ ] F-batch-blueprint      Batch blueprinting with dependency order
[ ] F-batch-construct      Batch construction mode with ensemble validation
[ ] F-batch-craft          Workstation-driven batch crafting with time discount
[ ] F-binding-conflicts    Binding conflict detection
[ ] F-bldg-concert         Concert hall
[ ] F-bldg-dining          Dining hall
[ ] F-bldg-library         Magic learning building (library/spire)
[ ] F-bldg-storehouse      Storehouse (item storage)
[ ] F-blueprint-mode       Layer-based blueprint selection UI
[ ] F-boundary-decim       Mesh decimation at chunk boundaries
[ ] F-branch-growth        Grow branches for photosynthesis/fruit
[ ] F-bridges              Bridge construction between tree parts
[ ] F-buff-system          Generic timed stat modifier buffs on creatures
[ ] F-build-queue-ui       Construction queue/progress UI
[ ] F-building-door        Player-controlled building door orientation
[ ] F-cascade-fail         Cascading structural failure
[ ] F-cavalry              Mount tamed creatures as cavalry
[ ] F-choir-build          Choir-based construction singing
[ ] F-choir-harmony        Ensemble harmony in construction singing
[ ] F-civ-knowledge        Civilization knowledge system (fruit tiers, discovery)
[ ] F-civ-pets             Non-elf civ members and pets
[ ] F-cloak-slot           Cloak/cape equipment slot
[ ] F-combat               Combat and invader threat system
[ ] F-combat-singing       Combat singing buffs and musical instrument bands
[ ] F-conjured-creatures   Temporary creature spawning with lifetime and auto-despawn
[ ] F-controls-config      Centralized controls config with rebinding and persistence
[ ] F-controls-config-A    ControlsConfig autoload and handler migration
[ ] F-controls-config-B    Controls persistence and sensitivity settings
[ ] F-controls-config-C    Controls settings screen with rebinding UI
[ ] F-creature-control     Temporary allegiance change and AI override
[ ] F-cultural-drift       Inter-tree cultural divergence
[ ] F-dance-choreo         Refine dance figure choreography
[ ] F-dance-movespeed      Dance movement paced to creature walk speed
[ ] F-dance-scaling        Support more than 3 dancers
[ ] F-dance-self-org       Elves self-organize dances
[ ] F-dappled-light        Dappled light effect via scrolling noise on ground shader
[ ] F-day-night            Day/night cycle and pacing
[ ] F-day-night-color      Color grading shift by time of day
[ ] F-defense-struct       Defensive structures (ballista, wards)
[ ] F-demolish             Structure demolition
[ ] F-distance-fog         Depth-based atmospheric fog/haze
[ ] F-dwarf-fort-gen       Underground dwarf fortress generation
[ ] F-dye-application      Apply dye to equipment at workshop
[ ] F-dye-mixing           Dye color mixing recipes
[ ] F-dye-palette          Named color palette system for dyes
[ ] F-edge-outline         Edge highlighting shader (depth/normal discontinuity)
[ ] F-edge-scroll          Configurable edge scrolling (pan, rotate, or off)
[ ] F-elf-assign           Elf-to-building assignment UI
[ ] F-elf-leave            Devastated elves permanently leave
[ ] F-elfcyclopedia-know   Elfcyclopedia civ/fruit knowledge pages
[ ] F-emotions             Multi-dimensional emotional state
[ ] F-festivals            Festivals and community ceremonies
[ ] F-ff-vertical-arc      Vertical arc awareness for friendly-fire checks
[ ] F-fire-advanced        Heat accumulation and ignition thresholds
[ ] F-fire-basic           Fire spread and voxel destruction
[ ] F-fire-ecology         Fire as ecological force, firefighting
[ ] F-fire-structure       Fire x structural integrity cascades
[ ] F-fog-of-war           Visibility via tree and root network
[ ] F-follow-multi         Camera zoom-to and follow for multi-selections
[ ] F-food-chain           Food production/distribution pipeline
[ ] F-food-quality-mood    Food quality affects dining mood boost
[ ] F-forest-ecology       Forest floor ecology (flora, fauna, foraging)
[ ] F-forest-radar         Forest awareness radar (world map detection)
[ ] F-fruit-pigments       More natural fruit pigment colors (secondaries on fruit parts)
[ ] F-fruit-prod           Basic fruit production and harvesting
[ ] F-fruit-sprite-ui      Fruit sprites in inventory/logistics/selection UI
[ ] F-funeral-rites        Funeral rites and mourning
[ ] F-greenhouse-revamp    Greenhouse planter growth cycle and pluck tasks
[ ] F-group-chat           Group chat social activity
[ ] F-hedonic-adapt        Asymmetric hedonic adaptation
[ ] F-herbalism            Herbalism and alchemy
[ ] F-herding              Manage animal groups with pens and grazing areas
[ ] F-infra-decay          Infrastructure decay with automated maintenance
[ ] F-insect-husbandry     Beekeeping and insect husbandry
[ ] F-instinctual-flee     Instinctual flee thresholds (species-level fear overrides)
[ ] F-jobs                 Elf job/role specialization
[ ] F-labor-panel          DF/Rimworld-style labor assignment UI
[ ] F-leaf-sway            Foliage vertex sway shader (wind simulation)
[ ] F-leaf-tuning          Leaf visual fine-tuning and interior decisions
[ ] F-lod-sprites          LOD sprites (chibi / detailed)
[ ] F-los-tuning           Line-of-sight tuning (terrain tolerance, tall creature bonus)
[ ] F-magic-items          Magic item personalities and crafting
[ ] F-mana-mood            Mana generation tied to elf mood
[ ] F-mana-scale           Rescale mana to human-readable values and ticks_per_mp_regen
[ ] F-mana-transfer        Tree-to-elf mana transfer
[ ] F-mass-conserve        Wood mass tracking and conservation
[ ] F-megachunk            MegaChunk spatial hierarchy with draw distance and frustum culling
[ ] F-mesh-cache-lru       LRU cache for chunk meshes at different Y cutoffs
[ ] F-mesh-par             Parallel off-main-thread chunk mesh generation with camera-priority
[ ] F-military-campaign    Send elves on world expeditions
[ ] F-military-org         Squad management and organization
[ ] F-mobile-support       Mobile/touch platform support
[ ] F-modding              Scripting layer for modding support
[ ] F-modifier-keybinds    Modifier key combinations in bindings
[ ] F-mouse-elevation      Ctrl+mouse wheel to move camera elevation
[ ] F-mp-chat              Multiplayer in-game chat
[ ] F-mp-reconnect         Multiplayer reconnection after disconnect
[ ] F-multi-tree           NPC trees with personalities
[ ] F-narrative-log        Events and narrative log
[ ] F-night-predators      Nocturnal predators
[ ] F-pack-animals         Beast-of-burden hauling for heavy loads and caravans
[ ] F-partial-struct       Structural checks on incomplete builds
[ ] F-path-civil           Civil path definitions and organic self-assignment
[ ] F-path-combat          Combat path definitions and player assignment
[ ] F-path-residue         Skill residue from past paths
[ ] F-path-specialize      Path specialization branching and prerequisites
[ ] F-path-stuck           Deep commitment personality drift and refusal
[ ] F-patrol               Patrol command for military groups
[ ] F-personality          Personality axes affecting behavior
[ ] F-phased-archery       Phased archery (nock/draw/loose) with skill-gated mobility
[ ] F-poetry-reading       Social gatherings and poetry readings
[ ] F-population           Natural population growth/immigration
[ ] F-proc-poetry          Procedural poetry via simulated annealing
[ ] F-quality-filters      Quality filters for logistics wants and active recipes
[ ] F-raid-detection       Raid detection gating and stealth spawning
[ ] F-raid-polish          Raid polish: military groups, provisions for long treks
[ ] F-recipe-any-mat       Any-material recipe parameter support
[ ] F-rescue               Rescue and stabilize incapacitated creatures
[ ] F-root-network         Root network expansion and diplomacy
[ ] F-rope-retract         Retractable rope ladders (furl/unfurl)
[ ] F-round-building       Round/circular building construction
[ ] F-rust-mesh-complex    Rust mesh gen for buildings/ladders
[ ] F-sculptures           Decorative sculptures
[ ] F-seasons              Seasonal visual and gameplay effects
[ ] F-selection-bar        Bottom-of-screen selection bar (SC2-style)
[ ] F-settlement-gen       Procedural NPC settlement generation
[ ] F-skirmish             Ranged skirmish/kite behavior (shoot-retreat loop)
[ ] F-slow-eating          Slow eating with interruptible consumption and partial restoration
[ ] F-smooth-perf          Smooth mesh performance optimization
[ ] F-smooth-ycutoff       Post-smoothing Y-cutoff with cap faces
[ ] F-social-graph         Relationships and social contagion
[ ] F-soul-mech            Death, soul passage, resurrection
[ ] F-sound-effects        Basic ambient and action sound effects
[ ] F-spell-berserk        Berserk frenzy buff (damage up, uncontrollable)
[ ] F-spell-blink          Short-range teleport spell
[ ] F-spell-cloak          Invisibility spell on self or nearby allies
[ ] F-spell-ench-arrow     Enchanted arrow shot with mana cost and hit effects
[ ] F-spell-gust           Gust AoE knockback cone spell
[ ] F-spell-ice-shard      Ice Shard ranged magic projectile with autocast
[ ] F-spell-mend           Mend healing spell with autocast healer AI
[ ] F-spell-mind-ctrl      Temporary mind control of enemy creature
[ ] F-spell-rootbind       Rootbind immobilize spell (contested duration)
[ ] F-spell-summon         Conjure temporary allied creature
[ ] F-spell-system         Core spell casting infrastructure (SpellId, commands, mana costs)
[ ] F-spell-thornbriar     Thornbriar zone spell (slow + damage area)
[ ] F-stairs               Stairs and ramps for vertical movement
[ ] F-starvation-rework    Starvation rework: incapacitation interaction and bleed-out
[ ] F-status-effects       Generic creature status effect system
[ ] F-stealth              Camouflage and stealth mechanics
[ ] F-stress-heatmap       Stress visualization in blueprint mode
[ ] F-struct-upgrade       Structure expansion/upgrade
[ ] F-sung-furniture       Sung furniture grown from living wood
[ ] F-tab-change-track     Change tracking (insert/update/delete diffs)
[ ] F-tab-cycle            Tab to cycle focus through units in selection
[ ] F-tab-indexmap-fork    Forked IndexMap with tombstone compaction (alternative to F-tab-ordered-idx)
[ ] F-tab-joins            Join iterators across tables
[ ] F-tab-schema-evol      Schema evolution: custom migrations
[ ] F-tame-aggro           Taming failure can aggro the target animal
[ ] F-task-assign-opt      Event-driven bidirectional task assignment
[ ] F-task-panel-sprites   Creature sprites in tasks panel and activity cards
[ ] F-task-priority        Priority queue and auto-assignment
[ ] F-task-tags            Decouple task eligibility from species via capability tags
[ ] F-terrain-manip        Temporary voxel/zone placement with expiry
[ ] F-test-perf            Test performance audit: per-test timing
[ ] F-traders              Visiting traders from other civs
[ ] F-tree-capacity        Per-tree carrying capacity limits
[ ] F-tree-disease         Tree diseases and parasites
[ ] F-tree-memory          Ancient tree knowledge/vision system
[ ] F-tree-species         Multiple tree species with properties
[ ] F-two-click-build      Two-click construction designation (click start, click end)
[ ] F-undo-designate       Undo last construction designation
[ ] F-unfurnish            Unfurnish/refurnish a building
[ ] F-uplift-tree          Uplift lesser tree into bonded great tree
[ ] F-vaelith-expand       Expand Vaelith language for runtime use
[ ] F-vertical-garden      Vertical gardens on the tree
[ ] F-voxel-ao             Per-vertex ambient occlusion baked into chunk meshes
[ ] F-want-categories      Categorical want specifications (any footwear, any melee weapon)
[ ] F-war-animals          Train tamed creatures for combat
[ ] F-war-magic            War magic (combat spells)
[ ] F-weather              Weather within seasons
[ ] F-wild-grazing         Wild animal herbivorous food cycle
[ ] F-winged-elf           Winged elf species variant with flight-only movement
[ ] F-wireframe-ghost      Wireframe ghost for overlap preview
[ ] F-wood-stats           Wood-type material variation for crafted items
[ ] F-world-boundary       World boundary visualization
[ ] F-world-map            World map view
[ ] F-zone-world           Zone-based world with fidelity partitioning
```

### Done

```
[x] B-dead-enums           Remove dead GrownStairs/Bridge code and add explicit enum discriminants
[x] B-dead-node-panic      Panic on dead nav node in pathfinding
[x] B-dirt-not-pinned      Dirt unpinned in fast structural validator
[x] B-erratic-movement     Erratic/too-fast creature movement after move commands
[x] B-escape-menu          Rename pause_menu to escape_menu and block hotkeys/buttons while it's open
[x] B-first-notification   First notification not displayed (ID 0 skipped by polling cursor)
[x] B-flying-arrow-chase   Flying creatures excluded from arrow-chase
[x] B-flying-tasks         Flying creatures skip task system entirely
[x] B-hostile-detect-nav   detect_hostile_targets panics on flying targets (NavNodeId u32::MAX hack)
[x] B-modifier-hotkeys     Hotkeys should not fire when modifier keys (Ctrl/Shift/Alt) are held
[x] B-music-floats         Excise f32/f64 from music composition for determinism
[x] B-preview-blueprints   Preview treats blueprints as complete
[x] B-raid-spawn           Raiders sometimes spawn inside map instead of at perimeter
[x] B-sim-floats           Remaining f32/f64 in sim logic threaten determinism
[x] B-tab-serde-tests      Fix tabulosity test compilation under feature unification
[x] B-task-civ-filter      Tasks lack civilization-level eligibility filtering
[x] F-ai-test-harness      Remote game control for AI-driven testing (Puppet)
[x] F-alt-deselect         Alt+click to remove from selection
[x] F-armor                Wearable armor system
[x] F-arrow-chase          Enemies chase toward arrow source outside detection range
[x] F-arrow-durability     Arrow durability and recovery
[x] F-attack-evasion       Attack accuracy and evasion with quasi-normal hit rolls
[x] F-attack-move          Attack-move task (walk + fight en route)
[x] F-attack-task          AttackCreature task (player-directed target pursuit)
[x] F-audio-synth          Waveform synthesis for audio rendering
[x] F-bigger-world         Larger playable area
[x] F-bldg-dormitory       Dormitory (unassigned elf sleep)
[x] F-bldg-home            Home (single elf dwelling)
[x] F-bldg-kitchen         Kitchen (cooking from ingredients)
[x] F-bldg-transparency    Toggle building roof visibility (hide/show)
[x] F-bldg-workshop        Craftself's workshop
[x] F-bread                Bread items and elf food management
[x] F-bridge-integ-tests   Integration tests for gdext bridge functions
[x] F-building             Building construction (paper-thin walls)
[x] F-cam-follow           Camera follow mode for creatures
[x] F-capybara             Capybara species
[x] F-carve-holes          Remove material (doors, storage hollows)
[x] F-child-table-pks      Convert child tables to natural compound primary keys
[x] F-civilizations        Procedural civilization generation and diplomacy
[x] F-clothing             Wearable clothing system
[x] F-command-queue        Shift+right-click to queue commands
[x] F-component-recipes    Component-based crafting recipes (bread, thread, bowstring)
[x] F-compound-pk          Compound (multi-column) primary keys
[x] F-config-file          Game config file (user://config.json)
[x] F-config-ui            Settings UI panel (main menu + pause menu)
[x] F-construction         Platform construction (designate/build/cancel)
[x] F-core-types           VoxelCoord, IDs, SimCommand, GameConfig
[x] F-crafting             Non-construction jobs and crafting
[x] F-crate-structure      Two-crate sim/gdext structure
[x] F-creature-actions     Creature action system: typed duration-bearing actions
[x] F-creature-biology     Biological traits for deterministic creature appearance
[x] F-creature-death       Basic creature death (starvation)
[x] F-creature-gravity     Creatures fall when on unsupported voxels
[x] F-creature-info        Creature info panel with follow button
[x] F-creature-stats       Creature stats (str/agi/dex/con/wil/int/per/cha)
[x] F-creature-tooltip     Hover tooltips for world objects
[x] F-dblclick-select      Double-click to select all of same military group
[x] F-debug-menu           Move spawn/summon into debug menu
[x] F-dye-crafting         Dye pressing from pigmented fruit components
[x] F-dynamic-pursuit      Dynamic repathfinding for moving-target tasks
[x] F-elf-acquire          Elf personal item acquisition
[x] F-elf-mana-pool        Per-elf mana pool wired to WIL/INT stats
[x] F-elf-names            Elf name generation from conlang rules
[x] F-elf-needs            Hunger and rest self-direction
[x] F-elf-sprite           Billboard elf sprite rendering
[x] F-elf-weapons          Bows, spears, clubs for elf combat
[x] F-elfcyclopedia-srv    Embedded localhost HTTP elfcyclopedia server
[x] F-emotions-basic       Mood score from thought weights
[x] F-enemy-raids          Enemy civilizations send raids
[x] F-engagement-style     Unified engagement style (species + military group combat tactics)
[x] F-equipment-color      Equipment sprites use item resolved color
[x] F-equipment-sprites    Dynamic sprite customization for equipment
[x] F-event-loop           Event-driven tick loop (priority queue)
[x] F-flee                 Flee behavior for civilians
[x] F-flying-nav           3D flight nav for 1×1 flying creatures (vanilla A*)
[x] F-flying-nav-big       3D flight nav for 2×2×2 flying creatures
[x] F-food-gauge           Creature food gauge with decay
[x] F-footwear-split       Sandals/shoes as footwear, boots as armor
[x] F-friendly-fire        Friendly-fire avoidance for ranged attacks
[x] F-fruit-extraction     Fruit extraction (hulling/separation into components)
[x] F-fruit-naming         Fruit naming overhaul
[x] F-fruit-sprites        Procedural fruit sprites
[x] F-fruit-yields         Fruit yield model overhaul
[x] F-furnish              Building furnishing framework (dormitories)
[x] F-game-session         Game session autoload singleton
[x] F-game-speed-fkeys     Move game speed controls to F1/F2/F3
[x] F-gdext-bridge         gdext compilation and Rust bridge
[x] F-gdscript-tests       GDScript unit tests (GUT or built-in)
[x] F-ghost-above          Hide voxels above camera focus height
[x] F-giant-hornet         Giant hornet hostile flying creature
[x] F-godot-setup          Godot 4 project setup
[x] F-group-activity       Multi-worker activity coordination layer
[x] F-group-dance          Group dance and social singing activities
[x] F-hauling              Item hauling task type
[x] F-hilly-terrain        Hilly forest floor with dirt voxels
[x] F-home-camera          Home key to center camera on tree
[x] F-hostile-detection    Hostile detection and faction logic
[x] F-hostile-species      Goblin, Orc, and Troll species
[x] F-hp-death             HP, VitalStatus, and creature death handling
[x] F-hp-ui                HP bars in creature UI
[x] F-immediate-commands   Immediate command application (zero-tick updates)
[x] F-incapacitation       Incapacitation at 0 HP instead of instant death
[x] F-item-color           Item color system (material-derived and dye override)
[x] F-item-durability      Item durability system (current/max HP on items)
[x] F-item-quality         Item and output quality system
[x] F-items                Items and inventory system
[x] F-keybind-help         Keyboard shortcuts help overlay
[x] F-ladders              Rope/wood ladders as cheap connectors
[x] F-lang-crate           Shared Vaelith language crate
[x] F-large-nav-tolerance  1-voxel height tolerance for large nav
[x] F-large-pathfind       2x2 footprint nav grid
[x] F-lesser-trees         Lesser trees (non-sentient, resource/ecology)
[x] F-logistics            Spatial resource flow (Kanban-style)
[x] F-logistics-filter     Logistics material filter
[x] F-main-menu            Main menu UI
[x] F-mana-depleted-vfx    Visual feedback for mana-depleted work actions
[x] F-mana-grow-recipes    Grow-verb crafting recipes cost mana
[x] F-mana-system          Mana generation, storage, and spending
[x] F-manufacturing        Item schema expansion + workshop manufacturing
[x] F-melee-action         Melee attack action
[x] F-mesh-gen-rle         RLE-aware chunk mesh generation
[x] F-mesh-lod             Mesh level-of-detail for distant chunks
[x] F-military-armor       Military equipment auto-equip and slot validation
[x] F-military-equip       Military group equipment acquisition
[x] F-military-groups      Military group data model and configuration
[x] F-minimap              Minimap with tree silhouette and creature positions
[x] F-mmb-pan              Ctrl+MMB drag to pan camera horizontally
[x] F-mood-system          Mood with escalating consequences
[x] F-move-interp          Smooth creature movement interpolation
[x] F-move-spread          Spread destinations for multi-creature move commands
[x] F-mp-checksums         Multiplayer state checksums for desync detection
[x] F-mp-integ-test        Multiplayer integration test harness
[x] F-mp-mid-join          Mid-game join with state snapshot
[x] F-music-gen            Palestrina-style music generator (standalone)
[x] F-music-runtime        Integrate music generator into game
[x] F-music-use-lang       Migrate music crate to shared lang crate
[x] F-nav-gen-opt          RLE-aware nav graph generation
[x] F-nav-graph            Navigation graph construction
[x] F-nav-incremental      Incremental nav graph updates
[x] F-nav-perf             Optimize nav graph generation performance
[x] F-new-game-ui          New game screen with tree presets
[x] F-no-bp-overlap        Reject overlapping blueprint designations
[x] F-orbital-cam          Orbital camera controller
[x] F-path-core            Elf path system core (Way/Calling/Attunement)
[x] F-pathfinding          A* pathfinding over nav graph
[x] F-pause-menu           In-game pause overlay
[x] F-per-detection        Perception stat modifies hostile detection range
[x] F-pile-gravity         Ground pile gravity and merging
[x] F-placement-ui         Revamp construction placement UX
[x] F-player-identity      Persistent player identity with username
[x] F-preemption           Task priority and preemption system
[x] F-projectiles          Projectile physics system (arrows)
[x] F-recipe-hierarchy     Recipe catalog UI hierarchy and organization
[x] F-recipe-params        Parameterized recipe templates
[x] F-recipe-search        Recipe catalog search/filter
[x] F-recipes              Recipe system for crafting/cooking
[x] F-relay-multi-game     Relay server supports multiple simultaneous games
[x] F-relay-release        Standalone relay server release build
[x] F-rle-voxels           RLE column-based voxel storage
[x] F-rm-floor-extent      Remove floor_extent and ForestFloor layer
[x] F-roof-click-select    Roof click selects building, not elf underneath
[x] F-rts-selection        RTS box selection and multi-creature commands
[x] F-rust-mesh-gen        Rust-side voxel mesh gen with face culling
[x] F-rust-sprites         Move sprite generation to new elven_canopy_sprites crate
[x] F-save-load            Save/load to JSON with versioning
[x] F-select-struct        Selectable structures with interaction UI
[x] F-selection            Click-to-select creatures
[x] F-selection-groups     Ctrl+number selection groups with double-tap camera center
[x] F-serde                Serialization for all sim types
[x] F-session-sm           Formal session & sim state machines
[x] F-shadow-cull          Shadow-only rendering for culled chunks in light direction
[x] F-shared-prng          Shared PRNG crate across all Rust crates
[x] F-shoot-action         Ranged attack action (shooting arrows)
[x] F-sim-commands         SimCommand pipeline
[x] F-sim-db-impl          Tabulosity typed in-memory relational store
[x] F-sim-speed            Simulation speed controls UI
[x] F-sim-tab-migrate      Migrate sim entity storage to tabulosity SimDb
[x] F-spatial-index        Creature spatial index for voxel-level position queries
[x] F-spawn-toolbar        Spawn toolbar and placement UI
[x] F-split-sim            Split monolithic sim.rs into domain sub-modules
[x] F-status-bar           Persistent status bar (population, idle count, active tasks)
[x] F-struct-basic         Basic structural integrity (flood fill)
[x] F-struct-names         User-editable structure names
[x] F-structure-reg        Completed structure registry + UI panel
[x] F-support-struts       Support strut construction
[x] F-tab-auto-pk          Auto-generated primary keys
[x] F-tab-cascade-del      Cascade/nullify on delete
[x] F-tab-compound-idx     Compound indexes with prefix queries
[x] F-tab-filter-idx       Filtered/partial indexes
[x] F-tab-hash-idx         Hash-based indexes in Tabulosity derive macro
[x] F-tab-modify-unchk     Closure-based row mutation (modify_unchecked)
[x] F-tab-nonpk-autoinc    Non-PK auto-increment fields in tabulosity
[x] F-tab-ordered-idx      Deterministic-iteration hash index with tombstone skip
[x] F-tab-parent-pk        Tabulosity: allow parent PK as child table PK for 1:1 relations
[x] F-tab-query-opts       Query options struct for index queries
[x] F-tab-schema-ver       Schema versioning fundamentals
[x] F-tab-unique-idx       Unique index enforcement
[x] F-task-interruption    Unified task interruption and cleanup
[x] F-task-panel-groups    Task panel grouped by origin + creature names
[x] F-task-proximity       Proximity-based task assignment (Dijkstra nearest)
[x] F-textile-crafting     Textile and clothing crafting recipes
[x] F-thoughts             Creature thoughts (DF-style event reactions)
[x] F-tiling-tex           Prime-period tiling textures for bark and ground
[x] F-tree-db              Trees as DB entities with elf-tree bonding
[x] F-tree-gen             Procedural tree generation (trunk+branches)
[x] F-tree-info            Tree stats/info panel
[x] F-tree-overlap         Construction overlap with tree geometry
[x] F-troll-regen          Troll health regeneration over time
[x] F-unified-craft-ui     Unified data-driven building crafting UI
[x] F-visual-smooth        Smooth voxel surface rendering
[x] F-voice-subsets        Variable voice count (SATB subsets)
[x] F-voxel-exclusion      Creatures cannot enter voxels occupied by hostile creatures
[x] F-voxel-fem            Voxel FEM structural analysis
[x] F-voxel-textures       Per-face Perlin noise voxel textures
[x] F-worldgen-framework   Worldgen generator framework
[x] F-wyvern               Wyvern hostile flying creature (2×2×2)
[x] F-zlevel-vis           Z-level visibility (cutaway/toggle)
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

**Related:** F-batch-construct, F-blueprint-mode, F-struct-basic

#### F-batch-construct — Batch construction mode with ensemble validation
**Status:** Todo · **Phase:** 3

Batch construction mode: the player enters a planning mode, creates several
blueprints (e.g., multiple struts holding up a platform with a building on
top), then hits "finalize" to commit the whole ensemble. All blueprints are
validated structurally as a unit, built together, and accompanied by a
single large choral composition (bigger project = grander song). This
enables building structures that can't be constructed piecemeal — e.g., a
platform in open air supported only by struts that aren't yet built.

Each blueprint within the batch still occupies distinct voxels
(F-no-blueprint-overlap enforced within the batch). Dependency ordering
(F-batch-blueprint) determines which pieces are built first.

**Related:** F-batch-blueprint, F-support-struts

#### F-bldg-concert — Concert hall
**Status:** Todo · **Phase:** 4

Furnished building where elves gather for musical performances. Exact
mechanics uncertain — may involve assigned musician elves, scheduled
performances, audience satisfaction, or ties to the music system.
Details to be worked out in a design doc.

**Related:** F-bldg-dining, F-group-dance, F-music-runtime

#### F-bldg-dining — Dining hall
**Status:** Todo · **Phase:** 4

Communal dining building where elves eat meals. Two hunger thresholds:
food_dining_threshold_pct (new, higher) triggers dining hall seek;
food_hunger_threshold_pct (existing, lowered) triggers carried food /
foraging. Dining gives mood boost (AteDining); non-dining eating gives
small penalty (AteAlone). Tables have implicit seats; capacity =
tables × dining_seats_per_table. Food stocked via logistics wants.
Elf reserves seat + food item, paths to table, eats instantly on
arrival. Interrupted elves release reservations, food preserved.

**Draft:** docs/drafts/F-bldg-dining.md

**Related:** F-bldg-concert, F-bldg-kitchen, F-food-chain, F-food-quality-mood, F-slow-eating

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

**Related:** F-bldg-dining, F-bread, F-component-recipes, F-elf-acquire, F-elf-assign, F-food-chain, F-fruit-extraction, F-fruit-variety, F-jobs, F-manufacturing, F-recipes

#### F-bldg-library — Magic learning building (library/spire)
**Status:** Todo

A building where elves learn and research magic. Could be a library, a spire,
or a sacred grove. Elves assigned here study spells (F-war-magic) and
potentially other knowledge systems. Design TBD for research mechanics,
spell unlocking, and training time.

**Related:** F-forest-radar, F-war-magic

#### F-bldg-storehouse — Storehouse (item storage)
**Status:** Todo · **Phase:** 4

Building for storing items and resources. Items placed inside persist
and are accessible to elves for retrieval.

**Related:** F-food-chain, F-logistics

#### F-bldg-workshop — Craftself's workshop
**Status:** Done · **Phase:** 4

Workshop where craftself elves create tools and equipment (bows, spears,
and other gear).

**Related:** F-component-recipes, F-crafting, F-elf-assign, F-elf-weapons, F-jobs, F-recipes

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

**Unblocked by:** F-group-activity
**Related:** F-choir-harmony, F-combat-singing, F-item-quality, F-mana-system, F-music-runtime, F-sung-furniture

#### F-construction — Platform construction (designate/build/cancel)
**Status:** Done · **Phase:** 2 · **Refs:** §11, §12

Basic construction loop: player designates rectangular platforms via the
construction controller UI, sim validates (all voxels Air, at least one
adjacent to solid), creates a blueprint + Build task, elves claim the task
and incrementally materialize voxels. Cancellation reverts placed voxels.
Incremental nav graph updates keep pathfinding current during construction.

**Related:** F-blueprint-mode, F-build-queue-ui, F-building, F-demolish, F-placement-ui, F-round-building, F-struct-upgrade, F-structure-reg, F-tree-overlap, F-two-click-build, F-undo-designate

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

**Related:** F-batch-craft, F-bldg-dormitory, F-building, F-greenhouse-revamp, F-unfurnish

#### F-infra-decay — Infrastructure decay with automated maintenance
**Status:** Todo

Structures and infrastructure (rope bridges, platforms, ladders) gradually
degrade over time and need maintenance. Only worth implementing if
maintenance can be automated — elves with appropriate jobs periodically
inspect and repair without player micromanagement. Neglected structures
eventually become unsafe or collapse. Weather (F-weather) accelerates
decay.

**Related:** F-weather

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

#### F-no-bp-overlap — Reject overlapping blueprint designations
**Status:** Done · **Phase:** 2

Reject construction designations that overlap existing blueprint voxels.
Currently nothing prevents a player from designating a platform on top of
an in-progress strut blueprint (or vice versa), which creates ambiguous
ownership and cancel-restoration conflicts. This constraint simplifies
the construction pipeline: a voxel can only belong to one blueprint at a
time. The player must wait for one build to complete (or cancel it) before
designating over the same voxels.

Future work (F-batch-construction) will allow composing multiple blueprints
as a single batch, but each blueprint within the batch still occupies
distinct voxels.

**Related:** F-support-struts

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

#### F-round-building — Round/circular building construction
**Status:** Todo

Support for round/circular building footprints in the construction system.
Currently all buildings are rectangular. Round buildings would wrap around
the tree trunk more naturally and offer architectural variety.

**Related:** F-construction

#### F-smooth-perf — Smooth mesh performance optimization
**Status:** Todo

Optimize the smooth mesh pipeline for larger draw distances and denser worlds.

Current issues:
- 3 vertices emitted per triangle (per-triangle colors prevent sharing). Could dedup vertices with matching position+normal+color.
- Draw distance temporarily reduced from 100 to 50 voxels.
- Chunk boundary alignment test is O(n²) — slow but only runs in tests.
- Smoothing iterations (when enabled) do ~157K vector evaluations per surface-heavy chunk.

Potential optimizations:
- Vertex dedup with (position, normal, color) key for shared-color triangles (same voxel type).
- Restore draw distance to 100+ after profiling.
- Profile chamfer/smoothing cost per chunk and optimize hot loops.
- Consider reducing smooth mesh border from 2 to 1 voxel if chamfer-only (no smoothing iterations) is the default.

**Related:** F-visual-smooth

#### F-smooth-ycutoff — Post-smoothing Y-cutoff with cap faces
**Status:** Todo

Apply Y-cutoff post-smoothing by clipping the cached smoothed mesh at a Y plane, rather than pre-smoothing (which regenerates the full smooth mesh for every cutoff change, causing lag when scrolling).

Requires: polygon-plane clipping (interpolate new vertices where triangle edges cross the cut), cap face generation (close the open cross-section), per-voxel-material caps (split cap polygon along voxel boundaries for correct vertex colors), and duplicated normals at cut edges (smooth normal for the surface side, (0,1,0) for the cap).

See `docs/drafts/visual_smooth.md` Y-Cutoff Interaction section for full design notes.

**Related:** F-visual-smooth

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

#### F-sung-furniture — Sung furniture grown from living wood
**Status:** Todo

Growing furniture and interior features directly from living wood via
elven singing — chairs, tables, shelves, beds that are part of the tree
itself. Distinct from manufactured furniture (crafted from harvested wood).
Costs mana instead of materials. Higher quality from skilled singers.
Unique to the tree-spirit concept and reinforces the symbiotic theme.

**Related:** F-choir-build, F-item-quality, F-mana-system

#### F-support-struts — Support strut construction
**Status:** Done · **Phase:** 3 · **Refs:** §9
**Draft:** `docs/drafts/support_struts.md`

Diagonal braces and support struts that elves can build between two
designated endpoints. A 3D Bresenham line of solid `Strut` voxels is placed
between the endpoints, replacing natural materials (air, dirt, trunk, leaves)
but not player-built structures. Virtual "rod springs" thread through the
strut for efficient axial load transfer in the structural solver — making
struts genuinely stronger than an ad-hoc staircase of wood voxels. Struts
can cross each other to form trusses. Two-click-then-confirm placement UX
using the height stepper.

Design doc §9: "A diagonal brace under a platform converts bending stress
into compression along the strut, dramatically reducing stress at the
connection."

**Related:** F-batch-construct, F-no-bp-overlap, F-stress-heatmap, F-voxel-fem

#### F-task-priority — Priority queue and auto-assignment
**Status:** Todo · **Phase:** 2 · **Refs:** §11, §15

Task queue with Low/Normal/High/Urgent priorities, auto-assignment of idle
elves to highest-priority available tasks. The Priority enum exists in
types.rs and blueprints carry a priority field, but the Task struct itself
has no priority field. The SetTaskPriority command handler is a TODO stub.
Task assignment (find_available_task in activation.rs) currently uses
Dijkstra-based proximity only (F-task-proximity). Needs: add priority to
Task, wire it into find_available_task sorting, implement SetTaskPriority.

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
**Status:** Done · **Phase:** 2 · **Refs:** §8

Smooth voxel surface rendering — replaces blocky per-face geometry with subdivided and chamfered meshes for all solid opaque voxels and leaf voxels. Sim truth remains a discrete voxel grid; smoothing is purely a rendering concern.

**Draft:** `docs/drafts/visual_smooth.md`


**Implementation (complete):**

**Geometry pipeline** (`smooth_mesh.rs`): Each visible face (solid and leaf) is subdivided into 8 triangles (pie-slice pattern). Vertices deduplicated by grid position. All voxel types share a single unified `SmoothMesh` with per-triangle surface tags (TAG_BARK, TAG_GROUND, TAG_LEAF). Shared vertices at solid/leaf boundaries ensure seamless transitions. Per-vertex `has_solid_face` / `has_leaf_face` flags classify vertices for ordered processing.

**Face culling:**
- Solid↔solid: culled (standard opaque)
- Leaf↔leaf: culled (shell-only rendering)
- Leaf→solid: culled (hidden behind solid)
- Solid→leaf: NOT culled (wood visible through semi-transparent leaves)

**Anchoring:** Face centers always anchored. Faces adjacent to non-solid constructed voxels (BuildingInterior, WoodLadder, RopeLadder) get all vertices anchored. Low-valence boundary vertices anchored. Uses `is_face_center` flag.

**Chamfer (default mode):** Two-phase ordered chamfer: solid vertices first (leaf-only vertices invisible), then leaf-only vertices (seeing chamfered solid as anchors). Saddle-skip heuristic for 4+ anchored neighbors sharing axis coordinates. Direct offset toward anchored neighbor centroid (no normal projection).

**Smoothing (optional, off by default):** 2 Jacobi-style iterations available via debug toggle. Within each iteration: solid vertices processed first, then leaf-only. Candidates that would introduce new saddle points are rejected. Uses Laplacian pointiness metric with squared cost. Default is chamfer-only with flat per-face normals, which currently looks better.

**Chunk boundary handling:** Per-triangle source voxel position `[i32; 3]` for filtering. Vertex normals computed from ALL triangles before filtering. 2-voxel border for context.

**Fragment shaders — Solid** (`smooth_solid.gdshader`): Procedural value noise, triplanar mapping, per-material frequency and Y-scale. Bark anisotropic (y_scale=0.3, freq=16), ground isotropic (freq=8).

**Fragment shaders — Leaf** (`leaf_noise.gdshader`): Procedural noise with ALPHA_SCISSOR_THRESHOLD for boolean alpha (hardware alpha test path). Triplanar mapping. Low-frequency color noise for brightness/hue variation. cull_disabled for both-side rendering.

**Debug tools:** Wireframe toggle and Smoothing ON/OFF toggle in debug toolbar. Smoothing toggle switches between chamfer-only+flat normals and chamfer+smoothing+smooth normals with full mesh rebuild.

**Follow-up items:**
- **F-leaf-tuning:** Leaf shader quality, per-species color, interior rendering decisions.
- **F-smooth-ycutoff:** Post-smoothing Y-cutoff with cap faces.
- **F-smooth-perf:** Draw distance restoration, vertex dedup, profiling.

**AO interaction:** Smooth rendering changes vertex positions. F-voxel-ao must adapt when implemented.

**Unblocked:** F-mesh-lod
**Related:** F-leaf-tuning, F-megachunk, F-mesh-lod, F-smooth-perf, F-smooth-ycutoff, F-voxel-ao

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

**Related:** F-blueprint-mode, F-support-struts

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

**Related:** B-preview-blueprints, F-struct-basic, F-support-struts

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

#### B-erratic-movement — Erratic/too-fast creature movement after move commands
**Status:** Done

After issuing move commands (select creatures, right-click a destination), creature movement becomes erratic and possibly faster than intended. Repro: select one or more creatures, right-click to move them, observe movement behavior.

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

#### F-flying-nav — 3D flight nav for 1×1 flying creatures (vanilla A*)
**Status:** Done · **Phase:** 8+ · **Refs:** §10

3D flight pathfinding for one-voxel-sized flying creatures.
Vanilla A* in 3D space — the sky is mostly open so no nav graph
is needed.

**Unblocked:** F-flying-nav-big, F-giant-hornet, F-winged-elf

#### F-flying-nav-big — 3D flight nav for 2×2×2 flying creatures
**Status:** Done

3D flight pathfinding for 2×2×2 flying creatures. Extends
F-flying-nav with multi-voxel clearance checks.

**Unblocked by:** F-flying-nav
**Unblocked:** F-wyvern

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

#### F-nav-gen-opt — RLE-aware nav graph generation
**Status:** Done

Optimize initial nav graph generation to exploit RLE column storage.
Instead of scanning every voxel O(world_volume), iterate column spans to
find solid→air transitions — each such transition's bottom air voxel is a
walkable surface. This reduces node discovery to O(total_spans) which is
proportional to world surface complexity, not world volume.

Edge creation then checks neighboring columns' span data to find adjacent
walkable surfaces at compatible heights.

For a 1024×128×1024 world with ~3 spans per column, this is ~3M operations
instead of ~134M.

Also replace the flat `Vec<u32>` nav spatial index with a `LookupMap<VoxelCoord, u32>` — point queries only, never iterated in order, so no determinism concern. Eliminates the 4-bytes-per-voxel overhead that would otherwise scale with world volume.

**Unblocked by:** F-rle-voxels
**Unblocked:** F-bigger-world

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

#### B-flying-tasks — Flying creatures skip task system entirely
**Status:** Done

Flying creatures (Hornet, Wyvern, and future winged elves) use a separate
hardcoded activation loop (process_flying_creature_activation) that skips
the task system entirely — no task dispatch, no preemption, no player
commands. This means flying creatures cannot be given GoTo, AttackMove,
or any other task-based orders. The current loop only handles: resolve
action → flee check → detect hostiles → pursue/melee → wander. Unifying
the flying activation loop with the standard task-based activation
pipeline is required before flying creatures can participate in any
task-driven system (player commands, construction, hauling, etc.).

**Unblocked:** B-flying-arrow-chase, F-winged-elf
**Related:** F-arrow-chase

#### F-aggro-fauna — Neutral fauna with aggro triggers
**Status:** Todo

Neutral animals that can become hostile when provoked — territorial
creatures that aggro when you enter their space, mothers defending young,
predators that attack when hungry. Prerequisite for taming having real
risk. Distinct from F-enemy-ai (always-hostile species) — these start
neutral and transition to hostile based on triggers.

**Related:** F-enemy-ai, F-tame-aggro

#### F-animal-breeding — Breed tamed animals
**Status:** Todo

Breed tamed animals to grow herds. Mating pairs, gestation periods,
offspring with inherited traits. Population management — designating
breeding pairs, culling. Enables sustainable animal populations without
continual taming of wild stock.

**Related:** F-animal-husbandry, F-civ-pets

#### F-animal-husbandry — Tamed animal needs and care
**Status:** Todo

Care and maintenance of tamed animals — feeding (supplied food, trough
filling), shelter (stables, pens), health (injury treatment, disease).
Animals have needs that must be met or they become unhappy, unhealthy,
or revert to wild. Elves with Beastcraft skill perform husbandry tasks.

**Blocked by:** F-wild-grazing
**Related:** F-animal-breeding, F-civ-pets

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

#### F-civ-pets — Non-elf civ members and pets
**Status:** Todo

Non-elf creatures that belong to the player's civilization — tamed
animals, companion creatures, working beasts. They live in the
settlement, have needs, and may perform tasks or provide combat support.

Encompasses: elf-animal bonds/relationships, war training, mounting and
cavalry, animal needs (food, shelter), breeding, labor assignment panel
for controlling which creatures do which tasks.

**Blocked by:** F-taming
**Related:** F-animal-bonds, F-animal-breeding, F-animal-husbandry, F-cavalry, F-civilizations, F-herding, F-labor-panel, F-pack-animals, F-task-tags, F-war-animals

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

#### F-creature-biology — Biological traits for deterministic creature appearance
**Status:** Done

Creatures get biological traits stored as sim data — hair color, skin
tone, eye color, body proportions, and other species-specific appearance
parameters. Includes both the database schema (new fields/table in
SimDb) and updating sprite generation in `elven_canopy_sprites` to read
biological traits directly instead of re-deriving appearance from a seed
via hash functions. Makes creature appearance a first-class sim concept
rather than a rendering-side derivation.

This enables future features like heredity, aging, and biological
variation to affect appearance naturally. All sprite generation is fully
determined by biological data — no seed hashing in the sprite crate.

**Unblocked by:** F-rust-sprites
**Related:** F-creature-stats, F-rust-sprites

#### F-creature-death — Basic creature death (starvation)
**Status:** Done · **Phase:** 3 · **Refs:** §13, §15

When a creature's food gauge reaches zero, it dies (vital_status → Dead,
creature row kept in DB). Basic death mechanic without the spiritual
dimension (soul passage, resurrection) covered by F-soul-mech. Needs:
starvation trigger at food=0, death via F-hp-death handler, UI notification.
A prerequisite for food scarcity having real consequences.

Superseded by F-hp-death which covers the general death system including
combat death. F-creature-death covers the starvation trigger specifically.

**Related:** F-elf-needs, F-food-gauge, F-hp-death, F-soul-mech

#### F-creature-gravity — Creatures fall when on unsupported voxels
**Status:** Done

Creatures standing on unsupported voxels (e.g., a deconstructed platform)
fall until they reach a solid surface. Falling causes damage proportional
to distance fallen. Extends F-pile-gravity (which handles items) to
living creatures. Enables Gust spell knockback off platforms as a
meaningful tactical tool. Also improves general simulation fidelity.

**Unblocked:** F-spell-gust
**Related:** F-pile-gravity

#### F-creature-skills — Creature skill system (17 universal skills with path-gated advancement)
**Status:** In Progress · **Phase:** 4

17 universal skills (Striking, Archery, Evasion, Ranging, Herbalism,
Beastcraft, Cuisine, Tailoring, Woodcraft, Alchemy, Singing, Channeling,
Literature, Art, Influence, Culture, Counsel) with exponential 2^(s/100)
scaling reusing the stat multiplier pipeline. All skills universally
available to all elves.

Done: TraitKind variants, info panel Skills tab, SKILL_TRAIT_KINDS array,
SkillConfig (default_skill_cap=100, advancement_decay_base=100),
probabilistic advancement with INT scaling (try_advance_skill), triggers
wired to melee/ranged/harvest/construction/crafting, speed effect via
additive stat+skill through apply_stat_divisor (melee AGI+Striking,
ranged DEX+Archery, harvest DEX+Herbalism, construction CHA+Singing,
crafting DEX+verb skill, furnishing DEX+Woodcraft). Skill cap only
limits learning, not speed benefit.

Remaining: quality effect channel, composite skill applications (e.g.
Singing+Channeling+Art for woodsinging), path-gated caps and advancement
focusing (depends on F-path-core), efficiency/unlocks/failure-rate
channels.

**Draft:** docs/drafts/F-creature-skills.md

**Related:** F-apprentice, F-attack-evasion, F-item-quality, F-path-core, F-taming

#### F-creature-stats — Creature stats (str/agi/dex/con/wil/int/per/cha)
**Status:** Done

Eight per-creature stats rolled at spawn from species-specific distributions
(mean + stdev in SpeciesData): Strength, Agility, Dexterity, Constitution,
Willpower, Intelligence, Perception, Charisma. Integer scale centered on 0
(human baseline), with exponential effect: every +100 doubles the stat's
mechanical intensity. Lookup table maps stat → multiplier in 2^20 fixed-point.
All integer math, no floating point (multiplayer determinism).

Immediate mechanical hooks: Strength → melee damage multiplier and projectile
velocity; Agility → move speed multiplier (and climb speed blend with str);
Constitution → HP multiplier; Dexterity → arrow angular deviation (per-mille
of distance, asymptotically approaching zero). Mental stats (Willpower,
Intelligence, Perception, Charisma) rolled and stored but inert until their
systems exist (mana, crafting quality, singing).

Builds on F-creature-biology: stats are TraitKind variants stored as
TraitValue::Int in the creature_traits table.

**Draft:** docs/drafts/creature_stats.md

**Related:** F-attack-evasion, F-creature-biology, F-elf-mana-pool, F-per-detection, F-phased-archery

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

#### F-giant-hornet — Giant hornet hostile flying creature
**Status:** Done

Giant hornet hostile flying creature (1×1×1). Aggressive AI
targeting elves. Debug button to spawn one, angry at elves.
Requires F-flying-nav.

**Unblocked by:** F-flying-nav
**Related:** F-wyvern

#### F-hostile-species — Goblin, Orc, and Troll species
**Status:** Done

**Related:** F-troll-regen

#### F-hp-death — HP, VitalStatus, and creature death handling
**Status:** Done

Add hp, hp_max, vital_status fields to Creature. VitalStatus enum (Alive, Dead, future: Ghost, SpiritInTree, Undead). hp_max in SpeciesData. Death transition: vital_status → Dead, creature row NOT deleted (supports future states). On death: call unified task interruption (F-task-interruption), drop inventory as ground pile, clear assigned_home, remove from spatial index, emit CreatureDied event, terminate activation/heartbeat chains (no rescheduling). All existing queries that iterate creatures must filter by vital_status == Alive (rendering, task assignment, logistics, heartbeat processing). #[indexed] on vital_status for efficient filtering. #[serde(default)] on new fields for save compat. Supersedes F-creature-death (which only covered starvation — this is the general death system). Debug "kill creature" command for testing.

**Draft:** docs/drafts/combat_military.md (§3)

**Related:** F-creature-death, F-hp-ui, F-incapacitation

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

**Related:** F-friendly-fire, F-phased-archery, F-skirmish, F-spell-ench-arrow

#### F-slow-eating — Slow eating with interruptible consumption and partial restoration
**Status:** Todo · **Phase:** 4

Eating takes time rather than being instant. Food is consumed gradually
over eat_action_ticks (or a new duration field). If interrupted mid-meal,
food item is destroyed and elf gets partial hunger restoration proportional
to progress. Applies to all eating paths (dining hall, carried food,
foraging). Currently all eating is instant on arrival.

**Related:** F-bldg-dining

#### F-starvation-rework — Starvation rework: incapacitation interaction and bleed-out
**Status:** Todo

**Related:** F-incapacitation

#### F-tame-aggro — Taming failure can aggro the target animal
**Status:** Todo

Failed taming attempts can provoke the target animal into aggro,
creating risk/reward tension for taming dangerous species. Chance of
aggro per failed attempt, scaled by species temperament and tamer
skill. Depends on F-aggro-fauna for the neutral-to-hostile transition
mechanic.

**Related:** F-aggro-fauna, F-taming

#### F-taming — Tame neutral creatures via Scout-path elves
**Status:** In Progress

Scout-path elves can tame neutral creatures. Toggle a "Tame" button on
a neutral creature's detail panel to create an open task for any
available Scout. Each attempt is a quick action (few seconds) with a
small success chance based on WIL + CHA + Beastcraft skill; the Scout
keeps trying (and gaining Beastcraft XP) until success. Unchecking the
button cancels the task.

Post-taming: creature gets the player's civ_id, default wander behavior,
appears in a Pets/Animals section of the units panel.

**Draft:** docs/drafts/F-taming.md

**Blocks:** F-civ-pets
**Related:** F-creature-skills, F-tame-aggro, F-task-tags

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

#### F-troll-regen — Troll health regeneration over time
**Status:** Done

Trolls passively regenerate HP over time, making them harder to kill with
sustained low-damage attacks. Encourages concentrated burst damage or
fire-based strategies (if F-fire-basic is implemented). Species-level
config for regen rate.

**Related:** F-hostile-species

#### F-winged-elf — Winged elf species variant with flight-only movement
**Status:** Todo

A kind of elf that can join the player's civilization with wings.
Has a winged sprite variant and only flight speed (no walking or
climbing speed). Requires F-flying-nav.

**Unblocked by:** B-flying-tasks, F-flying-nav

#### F-wyvern — Wyvern hostile flying creature (2×2×2)
**Status:** Done

Wyvern hostile flying creature (2×2×2). Different sprite from the
giant hornet but similar aggressive AI, bigger and more dangerous.
Requires F-flying-nav-big.

**Unblocked by:** F-flying-nav-big
**Related:** F-giant-hornet

### Economy & Logistics

#### B-dead-owner-items — Dead creature items retain ownership, becoming invisible to all systems
**Status:** Todo

#### F-batch-craft — Workstation-driven batch crafting with time discount
**Status:** Todo

Crafting buildings (workshops, kitchens, mills, bakeries) contain
workstation furniture (tables, counters, etc.) whose count scales with
building size. Workstation count governs crafting capacity: more
workstations enable larger batch sizes or more elves working in parallel
on different stations. Some recipes are batch-able in the appropriate
building type — the player configures batch size in the UI (limited by
available workstations). A single batch task reserves that many
workstations and that many multiples of the inputs, and a single elf
completes the batch in less total time than individual tasks would take
(configurable time discount). Examples: milling in a mill, baking in a
bakery.

**Unblocked by:** F-recipe-params
**Related:** F-furnish, F-manufacturing, F-recipes, F-unified-craft-ui

#### F-cloak-slot — Cloak/cape equipment slot
**Status:** Todo

A cloak/cape equipment slot — worn over other clothing/armor. Provides
weather protection (F-weather), stealth bonus (F-stealth), and visual
flair. Material and dye affect appearance. Distinct from armor — cloaks
are utility/fashion, not protection (though enchanted cloaks could be
a thing with F-magic-items).

**Related:** F-clothing, F-magic-items, F-stealth, F-weather

#### F-clothing — Wearable clothing system
**Status:** Done

Creatures can wear clothing items in defined body slots (e.g., head, torso, legs, feet). Clothing is crafted at workshops, stored in inventories, and equipped by creatures. Many details TBD: slot system design (fixed slots vs. layering), how clothing affects mood/comfort/thoughts, crafting recipes and material requirements, visual representation (sprite overlays? color tinting?), clothing durability and wear, species-specific clothing (elf vs. other species body plans), and whether clothing provides any mechanical benefits beyond mood. This is the base wearable-item infrastructure that armor builds on.

**Unblocked:** F-armor, F-equipment-sprites
**Related:** F-cloak-slot, F-footwear-split, F-item-durability

#### F-component-recipes — Component-based crafting recipes (bread, thread, bowstring)
**Status:** Done · **Phase:** 7

Property-based crafting recipes that consume extracted fruit components
(from F-fruit-extraction) to produce useful items. Recipes match on
part properties, not fruit species IDs — "10 units of any starch-bearing
component → 1 loaf of that-species bread" works for any fruit with a
starchy part. Same-species constraint: a single recipe invocation uses
components from one fruit species only.

Initial recipe set:
- Starchy component → mill → flour → bake → bread (food)
- FibrousFine component → spin → thread (crafting material)
- Thread → bowstring (replaces or supplements current workshop recipe)
- FibrousCoarse component → twist → cord → rope/bowstring

Each recipe specifies: input component property requirement, input unit
cost, output ItemKind, output count. Data-driven via GameConfig so
recipes can be tuned without code changes. Kitchen and workshop
buildings each support a subset of recipes based on their type.

Later expansions (not in initial scope): dye pressing from pigmented
parts, fermentation, medicinal brewing, luminous oil distillation,
mana essence refinement.

**Related:** F-bldg-kitchen, F-bldg-workshop, F-fruit-variety, F-mana-grow-recipes, F-recipe-hierarchy, F-recipe-params, F-recipes, F-textile-crafting

#### F-crafting — Non-construction jobs and crafting
**Status:** Done · **Phase:** 8+ · **Refs:** §11

Jobs beyond construction: woodworking, weaving, cooking, enchanting.
Crafting system for tools, furniture, and magical items.

**Unblocked:** F-elf-weapons
**Related:** F-bldg-workshop, F-items, F-magic-items, F-recipes

#### F-dye-application — Apply dye to equipment at workshop
**Status:** Todo

Dyeing recipes and task workflow. A parameterized recipe template
(from F-recipe-params) for each dyeable item kind (Cloth, Tunic,
Leggings, Boots, Hat, Gloves) takes a dye color parameter (palette
entry from F-dye-palette). A creature brings dye and a target item
to a workshop, consumes the dye, and sets the palette color reference
on the target item's stack. Different target items consume different
amounts of dye (configurable via GameConfig).

Depends on F-recipe-params for parameterized recipe templates and
F-dye-palette for the named color system that dye color parameters
reference.

**Blocked by:** F-dye-palette
**Unblocked by:** F-dye-crafting, F-item-color, F-recipe-params
**Related:** F-dye-crafting, F-dye-palette

#### F-dye-crafting — Dye pressing from pigmented fruit components
**Status:** Done · **Phase:** 7

Dye pressing from pigmented fruit components. Fruits with pigmented
parts (which already have a DyeColor field) can be pressed into dye
items via the Press recipe verb. A single ItemKind (Dye) is
differentiated by its color reference. Press recipes are generated
automatically for any pigmented part. Furnishing: Kitchen. Ratio:
100 pigmented component → 100 dye.

**Current state:** Initial implementation complete — Press verb,
ItemKind::Dye, recipe generation, config fields all working. Dye
items currently store raw RGB via dye_color on ItemStack. Needs
retrofit to reference palette color IDs (F-dye-palette) instead of
raw RGB once the palette system exists.

Color mixing tracked in F-dye-mixing. Dyeing recipes (applying dye
to items) tracked in F-dye-application.

**Unblocked by:** F-item-color
**Unblocked:** F-dye-application, F-dye-mixing
**Related:** F-dye-application, F-dye-mixing, F-dye-palette, F-fruit-pigments, F-fruit-variety, F-recipe-params, F-textile-crafting

#### F-dye-mixing — Dye color mixing recipes
**Status:** Todo · **Phase:** 7

Player-driven dye color mixing. UI with sliders for combining
existing pigments (primary dye items) in varying ratios, with a
live preview of the resulting color. When the player finalizes a
mix, they name the new color, which creates a palette entry
(F-dye-palette) storing both the RGB and the source recipe (which
pigments, what ratios). The mixing recipe is then available for
crafting at a workshop.

Depends on F-dye-palette for the color naming system and
F-recipe-params for representing mixing recipes with parameterized
inputs.

**Blocked by:** F-dye-palette
**Unblocked by:** F-dye-crafting, F-recipe-params
**Related:** F-dye-crafting, F-dye-palette, F-fruit-pigments

#### F-dye-palette — Named color palette system for dyes
**Status:** Todo · **Phase:** 7

Named color palette system for dyes. Each game has a palette table in
the sim DB storing named colors (ID, name, RGB values, source recipe
info). Palettes are civ-scoped so opposing players can't interfere
with each other's color systems.

Worldgen seeds the initial palette from fruit pigments — each
pigmented fruit species contributes a named color (e.g., "Red" from
a red-pigmented Shinethúni). F-dye-crafting is retrofitted so pressed
dye items reference a palette color ID rather than embedding raw RGB.

Dyed items also reference palette color IDs instead of raw ItemColor.
The item_color() resolution path looks up the palette entry's RGB.

Future dye mixing (F-dye-mixing) will let the player combine pigments
with sliders, preview the resulting color, name it, and add it to
the palette — storing the mixing recipe on the palette entry itself.

**Why:** Raw RGB on items creates unbounded cardinality that breaks
the recipe system. Named palette entries make colors a finite,
player-curated set that recipes and UI can reference cleanly.

**Blocks:** F-dye-application, F-dye-mixing
**Related:** F-dye-application, F-dye-crafting, F-dye-mixing, F-fruit-pigments

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

**Related:** F-bldg-dining, F-bldg-kitchen, F-bldg-storehouse, F-bread, F-fruit-extraction, F-fruit-prod, F-fruit-variety, F-hauling, F-logistics, F-recipes

#### F-food-quality-mood — Food quality affects dining mood boost
**Status:** Todo · **Phase:** 4

When item quality lands on food, scale the dining mood boost by
quality tier. Crude food gives a reduced (or zero) bonus; Fine and
above give progressively larger boosts. Applies to dining hall meals
and possibly carried-food eating. Depends on F-item-quality for the
quality tiers and F-bldg-dining for the dining mood system.

**Related:** F-bldg-dining, F-item-quality

#### F-footwear-split — Sandals/shoes as footwear, boots as armor
**Status:** Done

Split footwear into civilian (sandals, shoes) and military (boots as armor).
Sandals and shoes are normal clothing items; boots become explicitly an
armor piece that provides protection at the cost of comfort or speed.
Requires updates to equipment slots and item definitions.

**Related:** F-armor, F-clothing, F-want-categories

#### F-fruit-extraction — Fruit extraction (hulling/separation into components)
**Status:** Done · **Phase:** 7

Fruit extraction: the first processing step that converts a whole fruit
into its constituent component item stacks. A kitchen takes one whole
fruit and produces N separate inventory items — one per part — each
carrying the source species as material. For example, hulling a
Shinethuni fruit (37 pulp + 52 fiber + 15 seed) produces "37 Shinethuni
Pulp", "52 Shinethuni Fiber", and "15 Shinethuni Seed".

Implemented on `feature/F-fruit-extraction-v2` using the unified
crafting system — no separate monitor, action kind, or structure fields.
Each fruit species gets one dynamically generated Extract recipe in the
RecipeCatalog (1 Fruit → N component items based on species parts).

**What's done:**
- 6 new ItemKind variants: Pulp, Husk, Seed, FruitFiber, FruitSap,
  FruitResin. PartType::extracted_item_kind() maps part types to items.
- Dynamic extraction recipes generated per fruit species in
  build_catalog(). RecipeVerb::Extract, category "Extraction", Kitchen
  furnishing type, auto_add_on_furnish=false.
- RecipeDef.auto_add_on_furnish flag + default_recipes_for_furnishing()
  so extraction recipes don't clutter every new kitchen.
- Species-aware display names ("Shinethuni Pulp", "Testaleth Fiber").
- extract_work_ticks config parameter (default 3000).
- Greenhouse logistics priority fix: greenhouses now get priority 1 so
  the haul system can pull fruit from them.
- Auto-logistics works via the unified crafting system's
  compute_effective_wants().
- gdext bridge: new ItemKinds in logistics picker and material options.
- 11 tests covering recipe generation, catalog integration, monitor
  task creation, full extraction resolution, display names, serde
  roundtrip, target satisfaction, auto-add filtering, part mapping,
  and greenhouse-to-kitchen haul.

**Remaining (nice-to-have, not blocking merge):**
- Separation verb cosmetics (hull, press, crack, etc.)

Does NOT include downstream transformation recipes (bread, thread, etc.)
— those belong in F-component-recipes.

**Related:** F-bldg-kitchen, F-food-chain, F-fruit-variety

#### F-fruit-pigments — More natural fruit pigment colors (secondaries on fruit parts)
**Status:** Todo · **Phase:** 7

Allow secondary dye colors (Orange, Green, Violet) to appear naturally
on fruit parts during worldgen, not just the current 5 primaries +
modifiers (Red, Yellow, Blue, Black, White). This would expand the
palette of directly-pressable dyes and reduce dependence on F-dye-mixing
for color variety. Requires adding coverage categories for the new
pigment colors, updating FRUIT_COLORS, and adjusting the coverage-biased
generation algorithm.

**Related:** F-dye-crafting, F-dye-mixing, F-dye-palette, F-fruit-variety

#### F-fruit-prod — Basic fruit production and harvesting
**Status:** Todo · **Phase:** 2 · **Refs:** §13

Tree produces fruit at Leaf voxels over time. Elves harvest fruit to refill
their food gauge. Production rate depends on number of Leaf voxels
(photosynthesis capacity). Basic version: fruit spawns periodically at
random Leaf-adjacent positions, elves pathfind to harvest. Bridges the gap
between the existing food decay mechanic (F-food-gauge) and the advanced
food system (F-fruit-variety).

**Related:** F-branch-growth, F-elf-needs, F-food-chain, F-food-gauge, F-fruit-variety, F-greenhouse-revamp, F-population, F-vertical-garden

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
enum with FruitSpecies variant and serde support. Fruit extraction
(hull fruit → component items). Property-based component recipe
generation (starchy→flour→bread, fine fiber→thread→bowstring, coarse
fiber→cord→bowstring). Textile chain (fine fiber→thread→cloth→clothing
via Weave/Sew verbs).

**Still TODO:** Wild-only species restrictions, fruit part
rendering/visual differentiation, deeper integration with food chain
and cooking.

**Blocks:** F-civ-knowledge
**Related:** F-bldg-kitchen, F-civ-knowledge, F-civilizations, F-component-recipes, F-dye-crafting, F-food-chain, F-fruit-extraction, F-fruit-naming, F-fruit-pigments, F-fruit-prod, F-fruit-sprite-ui, F-fruit-sprites, F-fruit-yields, F-greenhouse-revamp, F-logistics-filter, F-recipes, F-textile-crafting

#### F-greenhouse-revamp — Greenhouse planter growth cycle and pluck tasks
**Status:** Todo

Revamp greenhouse internals: greenhouses contain individual planters as
furniture, each growing a single fruit on a visible growth schedule.
Planters show visual progress (sprout → mature → ripe). Fruits aren't
available until ripe, at which point an elf must come pluck them (a
task) to move them into the greenhouse's inventory. Replaces the current
autonomous production-during-heartbeat model with a more tactile,
visually rich loop. Number of planters scales with building floor area
(like other furnishing types).

**Related:** F-fruit-prod, F-fruit-variety, F-furnish, F-vertical-garden

#### F-hauling — Item hauling task type
**Status:** Done · **Phase:** 3

Multi-phase Haul task: creature walks to source (ground pile or building),
picks up reserved items, walks to destination building, deposits them.
Includes item reservation system to prevent double-claiming, cleanup on
task abandonment (clear reservations or drop carried items as ground pile).

**Related:** F-elf-acquire, F-food-chain, F-logistics

#### F-herbalism — Herbalism and alchemy
**Status:** Todo

Gathering forest plants and brewing them into useful products — healing
salves (F-rescue), combat buffs, mood tonics, mana potions (restore or
boost mana generation), poultices for tree diseases (F-tree-disease).
Requires foraging from the forest floor (F-forest-ecology) and a
workshop or alchemist's station. Needs more design research before
implementation.

**Related:** F-forest-ecology, F-rescue, F-tree-disease

#### F-herding — Manage animal groups with pens and grazing areas
**Status:** Todo

Manage groups of tamed animals — designate pastures and pens, assign
animals to grazing areas, herd movement between zones. Prevents animals
from wandering off. Elves with Beastcraft skill perform herding tasks.

**Blocked by:** F-wild-grazing
**Related:** F-civ-pets

#### F-insect-husbandry — Beekeeping and insect husbandry
**Status:** Todo

Domesticated insects integrated into the settlement economy — bees for
honey and wax, silkworms for thread, maybe giant beetles as pack animals.
Requires hives/enclosures as buildings or furniture. Honey as a food
ingredient and trade good; wax for candles and sealant; silk as a
textile material. Fits the forest ecology theme.

**Related:** F-forest-ecology

#### F-item-color — Item color system (material-derived and dye override)
**Status:** Done

Every item has a resolved color. Undyed items derive their color from
their material (e.g., oak → warm brown, iron → grey). Dyed items use
their dye color instead. A new optional `dye_color` field on `ItemStack`
stores the applied dye. A helper function `item_color(stack) -> Color`
returns the resolved color, using the dye color if present, otherwise
deriving from the stack's material, with a sensible default for items
with no material.

This item covers only the schema and color retrieval logic — not the
crafting process for creating dyes or applying them to items.

**Unblocked:** F-dye-application, F-dye-crafting, F-equipment-color

#### F-item-durability — Item durability system (current/max HP on items)
**Status:** Done

General item durability system: items have current_hp and max_hp fields
on ItemStack. Max HP is set from config at creation time; items not in
the durability config map are indestructible (0/0). When damaged via
inv_damage_item(), current_hp decreases; at 0 the item breaks (removed
from inventory, ItemBroken event emitted). For multi-item stacks, one
item is split off before damage so the rest keep full HP.

Display names show condition labels: "(worn)" when HP% <= worn threshold
(default 70%), "(damaged)" when HP% <= damaged threshold (default 40%).
Both thresholds are configurable in GameConfig. A GDScript mirror
(ItemUtils.condition_label) is available for UI-side use.

**Branch:** `feature/F-item-durability-display`

**Unblocked:** F-armor, F-arrow-durability
**Related:** F-clothing

#### F-item-quality — Item and output quality system
**Status:** Done · **Phase:** 4

Coarse-grained item quality (-1 to +3). At craft completion, roll
quasi_normal(stddev=50) + stats + skill against thresholds: <50 Crude,
50-249 Fine, 250+ Superior. Masterwork (+2) and Legendary (+3) locked
behind future path/recipe/material systems. Input quality drags down
output quality but cannot boost it. Elf starting gear is Crude for
early progression. All tiers displayed as prefix ("Crude Oak Bow",
"Fine Bread"). Items stack only when quality matches. Effects on food
mood, equipment stats, construction aesthetics TBD per tier.

**Draft:** docs/drafts/F-item-quality.md

**Related:** F-choir-build, F-creature-skills, F-food-quality-mood, F-manufacturing, F-quality-filters, F-sung-furniture

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

#### F-mana-grow-recipes — Grow-verb crafting recipes cost mana
**Status:** Done · **Refs:** §11

Grow-verb crafting recipes (magical shaping of wood, fruit cultivation, etc.)
should cost mana from the crafting creature's personal pool, using the same
per-action drain and wasted-action mechanics as construction. Deferred until
crafting refactoring is complete.

**Unblocked by:** F-mana-system
**Related:** F-component-recipes

#### F-mana-system — Mana generation, storage, and spending
**Status:** Done · **Phase:** 2 · **Refs:** §11

Dual-pool mana economy. Creatures have personal mana (mp / mp_max); many
species are nonmagical (mp_max = 0). Magical creatures (elves) generate mana
over time into their personal pool at a flat base rate (mood-dependent scaling
deferred to F-mana-mood). Excess mana overflows to the creature's bonded tree
each heartbeat (clamped to tree's mana_capacity; excess beyond cap is lost).
Creatures with no civ bond (wild creatures) lose their excess. Trees store
mana (mana_stored / mana_capacity) but do not generate it.

Construction and furnishing tasks have a per-action mana cost (config-driven,
per build type; types without a specific field use default_mana_cost_per_mille)
drained from the working creature's personal pool at the start of each work
action. If the creature lacks sufficient mana, it spends the time but
accomplishes no work (wasted action). Consecutive wasted actions are tracked
in a wasted_action_count field on Creature (reset on successful work or task
change). When the count reaches mana_abandon_threshold (configurable), the
creature abandons the task; it reverts to Available with all progress preserved
for another elf to pick up.

Task claiming: creatures with mp_max = 0 cannot claim mana-requiring tasks.
Magical creatures must have enough mana for at least one work action to claim
a mana-requiring task. More sophisticated rate-based sustainability estimates
are future work.

**Unblocked:** F-mana-depleted-vfx, F-mana-grow-recipes, F-mana-mood, F-mana-transfer, F-root-network
**Related:** F-branch-growth, F-choir-build, F-elf-mana-pool, F-forest-radar, F-mass-conserve, F-population, F-sung-furniture, F-tree-info, F-war-magic

#### F-mana-transfer — Tree-to-elf mana transfer
**Status:** Todo · **Refs:** §11

Mechanisms for the tree to transfer mana back to bonded elves. Design TBD —
possible triggers include proximity to the trunk, resting at home, or an
explicit player command. Enables the tree's communal mana reserve to support
individual elves who are mana-starved.

**Unblocked by:** F-mana-system

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

**Related:** F-batch-craft, F-bldg-kitchen, F-bread, F-item-quality, F-items, F-sculptures

#### F-pack-animals — Beast-of-burden hauling for heavy loads and caravans
**Status:** Todo

Tamed beasts of burden that can carry heavier loads than elves. Useful
for hauling large quantities of resources, potentially for trade
caravans. Species determines carry capacity. May require loading/
unloading tasks.

**Related:** F-civ-pets

#### F-path-civil — Civil path definitions and organic self-assignment
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Civil path definitions (Cook, Harvester, Artisan, Woodsinger, Poet) and
organic self-assignment. Civil paths can be player-assigned or self-assigned:
an elf who completes enough tasks of a given type triggers self-assignment
via the PathAffinity counter, with a notification to the player. Leveling
improves task speed, output quality, and recipe availability. Poet path
provides passive mana generation bonus scaling with CHA. Personality-based
compatibility checks deferred until F-personality exists — initial
self-assignment is purely counter-based.

**Blocks:** F-path-specialize
**Unblocked by:** F-path-core
**Related:** F-personality

#### F-pile-gravity — Ground pile gravity and merging
**Status:** Done · **Phase:** 4

Ground piles that are not physically on a solid surface (e.g., after the
platform beneath them is deconstructed) should fall until they reach a
surface. If a falling pile lands on a voxel that already has a ground pile,
the two piles merge their inventories into one.

**Related:** F-creature-gravity

#### F-quality-filters — Quality filters for logistics wants and active recipes
**Status:** Todo

**Related:** F-item-quality

#### F-recipe-any-mat — Any-material recipe parameter support
**Status:** Todo

Allow recipes to use material=Any (no specific material binding).
When a recipe runs with material=Any, the crafting action inspects
the reserved input stacks and propagates their material to the
output items. The UI shows "Any" as the default material option
for recipes where allows_any_material() is true (e.g., GrowBow,
assembly recipes). Recipes like Extract and Press that produce
species-dependent outputs cannot use Any.

F-recipe-params ships with specific-material-only; this ticket
adds the Any path.

**Unblocked by:** F-recipe-params
**Related:** F-recipe-params

#### F-recipe-hierarchy — Recipe catalog UI hierarchy and organization
**Status:** Done · **Phase:** 4

The recipe catalog now generates many per-species recipes (extraction,
milling, baking, spinning, twisting, bowstring assembly). With 20-40
fruit species and multiple recipe chains, the flat recipe list in the
structure info panel becomes unwieldy.

Add hierarchical browsing of the recipe catalog in the UI. Recipes
already carry a `category` field (e.g., `["Processing", "Milling"]`,
`["Extraction"]`). The UI should present these as collapsible groups
or a tree so players can quickly find and enable/disable recipes by
category rather than scrolling a flat list of 100+ entries.

Scope:
- GDScript UI changes to render recipe categories as a tree/accordion
- Possibly a search/filter box for quick lookup
- No sim changes needed — category data is already populated

**Related:** F-component-recipes, F-recipe-search, F-recipes

#### F-recipe-params — Parameterized recipe templates
**Status:** Done

Refactor the recipe system from a flat pre-generated catalog to
parameterized recipe templates. Currently every material variant of a
recipe is a separate RecipeDef (e.g., 30 "Extract {species}" recipes,
30 "Mill {species} Pulp" recipes). Parameterized recipes collapse these
into a single template with typed parameters that the player configures.

Parameter types:
- **Material**: selects which material applies to inputs/outputs. Can be
  "any" or a specific material. Propagation rules define how the chosen
  material flows to inputs and outputs.
- **Batch size** (future, see F-batch-craft): integer multiplier on
  input/output quantities, constrained by building workstation count.
- **Dye color** (future, see F-dye-palette): reference to a named color
  palette entry. Used by dyeing and dye mixing recipes.

A "configured recipe instance" is a template + parameter bindings.
The active recipe table, task reservation, crafting execution, save
format, and GDScript UI all need updating. The recipe catalog becomes
a small set of templates rather than hundreds of concrete recipes.

This is a foundational refactor that unblocks cleaner implementations
of dye application, dye mixing, and batch crafting.

**Why:** The current approach of proliferating concrete recipes does not
scale — dye colors and future recipe types would cause combinatorial
explosion. Parameterization keeps the catalog small and the UI
navigable.

**Draft:** docs/drafts/recipe_params.md

**Unblocked:** F-batch-craft, F-dye-application, F-dye-mixing, F-recipe-any-mat
**Related:** F-component-recipes, F-dye-crafting, F-recipe-any-mat, F-recipes, F-unified-craft-ui

#### F-recipes — Recipe system for crafting/cooking
**Status:** Done · **Phase:** 3

Shared recipe abstraction for kitchens and workshops: input items +
processing time → output items. Kitchens use recipes to convert fruit
to bread; workshops use recipes to convert wood to bows. Data-driven
via GameConfig so recipes can be added/tuned without code changes.
Avoids hardcoding conversion logic per building type.

**Related:** F-batch-craft, F-bldg-kitchen, F-bldg-workshop, F-component-recipes, F-crafting, F-food-chain, F-fruit-variety, F-recipe-hierarchy, F-recipe-params

#### F-sculptures — Decorative sculptures
**Status:** Todo

Craftable decorative sculptures that improve elf mood when placed in
buildings or public spaces. Material and quality affect mood bonus.
Skilled artisans produce better sculptures.

**Related:** F-manufacturing, F-mood-system

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
**Related:** F-activation-revamp, F-task-priority, F-task-proximity

#### F-task-proximity — Proximity-based task assignment (Dijkstra nearest)
**Status:** Done · **Phase:** 4

**Related:** F-task-assign-opt

#### F-task-tags — Decouple task eligibility from species via capability tags
**Status:** Todo

Replace hardcoded elf-only checks on tasks with a tag-based capability
system. Each species defines a set of capability tags (e.g. can_haul,
can_build, can_craft, can_cook, can_fight, can_tame). Elves have the
broadest tag set; tamed animals get a species-appropriate subset (most
just can_haul). Current path supplements the species base tags (e.g.
Scout adds can_tame). Task eligibility checks query tags instead of
checking `species == Elf`.

**Related:** F-civ-pets, F-labor-panel, F-path-core, F-taming

#### F-textile-crafting — Textile and clothing crafting recipes
**Status:** Done · **Phase:** 7

Textile and clothing crafting recipe chain for fine-fibrous fruit
species. Extends the component recipe system (F-component-recipes) with
weaving and sewing steps:

- Thread → Cloth (Weave, Workshop) — 10 thread → 1 cloth
- Cloth → Tunic (Sew, Workshop) — 3 cloth → 1 tunic
- Cloth → Leggings (Sew, Workshop) — 2 cloth → 1 leggings
- Cloth → Boots (Sew, Workshop) — 2 cloth → 1 pair of boots
- Cloth → Hat (Sew, Workshop) — 1 cloth → 1 hat

New ItemKind variants: Cloth, Tunic, Leggings, Boots, Hat.
New RecipeVerb variants: Weave, Sew.
All recipes are material-specific (per fruit species) and data-driven
via ComponentRecipeConfig. Clothing items are created but elves don't
wear them yet (that's F-clothing).

Coarse fiber textile paths (cord → canvas/burlap) deferred to future work.
Dye integration deferred to F-dye-crafting.

**Related:** F-component-recipes, F-dye-crafting, F-fruit-variety

#### F-traders — Visiting traders from other civs
**Status:** Todo

Traders from other civilizations periodically visit to buy and sell goods.
Provides access to materials and items not locally available. Trade
relationships affected by diplomacy (F-civilizations). Requires a trade
depot or meeting area.

**Related:** F-civilizations

#### F-tree-capacity — Per-tree carrying capacity limits
**Status:** Todo · **Phase:** 7 · **Refs:** §13

Each tree has a carrying capacity limiting how many elves/structures it can
support. Encourages distributed village design across multiple trees.

**Related:** F-multi-tree, F-population

#### F-unified-craft-ui — Unified data-driven building crafting UI
**Status:** Done · **Phase:** 4

Replace per-building-type crafting UIs (kitchen cooking toggle, workshop recipe list, extraction settings) with a single unified data-driven crafting panel. Buildings expose available recipes based on their furnishing type; the UI dynamically renders recipe selection, material pickers, and output targets from recipe metadata. Reduces code duplication and makes adding new recipe types trivial.

**Draft:** docs/drafts/unified_craft_ui.md

**Related:** F-batch-craft, F-recipe-params

#### F-vertical-garden — Vertical gardens on the tree
**Status:** Todo

Growing food directly on the tree — mosses, epiphytes, hanging planters,
vine-trained crops. Reduces dependence on ground-level farming. May depend
on moss/epiphyte visual systems being in place first. Distinct from
F-greenhouse-revamp (enclosed building) — these are open-air, integrated
into the tree's living surface.

**Related:** F-fruit-prod, F-greenhouse-revamp

#### F-want-categories — Categorical want specifications (any footwear, any melee weapon)
**Status:** Todo

Allow logistics wants to specify item categories instead of exact ItemKind
values. For example, "any civilian footwear" (Sandals or Shoes), "any melee
weapon" (Spear or Club), "any armor piece", etc. Currently each want must
name a specific ItemKind, which means the default wants hard-code Shoes
rather than expressing "I want some kind of footwear." A categorical want
would let creatures pick up whichever matching item is available, making the
system more flexible and reducing the need to update wants when new item
variants are added.

Change the default elf wants to use categorical specifications where
appropriate (e.g., "any civilian footwear" instead of "Shoes").

**Related:** F-footwear-split

#### F-wood-stats — Wood-type material variation for crafted items
**Status:** Todo

**Related:** F-elf-weapons

### Social & Emotional

#### F-animal-bonds — Elf-animal relationships and bonded pairs
**Status:** Todo

Relationships between specific elves and tamed animals. Bonded pairs
form through prolonged interaction (taming, care, proximity). Mood
effects for both elf and animal — happiness from bond, grief on death.
Bonded animals follow their elf, respond better to commands, and may
defend them in combat.

**Related:** F-civ-pets, F-emotions

#### F-apprentice — Skill transfer via proximity
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elves learn skills by working near skilled elves. Apprenticeship as an
emergent social/economic system.

**Related:** F-creature-skills, F-path-core

#### F-emotions — Multi-dimensional emotional state
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Emotions as multiple simultaneous dimensions: joy, fulfillment, sorrow,
stress, pain, fear, anxiety. Not a single "happiness" number.

**Blocks:** F-elf-leave, F-hedonic-adapt, F-mana-mood
**Related:** F-animal-bonds, F-path-core, F-path-stuck, F-social-graph

#### F-emotions-basic — Mood score from thought weights
**Status:** Done · **Phase:** 4 · **Refs:** §18

Derived mood score: sum of configurable per-ThoughtKind weights across a
creature's active thoughts. Seven-tier label (Devastated → Elated). Computed
on demand, never stored. Lays groundwork for full F-emotions.

#### F-festivals — Festivals and community ceremonies
**Status:** Todo

Periodic community events — harvest festival, solstice celebration,
remembrance of the fallen, coming-of-age ceremonies. Elves gather, sing,
feast; temporarily unavailable for work but gain significant mood boosts.
Scheduling tradeoffs: cancel a festival to meet a construction deadline,
but elves will be unhappy about it. Distinct from F-poetry-reading (small
social gatherings) — festivals are larger, rarer, whole-community events
with mechanical consequences.

**Related:** F-mood-system, F-poetry-reading

#### F-funeral-rites — Funeral rites and mourning
**Status:** Todo

Cultural rituals when an elf dies. The community mourns — friends and
family attend a ceremony, mood is affected settlement-wide. The form of
the funeral may be culturally determined (sky burial in the canopy, root
burial, soul-singing). Neglecting funeral rites (e.g., during a crisis)
has lasting mood consequences. Interacts with F-incapacitation, F-soul-mech,
and F-social-graph (close relationships = deeper grief).

**Related:** F-incapacitation, F-mood-system, F-social-graph, F-soul-mech

#### F-group-chat — Group chat social activity
**Status:** Todo

**Unblocked by:** F-group-activity

#### F-group-dance — Group dance and social singing activities
**Status:** Done

Procedural dance generator for group dance activities. Elves on a
rectangular voxel-grid floor perform coordinated figures (chains, ring
rotations, swaps, etc.) synchronized to music generator output. V1:
sequential figures, one at a time. Future V2: simultaneous overlapping
figures with per-beat collision avoidance — Celtic knot-style interlocking
geometric patterns. Module within elven_canopy_sim.

**Draft:** docs/drafts/F-group-dance.md

**Unblocked by:** F-group-activity
**Related:** F-bldg-concert, F-dance-choreo, F-dance-movespeed, F-dance-scaling, F-dance-self-org, F-music-runtime

#### F-hedonic-adapt — Asymmetric hedonic adaptation
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elves adapt to good conditions faster than bad ones. A beautiful new
platform stops feeling special after a while, but a cold sleeping spot
never stops being miserable.

**Blocked by:** F-emotions

#### F-mana-mood — Mana generation tied to elf mood
**Status:** Todo · **Phase:** 4 · **Refs:** §11, §18

Replace flat-rate per-creature mana generation with mood-dependent rates.
Happy elves generate more mana into their personal pool, unhappy elves
generate less. Uses the existing mana_mood_multiplier_range config to
interpolate a multiplier from worst to best mood tier. Completes the core
feedback loop: happy elves → more mana → faster construction → better
village → happier elves.

**Blocked by:** F-emotions
**Unblocked by:** F-mana-system

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

**Related:** F-festivals, F-funeral-rites, F-sculptures

#### F-narrative-log — Events and narrative log
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Sim emits narrative events (arguments, friendships formed, dramatic moments).
Log viewable by player, drives emergent storytelling.

#### F-notifications — Player-visible event notifications
**Status:** In Progress · **Phase:** 4

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
- Bell icon button (top-right HUD) with unread count badge. Opens a
  scrollable notification history panel showing timestamped entries
  grouped by type, newest-first, with 200-entry cap and oldest-first
  eviction. Count-based unread tracking, hydrated on save/load.
- Minimap relocated from bottom-right to bottom-left (above status bar)
  to accommodate the history panel.
- GDScript unit tests for bell state, panel population, eviction, and
  unread tracking (test_notification_bell.gd, test_notification_history_panel.gd).

**Still needed:**
- Wire more sim events to push notifications (construction complete,
  creature idle, structure collapsed, elf left, etc.).

**Blocks:** F-elf-leave
**Related:** F-status-bar

#### F-path-core — Elf path system core (Way/Calling/Attunement)
**Status:** Done · **Phase:** 4 · **Refs:** §18

Core data model for the elf path system. Paths are disciplines elves commit
to with escalating depth: Way (flexible, learning) → Calling (committed,
faster XP) → Attunement (permanent, identity-defining). Adds PathAssignment,
PathHistory, and PathAffinity tables to SimDb, PathConfig to GameConfig, XP
tracking, level thresholds, and tier transition logic including Attunement
lock and warning window. Integer math only for determinism. Tier transitions
are initially threshold-based (time + level); personality gating deferred
until F-personality exists. Foundation for all other path features.

**Draft:** docs/drafts/F-elf-paths.md

**Unblocked:** F-path-civil, F-path-combat, F-path-residue, F-path-stuck, F-path-ui
**Related:** F-apprentice, F-creature-skills, F-emotions, F-personality, F-task-tags

#### F-path-residue — Skill residue from past paths
**Status:** Todo · **Phase:** 4 · **Refs:** §18

When an elf leaves a path, a configurable fraction of their accumulated
stat bonuses is retained as a permanent passive modifier (skill residue).
An ex-Warrior archer is slightly tougher than a pure archer. Additionally,
re-entering a previously walked path has reduced XP cost. Residue does not
grant active abilities from the old path — only stat echoes. Attuned elves
cannot leave their category, so residue only applies to Way/Calling-tier
transitions. Residue fraction is configurable per path via
residue_fraction_permille. Woodsinging has a higher residue fraction than
other craft paths.

**Unblocked by:** F-path-core

#### F-path-specialize — Path specialization branching and prerequisites
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Specialization branching within base paths. At a configurable level
threshold, an elf can fork into a narrower discipline (e.g., Warrior →
Blademaster, Archer → Sharpshooter, Artisan → Carver). Specializations
have prerequisites beyond just level — Champion requires civil path
experience, Healer requires both Woodsinger and Harvester levels.
Specialization narrows ability: a Spoon Carver refuses to craft chairs.
For combat paths, the player picks the specialization. For civil paths,
organic specialization is possible via repeated narrow task performance.

**Blocked by:** F-path-civil, F-path-combat

#### F-path-stuck — Deep commitment personality drift and refusal
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Deep commitment personality effects. Elves deeply invested in a path
undergo personality drift — personality axes shift toward stereotypes of
their discipline (Warriors become aggressive, Artisans become perfectionist).
Forced reassignment at Calling tier incurs mood penalties scaled by
personality. Note: the Attunement lock itself (hard refusal to leave
category) is part of F-path-core's tier transition logic and does not
require personality. This feature covers only the personality-dependent
aspects: drift, mood scaling, and personality-informed behavior changes.

**Blocked by:** F-personality
**Unblocked by:** F-path-core
**Related:** F-emotions

#### F-personality — Personality axes affecting behavior
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Multi-axis personality model affecting task preferences, social behavior,
stress responses, and creative output.

**Blocks:** F-cultural-drift, F-path-stuck
**Related:** F-path-civil, F-path-core, F-social-graph

#### F-poetry-reading — Social gatherings and poetry readings
**Status:** Todo · **Phase:** 4 · **Refs:** §18, §20

Elves gather for poetry readings, festivals, and social events. Quality of
poetry/music affects mood and mana generation.

**Related:** F-festivals, F-proc-poetry, F-vaelith-expand

#### F-seasons — Seasonal visual and gameplay effects
**Status:** Todo · **Phase:** 4 · **Refs:** §8, §18

Leaf color changes, snow, seasonal fruit production variation. Gameplay
effects: cold weather increases clothing need, leaf drop reduces canopy
shelter.

**Related:** F-forest-ecology, F-weather

#### F-social-graph — Relationships and social contagion
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elf-to-elf relationships: friendships, rivalries, romantic bonds, mentorship.
Emotional contagion spreads mood through social connections.

**Related:** F-emotions, F-funeral-rites, F-personality

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

#### B-music-floats — Excise f32/f64 from music composition for determinism
**Status:** Done

The music composition system (create_composition in elven_canopy_music,
called from elven_canopy_sim) uses f32 arithmetic internally. This
violates the sim's determinism constraint — f32 results can vary across
platforms and compilers. If composition results feed back into sim state
(e.g., choir harmony affecting construction speed), the floats must be
replaced with fixed-point or integer arithmetic to maintain cross-platform
determinism. The music crate's standalone use (CLI, audio rendering) is
not affected, but any path from composition → sim state must be
float-free.

**Related:** F-choir-harmony

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

**Related:** B-music-floats, F-choir-build, F-combat-singing, F-group-activity, F-music-runtime

#### F-combat-singing — Combat singing buffs and musical instrument bands
**Status:** Todo

Far-future feature: elves singing in combat to buff allies (attack speed,
damage, morale, mana regen). Could evolve into a musical instrument /
band system where different instruments provide different buffs and a
full ensemble produces harmony bonuses. Mechanically distinct from
construction choir singing (F-choir-build / F-choir-harmony). The music
crate and lang crate already generate Vaelith lyrics and polyphonic
music — this could eventually tie in. CHA stat drives singing
effectiveness.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-buff-system
**Related:** F-buff-system, F-choir-build, F-choir-harmony, F-group-activity

#### F-dance-choreo — Refine dance figure choreography
**Status:** Todo

Refine the dance figure vocabulary and choreography. Current figures (advance/retire, ring rotation, swap, set-in-place) are basic. Improvements: advance-and-retire should move lines toward each other (not all +z), add Grid formation for odd-count groups, add do-si-do and chain/grand-chain figures, variable-length set-in-place (2-4 steps), and better figure selection weighting for visual variety.

**Related:** F-group-dance

#### F-dance-movespeed — Dance movement paced to creature walk speed
**Status:** Todo

Dance waypoint timing currently ignores creature walk speed — elves teleport to positions at beat-aligned ticks regardless of distance. Movement duration should be plausible relative to the creature's actual walk speed, with the beat grid informing when moves START rather than dictating impossible speeds.

**Related:** F-group-dance

#### F-dance-scaling — Support more than 3 dancers
**Status:** Todo

Scale dance activities beyond the current 3-elf minimum/desired count. Larger dance halls should attract more dancers, formations should adapt to participant count, and the choreography should remain visually interesting at 6-12+ participants.

**Related:** F-group-dance

#### F-dance-self-org — Elves self-organize dances
**Status:** Todo

Idle elves autonomously discover dance halls and organize dances without player intervention via the Debug Dance button. Includes choosing when to dance (mood/social need triggers), recruiting other elves, and naturally ending dances. Open question: how many elves should a dance recruit? Currently hardcoded min=3 desired=6 in the debug dance path.

**Related:** F-group-dance

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

**Related:** F-bldg-concert, F-choir-build, F-choir-harmony, F-group-dance

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

#### F-voice-subsets — Variable voice count (SATB subsets)
**Status:** Done

### Combat & Defense

#### B-flying-arrow-chase — Flying creatures excluded from arrow-chase
**Status:** Done

Flying creatures (Hornet, Wyvern) are currently excluded from
maybe_arrow_chase because their activation loop ignores tasks. Once
B-flying-tasks is resolved and flying creatures use the standard
task-based activation pipeline, remove the flight_ticks_per_voxel
guard in maybe_arrow_chase so flying creatures also chase toward
arrow sources outside their detection range.

**Unblocked by:** B-flying-tasks
**Related:** F-arrow-chase

#### B-flying-flee — Flying creatures flee by random wander instead of directionally
**Status:** Todo

Flying creatures (Hornet, Wyvern) flee by calling fly_wander (random direction) instead of directionally away from threats like ground creatures do with ground_flee_step (picks nav edge maximizing distance from nearest hostile). A proper fly_flee_step should pick the flyable neighbor voxel that maximizes distance from the nearest threat.

#### B-hostile-detect-nav — detect_hostile_targets panics on flying targets (NavNodeId u32::MAX hack)
**Status:** Done

detect_hostile_targets returns (CreatureId, NavNodeId) but flying
creatures may have no nav node. Currently hacked with NavNodeId(u32::MAX)
placeholder which will likely cause a vec lookup panic if ground
combat code ever receives it.

Revamp to work on coordinate logic instead of requiring a nav node
for every target. If a target isn't on your nav grid, find a place
on your grid that is in melee range of the target's position; if
none exists, don't try to path to it. Remove the u32::MAX hack.

#### B-raid-spawn — Raiders sometimes spawn inside map instead of at perimeter
**Status:** Done

Raiders are supposed to spawn at the map edge perimeter, but some spawn closer to the center. Likely related to floor_extent no longer matching the actual terrain extent — floor_extent defines the raid perimeter band, but terrain now covers the full world (1024x1024). The perimeter filter in find_perimeter_positions (raid.rs) uses floor_extent to determine "edge" positions, which may be much smaller than the actual map edge with the bigger world.

#### F-anatomy — DF-style hit location anatomy system
**Status:** Todo

Dwarf Fortress-style hit location system. Attacks target specific body parts
(head, torso, limbs) with distinct injury effects — a leg wound slows
movement, an arm wound reduces combat ability, a head wound can cause
unconsciousness. Species define their anatomy template. Interacts with
F-incapacitation (injury type may determine incapacitation behavior) and
F-armor (armor covers specific body parts).

**Related:** F-armor, F-incapacitation

#### F-armor — Wearable armor system
**Status:** Done

Armor items that can be worn in clothing slots, providing damage reduction in combat. Builds on the clothing/wearable system (F-clothing) for slot mechanics and equip/unequip flow. Many details TBD: armor types and their stats (leather, chain, plate?), how damage reduction is calculated (flat reduction? percentage? per-damage-type?), armor durability and repair, crafting recipes and material requirements, how armor interacts with movement speed or other stats, visual representation, whether armor and clothing can be worn simultaneously (layering), and species-specific armor availability.

**Unblocked by:** F-clothing, F-item-durability
**Unblocked:** F-military-armor
**Related:** F-anatomy, F-footwear-split

#### F-arrow-chase — Enemies chase toward arrow source outside detection range
**Status:** Done

When a hostile creature is hit by a projectile from outside its detection
range, it infers the approximate direction of the attacker. If the creature
is aggressive, it chases in that direction for a limited time/distance before
giving up. Adds tactical depth — sniping from range has consequences, and
enemies don't just stand there absorbing arrows from the fog.

**Related:** B-flying-arrow-chase, B-flying-tasks, F-enemy-ai

#### F-arrow-durability — Arrow durability and recovery
**Status:** Done · **Phase:** 3

Arrows take random durability damage on impact (creature or surface).
Damage is uniform random in [arrow_impact_damage_min, arrow_impact_damage_max]
(defaults 0–3). With 3 max HP, this gives roughly equal chances of:
undamaged, worn (2/3 HP), damaged (1/3 HP), or destroyed. Destroyed
arrows emit ItemBroken and are not placed in ground piles. Surviving
arrows land in ground piles with their reduced HP preserved, and arrows
at different HP levels remain as separate stacks. Worn/damaged arrows
deal proportionally less creature damage (scaled by current_hp/max_hp,
minimum 1).

**Branch:** `feature/arrow-damage-scaling`

**Unblocked by:** F-item-durability

#### F-attack-evasion — Attack accuracy and evasion with quasi-normal hit rolls
**Status:** Done · **Phase:** 3

Attack accuracy and evasion using quasi-normal hit rolls. Attacker
rolls (attacking skill + DEX) + quasi-normal random (stdev ~50) against
defender (Evasion skill + AGI). Equal stats → ~50% hit chance. If roll
exceeds defender total by 100+ (≈2 stdev, ~2.3%), critical hit deals
double damage. Uses sum of 12 uniform integer samples in [-25,25] for
deterministic quasi-normal distribution. Melee uses Striking + DEX vs
Evasion + AGI. Ranged applies this check only after the existing
projectile-hit check passes (arrow can miss physically OR be evaded).
Stacks with existing armor damage reduction.

**Related:** F-creature-skills, F-creature-stats

#### F-attack-move — Attack-move task (walk + fight en route)
**Status:** Done

TaskKindTag::AttackMove — hotkey F + click on ground (R and F camera keys removed from focal_up/focal_down, Page Up/Down remain). Extension table TaskAttackMoveData with destination (VoxelCoord). Walk toward destination; on each activation scan for hostiles within species detection range. If hostile detected, pick nearest by squared euclidean distance (ties broken by CreatureId), set target_creature on base Task row, and engage (melee/ranged actions via shared combat helpers). Poll target vital_status — if Dead or missing, clear target_creature and resume walking. On path failure during engagement, disengage immediately (no retry). On arrival at destination with no active target, task completes. Player-directed origin gives PlayerCombat preemption level (exempt from flee).

**Draft:** docs/drafts/attack_move.md

**Draft:** docs/drafts/combat_military.md (§2 "Attack-Move")

**Related:** F-attack-task

#### F-attack-task — AttackCreature task (player-directed target pursuit)
**Status:** Done

TaskKindTag::AttackTarget — player right-clicks a hostile creature. Creates task with TaskOrigin::PlayerDirected, PreemptionLevel::PlayerCombat(6). Extension table TaskAttackTargetData with target: CreatureId (plain ID, not FK). Behavior: pathfind toward target via dynamic pursuit, when adjacent perform melee actions, when in range with LOS perform shoot actions. Poll target vital_status each activation — if Dead or row missing, task completes. Works with melee-only initially; ranged is additive. Autonomous combat tasks (created by hostile detection) are immediately claimed by the detecting creature — NOT left in Available state.

**Failed pathfinding:** If pathfinding fails (target unreachable), retry on the next activation. After N consecutive failures (configurable, e.g., attack_path_retry_limit = 3), cancel the task. Creature returns to normal behavior (idle/wander) and may re-detect the target on a subsequent activation if the target has moved to a reachable location.

**Draft:** docs/drafts/combat_military.md (§5 "Attack Tasks")

**Related:** F-attack-move

#### F-buff-system — Generic timed stat modifier buffs on creatures
**Status:** Todo

Generic system for timed stat modifier buffs on creatures. Each buff has
a source, duration, and one or more stat modifiers (e.g., +damage, +speed,
-speed). Multiple buffs can stack. Buffs tick down each heartbeat and are
removed on expiry. Distinct from status effects (F-status-effects): status
effects are binary conditions (immobilized, cloaked), buffs are numeric
modifiers. Shared infrastructure for enchanted arrows, combat singing,
berserk, and future buff-granting spells.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Blocks:** F-combat-singing, F-spell-berserk, F-spell-ench-arrow
**Related:** F-combat-singing, F-path-combat, F-status-effects

#### F-cavalry — Mount tamed creatures as cavalry
**Status:** Todo

Elves can mount tamed creatures tagged as rideable. Mounted unit moves
at the mount's speed, uses the mount's footprint for pathing, and the
rider can use ranged or melee weapons from the saddle. Archers on
elephants, scouts on deer, etc. Mount and rider take damage separately.

**Related:** F-civ-pets, F-war-animals

#### F-combat — Combat and invader threat system
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Invader types, threat mechanics, and basic combat resolution. Ties into
fog of war for surprise attacks.

**Blocked by:** F-enemy-ai
**Blocks:** F-defense-struct, F-military-campaign, F-military-org
**Related:** F-elf-weapons, F-engagement-style, F-fog-of-war

#### F-conjured-creatures — Temporary creature spawning with lifetime and auto-despawn
**Status:** Todo

System for spells to spawn temporary allied creatures with a fixed
lifetime. Conjured creatures have independent AI, fight for the caster's
side, and auto-despawn when their duration expires or they are killed.
Not "real" creatures — they don't eat, sleep, have mood, or leave
corpses. Possible summon types: elephant (tanky/slow), giant hornet
(fast melee), bee swarm (AoE damage-over-time cloud).

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Blocks:** F-spell-summon

#### F-creature-control — Temporary allegiance change and AI override
**Status:** Todo

System for temporarily overriding a creature's allegiance or AI behavior.
Supports mind control (creature fights for the caster's side) and berserk
(creature attacks nearest target regardless of allegiance). When the
effect expires, original allegiance and AI restore. Must handle edge
cases: what happens if a mind-controlled creature's controller dies?

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Blocks:** F-spell-berserk, F-spell-mind-ctrl

#### F-defense-struct — Defensive structures (ballista, wards)
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Ballista turrets, magic wards, and other defensive construction. Requires
the construction system to support these build types.

**Blocked by:** F-combat

#### F-elf-weapons — Bows, spears, clubs for elf combat
**Status:** Done · **Phase:** 8+ · **Refs:** §16

Weapon types with different ranges, damage, and crafting requirements.

**Done so far:** Spear and Club item kinds added. GrowSpear/GrowClub single-step workshop recipes (wood only, zero inputs). Weapon selection in melee combat: best weapon chosen by distance — club preferred at close range (higher damage, range_sq=3), spear at extended range (range_sq=8), bare hands fallback. Weapon damage replaces species base; STR scaling still applies. Melee weapon degradation (0–2 HP per strike, configurable). Config fields for weapon base damage, range, work ticks, and degradation. GDExt bridge updated. 22 unit tests.

**Unblocked by:** F-crafting
**Related:** F-bldg-workshop, F-combat, F-wood-stats

#### F-enemy-ai — Hostile creature AI (goblin/orc/troll behavior)
**Status:** In Progress

Simple aggression AI for non-civ hostile creatures. This is the first "it all comes together" milestone — debug-spawn a goblin and watch it chase and attack an elf.

**Done so far:** Simplified hostile AI via the wander path (no tasks, no formal detection/preemption). `Species::is_hostile()` gates behavior for Goblin/Orc/Troll. On each activation with no task, `hostile_pursue()` collects living elf nav nodes, runs Dijkstra to find the nearest reachable elf, A* to get a path, and moves one edge toward it. When in melee range, auto-calls `try_melee_strike()`. On cooldown, waits in place and re-activates when cooldown expires. Falls back to random wander if no elf reachable. Events threaded through the activation chain so combat events (CreatureDamaged, CreatureDied) are properly emitted. Refactored `wander()` into `hostile_pursue()`, `random_wander()`, and shared `move_one_step()`. Formal hostile detection system (F-hostile-detection) with configurable detection range, `EngagementStyle` on SpeciesData (supersedes old CombatAI enum), and faction-based hostility. Task-driven attack (F-attack-task) with AttackTarget task kind and dynamic pursuit. Preemption system (F-preemption) so combat can interrupt lower-priority tasks.

**Not yet done:** Two-phase proximity optimization (squared distance filter before pathfinding). Target selection by closest distance rather than first-by-ID. Path caching to avoid Dijkstra+A* on every activation.

**Draft:** docs/drafts/combat_military.md (§6 "Initial Behavior")

**Draft:** docs/drafts/combat_military.md (§6 "Initial Behavior")

**Blocks:** F-combat
**Related:** F-aggro-fauna, F-arrow-chase, F-enemy-raids, F-engagement-style

#### F-enemy-raids — Enemy civilizations send raids
**Status:** Done

Hostile civilizations periodically send organized raid parties against the
player's tree. Raid frequency, composition, and strength scale with game
progression. Requires F-civilizations for civ relationships and diplomacy
to determine who raids and why.

**Unblocked by:** F-civilizations
**Related:** F-civilizations, F-enemy-ai, F-raid-detection, F-settlement-gen, F-zone-world

#### F-engagement-style — Unified engagement style (species + military group combat tactics)
**Status:** Done

A single `EngagementStyle` struct that governs how a creature uses its weapons in combat. Replaces the current split between `CombatAI` (species-level, coarse) and `HostileResponse` (military group, binary Fight/Flee). The same struct lives on both `SpeciesData` (species defaults for non-civ creatures) and `MilitaryGroup` (player-configurable per-group overrides for civ creatures), using identical code paths.

**Fields:**

- **Weapon preference:** Prefer ranged / prefer melee. "Prefer ranged" behaves like current hostile_pursue (shoot, close to melee when out of range). "Prefer melee" skips ranged unless no path to target exists (or future: can shoot while moving without penalty).
- **Ammo exhaustion behavior:** Switch to melee / flee.
- **Engagement initiative:** Aggressive (pursue on detection, willing to chase long distances) / defensive (counter-attack and fight within ~5 voxels of position when combat started, but don't chase beyond that) / passive (never initiate, never counter-attack).
- **Disengage threshold:** HP% at or below which the creature rationally breaks off combat and flees (distinct from F-instinctual-flee which is involuntary panic). At 100%, creature always flees (default for civilians). At 0%, creature never disengages.

Species defaults should make intuitive sense (goblins: aggressive melee; orc archers: prefer ranged, switch to melee on ammo out; deer: passive; elves: defensive, prefer ranged, flee on ammo out, disengage 100%). Military group config lets the player override for their civ creatures ("Archers" group: prefer ranged, flee on ammo out; "Vanguard": prefer melee, aggressive). No per-creature overrides — always group-level for civ creatures, species-level for non-civ.

Supersedes `CombatAI` enum on `SpeciesData` and `HostileResponse` on `MilitaryGroup` — both collapse into `EngagementStyle`. All existing combat decision logic (`should_flee()`, `hostile_pursue()`, `wander()`, `flee_step()`, `detect_hostile_targets()`) is rewritten against the unified struct.

**UI scope:** The military groups screen replaces the current Fight/Flee toggle with fields for each `EngagementStyle` option.

**Unblocked:** F-instinctual-flee, F-skirmish
**Related:** F-combat, F-enemy-ai, F-military-groups

#### F-ff-vertical-arc — Vertical arc awareness for friendly-fire checks
**Status:** Todo

Currently, friendly-fire checks use the projectile's 2D column (XZ) to detect friendlies in the flight path. A high-arc shot that passes over a friendly's head is treated as blocked even though it would be safe. This feature adds vertical (Y) awareness to the friendly-fire check, allowing shots whose arc clears friendlies vertically. This would let archers on higher platforms or at longer ranges (higher arcs) shoot over intervening friendlies.

**Related:** F-friendly-fire

#### F-flee — Flee behavior for civilians
**Status:** Done

Creatures with passive initiative or HP below disengage threshold detect hostiles within range, preempt current task, and perform greedy retreat. At each activation, pick nav neighbor maximizing squared euclidean distance from threat (anchor voxel for multi-voxel threats). Ties broken by NavNodeId. Continue fleeing while hostile is in detection range. Dead-end trapping is acceptable (mirrors panic behavior, motivates escape route construction). Future: cornered behavior, bounded A* instead of greedy.

**Done so far:** Flee behavior implemented via `should_flee()` / `flee_step()`. Controlled by `EngagementStyle`: passive initiative always flees, disengage threshold (HP%) triggers flee when HP is low (100% = always flee, used for civilians). Military group engagement style overrides species defaults for civ creatures. Elf `hostile_detection_range_sq` set to 225 (15-voxel radius). Flee check runs before the decision cascade in `process_creature_activation` — detects threats via existing `detect_hostile_targets()`, interrupts current task, then greedy retreat (maximize squared distance from nearest threat, NavNodeId tie-breaking). Cornered creatures (no eligible edges) reschedule activation and wait. Flee stops immediately when threat leaves detection range.

**Not yet done:** `flee_cooldown_ticks` for persistence after threat leaves range. Bounded A* instead of greedy. Cornered behavior (desperate fighting). Flee toward friendly soldiers. Panic/fear thoughts.

**Draft:** docs/drafts/combat_military.md (§7)

#### F-fog-of-war — Visibility via tree and root network
**Status:** Todo · **Phase:** 8+ · **Refs:** §17

World hidden except where observed by elves or sensed through tree/root
network. Strongest near trunk, weaker at root edges, absent beyond.

**Related:** F-combat, F-raid-detection, F-root-network, F-stealth

#### F-friendly-fire — Friendly-fire avoidance for ranged attacks
**Status:** Done

Projectiles currently hit any alive creature in their path after the origin voxel, with no distinction between friendly and hostile targets. This feature adds friendly-fire avoidance so ranged attackers don't shoot through friendly creatures, and actively reposition to find a clear shot.

**Detection:** Voxel-level checks along the projectile's 2D column flight path (vertical arc awareness is a separate future feature, F-ff-vertical-arc). The origin voxel and its immediate neighbors are excluded from the check for non-hostile creatures, so squads can stand together and fire, and elves can shoot point-blank at hostiles in adjacent voxels.

**In-flight arrows:** No retroactive avoidance. If a friendly walks into an arrow's path mid-flight, they get hit. This is intentional — unlucky shots hurt, and future features may introduce trait-based carelessness or other effects.

**When blocked (no clear shot to current target):**

1. **Redirect:** Try other hostiles already in range/awareness that have a clear flight path. Pick the best alternate target.
2. **Reposition:** If no hostile has a clear path, move to a neighboring nav node. Score candidates by: (a) clear shot available (primary), (b) doesn't block nearby elves' lines of fire, (c) distance to target (prefer not moving further away). Commit to the move before re-evaluating to prevent oscillation between two elves shuffling back and forth.
3. **Fallback reposition:** If no candidate scores well, pick a direction that at least doesn't block other nearby elves' lines of fire.
4. **Hold fire:** If truly stuck, hold position and wait.

**Attack-move when blocked:** Use repositioning logic if available; otherwise keep advancing toward the target (closing to melee range).

**Player-directed vs autonomous:** No distinction. Elves always respect friendly-fire avoidance regardless of how the attack was ordered.

**Emergent formation:** The repositioning behavior naturally produces firing-line-like formations as elves spread out to find clear angles, without explicit formation code.

**Related:** F-ff-vertical-arc, F-projectiles, F-shoot-action

#### F-hostile-detection — Hostile detection and faction logic
**Status:** Done

Activation-driven hostile scanning. On each creature activation, scan for hostiles within hostile_detection_range_sq (SpeciesData, squared euclidean voxels). Hostility determination: per-direction (not mutual). Civ creatures check CivOpinion::Hostile toward other civ. Non-civ creatures with aggressive `EngagementStyle` initiative treat all civ creatures as hostile (except same-species exemption). Non-civ aggressors don't attack each other. `EngagementStyle` struct on SpeciesData governs initiative (Aggressive/Defensive/Passive), weapon preference, ammo exhaustion, and disengage threshold. Auto-escalation when attacked (design question: no target civ for non-civ attackers — may only apply to civ-vs-civ). Detection is O(n) scan over all creatures with squared-distance filter (BTreeMap spatial index doesn't support 3D range queries). Height makes detection range ineffective across tree levels — design decision needed on whether this is intentional.

**Draft:** docs/drafts/combat_military.md (§6, §7)

**Related:** F-los-tuning, F-per-detection

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

**Related:** F-hp-death, F-incapacitation

#### F-incapacitation — Incapacitation at 0 HP instead of instant death
**Status:** Done

Creatures reaching 0 HP become incapacitated rather than dying instantly.
Incapacitated creatures fall over (sprite rotated 90 degrees), cannot act,
and begin bleeding out — HP continues to decrease. True death occurs when
HP reaches the negative of max HP (e.g., a creature with 100 max HP dies
at -100 HP). The red HP bar is replaced with a dark-gray-on-black bar
during incapacitation. This creates a rescue window (see
F-rescue) and makes combat feel less binary.

**Unblocked:** F-rescue
**Related:** F-anatomy, F-funeral-rites, F-hp-death, F-hp-ui, F-rescue, F-soul-mech, F-starvation-rework

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

**Unblocked by:** F-engagement-style

#### F-los-tuning — Line-of-sight tuning (terrain tolerance, tall creature bonus)
**Status:** Todo

Line of sight feels too constrained — elves lose sight of targets that go
over small terrain bumps. Two improvements:

1. **Terrain tolerance:** LOS checks should be more forgiving for minor
   elevation changes (e.g., a one-voxel hill shouldn't fully block sight).
   Consider allowing LOS rays to pass through partial obstructions or adding
   a tolerance margin for small obstacles.

2. **Tall creature bonus:** Large creatures like trolls should be easier to
   spot. Species height/size should factor into detection and LOS — a troll
   standing behind a small hill is still visible because it towers above it.

Related: F-hostile-detection already notes that height makes detection range
ineffective across tree levels. This item addresses the horizontal/terrain
case specifically.

**Related:** F-hostile-detection

#### F-military-armor — Military equipment auto-equip and slot validation
**Status:** Done

Military equipment acquisition auto-equips wearable items (armor and clothing) on pickup, displacing existing items in the same slot. Displaced items remain in inventory unequipped. Equipment wants are validated to prevent multiple wearables in the same equip slot — rejected with a player notification sim-side and an inline error UI-side. The wants_editor supports an `enforce_unique_equip_slots` mode for military use. Decoupled from F-armor (durability/damage-reduction integration) — the equip mechanic stands alone.

**Unblocked by:** F-armor, F-military-equip
**Related:** F-military-groups

#### F-military-campaign — Send elves on world expeditions
**Status:** Todo · **Phase:** 8+ · **Refs:** §26

Send elf parties on expeditions in the wider world with direct tactical
control (unlike Dwarf Fortress's hands-off approach).

**Blocked by:** F-combat, F-military-org
**Related:** F-world-map, F-zone-world

#### F-military-equip — Military group equipment acquisition
**Status:** Done

Military groups have an `equipment_wants` field — a list of (item kind, material filter, quantity) entries configurable via the military panel UI. The default "Soldiers" group starts with 1 bow + 20 arrows; new groups start empty.

Heartbeat Phase 2b¾ runs after moping but before personal wants: (a) drops unowned items that don't satisfy any military want or are owned by another creature, (b) creates `AcquireMilitaryEquipment` tasks for unsatisfied wants. This task works like `AcquireItem` but does not change item ownership on pickup.

The reusable `WantsEditor` widget (wants_editor.gd) provides the two-step item kind → material filter picker UI, shared by both building logistics and military equipment panels.

Spawn changes: elves start with 0 bows/arrows; ground pile includes bows, arrows, and armor.

**Unblocked:** F-military-armor
**Related:** F-military-groups

#### F-military-groups — Military group data model and configuration
**Status:** Done

MilitaryGroup table in SimDb with civ_id FK (cascade on civ delete). Auto-increment PK. Fields: name, is_default_civilian (bool, invariant: exactly one per civ), engagement_style (`EngagementStyle` struct with weapon preference, ammo exhaustion, initiative, and disengage threshold — replaces old Fight/Flee `HostileResponse`). Two default groups per civ during worldgen (Civilians with passive/100% disengage, Soldiers with aggressive/0% disengage). Implicit civilian membership: creature `military_group: None` means civilian (governed by civ's default civilian group settings), `Some(group_id)` means explicitly assigned. Civilian count computed as total civ creatures minus assigned creatures. Commands: CreateMilitaryGroup, DeleteMilitaryGroup (reject for civilian group, nullify members), ReassignMilitaryGroup, RenameMilitaryGroup, SetGroupEngagementStyle. `should_flee()` updated to check engagement style.

UI: Military panel opened via separate Military [M] toolbar button. Summary page lists groups with member counts and initiative, click to navigate to detail. Detail page: engagement style controls (initiative/weapon preference/ammo exhaustion cycle buttons, disengage threshold slider), rename, delete (non-civilian only), scrollable member list with reassign buttons. Reassignment overlay (modal) lists groups for quick reassignment. Creature info panel shows military group name as clickable link to the group's detail view.

**Draft:** docs/drafts/military_groups.md

**Draft:** docs/drafts/combat_military.md (§1)

**Related:** F-engagement-style, F-military-armor, F-military-equip, F-patrol

#### F-military-org — Squad management and organization
**Status:** Todo · **Phase:** 8+ · **Refs:** §16

Organize elves into military squads with patrol routes, defensive
positions, and alert levels.

**Blocked by:** F-combat
**Blocks:** F-military-campaign

#### F-move-spread — Spread destinations for multi-creature move commands
**Status:** Done

When a move or attack-move command targets a single location with multiple
selected creatures, automatically spread their destinations to nearby
nav nodes rather than stacking them all on the same voxel. Standard RTS
behavior — pick the closest available node for each creature, expanding
outward from the target. Applies to both regular move and attack-move
commands for military groups and ad-hoc selections.

**Related:** F-rts-selection

#### F-night-predators — Nocturnal predators
**Status:** Todo

Different threat profile at night — nocturnal creatures that climb, fly,
or hunt in darkness. Giant spiders, owlbears, shadow cats. Creates pressure
to have defenses and lighting, and makes the day/night cycle mechanically
meaningful beyond just pacing. Depends on F-day-night.

**Blocked by:** F-day-night
**Related:** F-day-night

#### F-path-combat — Combat path definitions and player assignment
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Combat path definitions (Warrior, Archer, Guard) and player assignment UI.
Combat paths are always player-assigned — the player explicitly sets an
elf's combat path via SimAction::AssignPath. Leveling grants fixed stat
bonuses (STR/CON for warriors, DEX/PER for archers) that flow through
the existing creature stat multiplier pipeline via effective_stat(). XP
gained from combat actions (melee hits, ranged kills, damage taken). All
combat-path elves of the same type and level behave identically — no
per-elf perk divergence, keeping the system RTS-friendly.

**Blocks:** F-path-specialize
**Unblocked by:** F-path-core
**Related:** F-buff-system

#### F-patrol — Patrol command for military groups
**Status:** Todo · **Phase:** 5

Patrol command for military groups. Select a group and issue a patrol
between two or more waypoints; the group walks the route repeatedly,
engaging hostiles encountered along the way. Useful for guarding
perimeters and trade routes.

**Related:** F-military-groups

#### F-per-detection — Perception stat modifies hostile detection range
**Status:** Done

Perception stat applies an exponential multiplier to the species-level
`hostile_detection_range_sq` in SpeciesData. Higher PER = detects hostiles
from further away. Uses the same 2^20 fixed-point stat multiplier table
as other stats. Depends on F-creature-stats for the Perception trait.

**Related:** F-creature-stats, F-hostile-detection, F-spell-cloak, F-stealth

#### F-phased-archery — Phased archery (nock/draw/loose) with skill-gated mobility
**Status:** Todo

Break the shoot action into three discrete phases: nock (attach arrow to
bowstring), draw/aim (pull back and acquire target), and loose (release —
near-instantaneous). Each phase has its own duration and movement rules:

- **Nock:** moderate duration. Must be stationary at novice skill; can nock
  while moving at moderate skill.
- **Draw/Aim:** longer duration, determines accuracy. Must be stationary at
  novice/moderate skill; can draw while moving at high skill (reduced
  accuracy penalty).
- **Loose:** instantaneous or near-instantaneous.

Skill-gated mobility makes experienced archers dramatically more effective
at skirmishing (F-skirmish) — a novice must stop, nock, stop, aim, loose,
then start moving again, while an expert can do the entire sequence on the
run. Could be implemented via cooldowns per phase or as a state machine on
the creature's action. Interacts with F-creature-stats (Dexterity for
nocking speed, Perception for aim time) and F-skirmish (mobile shooting
enables effective kiting).

**Related:** F-creature-stats, F-shoot-action, F-skirmish

#### F-projectiles — Projectile physics system (arrows)
**Status:** Done

SubVoxelCoord type (i64 per axis, 2^30 sub-units per voxel). Projectile entity table in SimDb with inventory-based payload (FK nullify on shooter, FK cascade on inventory). Ballistic trajectory with symplectic Euler integration (velocity updated before position). ProjectileTick batched event — one event per tick while any projectiles are in flight, advances all projectiles. Per-tick: save prev_voxel, apply gravity to velocity, apply velocity to position, check voxel collision (solid → surface impact, ground pile at prev_voxel), check creature collision (spatial index), check bounds (out of world → despawn). Momentum-based damage formula computed at impact time from velocity + item properties (linear in speed, not quadratic). Rendering: projectile_renderer.gd (pool pattern, thin elongated CylinderMesh oriented along velocity vector), SimBridge returns packed position+velocity arrays, interpolation via position + velocity * fractional_offset.

**Bounds check must be performed on i64 sub-voxel coordinates BEFORE converting to VoxelCoord via `as i32`**, to prevent silent truncation.

**ProjectileTick scheduling guard:** Schedule a ProjectileTick event if and only if the projectile table was empty before this spawn (count went from 0 → 1). Prevents duplicate scheduling when multiple archers fire on the same tick.

**Draft:** docs/drafts/combat_military.md (§4)

**Related:** F-friendly-fire, F-spatial-index, F-spell-ench-arrow, F-spell-ice-shard

#### F-raid-detection — Raid detection gating and stealth spawning
**Status:** Todo

Undetected raiders are invisible to the player; revealed when a friendly
creature detects them within hostile detection range. Raid alert fires on
first detection rather than on spawn. Periodic raid triggering based on
game progression replaces the current debug-only button.

**Related:** F-enemy-raids, F-fog-of-war

#### F-raid-polish — Raid polish: military groups, provisions for long treks
**Status:** Todo

Raiders should receive quality-of-life improvements for the bigger world:

- Raiders should be organized into their civ's equivalent of a 'soldier' military group (currently they spawn as ungrouped hostiles)
- Raiders should carry provisions (food) so they can survive the long trek from map edge to the tree on a 1024x1024 world — without food they may starve or become too weak to fight before arriving
- Raiders should consume provisions during the march, using the existing food/hunger system

#### F-rescue — Rescue and stabilize incapacitated creatures
**Status:** Todo

Allied creatures can rescue incapacitated friendlies — carry them to safety,
apply first aid to stabilize bleeding. A complex feature involving new task
types, pathfinding with a carried creature, and medical/stabilization
mechanics. Design TBD.

**Unblocked by:** F-incapacitation
**Related:** F-herbalism, F-incapacitation

#### F-skirmish — Ranged skirmish/kite behavior (shoot-retreat loop)
**Status:** Todo

Ranged creatures with a skirmish engagement style maintain distance from
their target, retreating while shooting — classic "kiting" behavior.
When an enemy closes to within a comfort range, the creature moves away
to re-establish range before firing again. Requires F-engagement-style
to provide the configuration knobs (weapon preference, disengage
threshold) that determine when a creature uses this tactic.

**Unblocked by:** F-engagement-style
**Related:** F-phased-archery, F-shoot-action

#### F-spatial-index — Creature spatial index for voxel-level position queries
**Status:** Done

BTreeMap<VoxelCoord, Vec<CreatureId>> on SimState, #[serde(skip)], rebuilt on load from Alive creatures. Maintained at every position mutation point (wander, walk_toward_task, handle_creature_movement_complete, resnap_creatures, spawn, death). Centralized update_creature_position() helper. Multi-voxel creatures (trolls 2x2x2) register at all occupied voxels. Used by projectile hit detection and hostile detection scanning. Note: BTreeMap with VoxelCoord lexicographic ordering does NOT support efficient 3D range queries — detection scans are O(n) over all creatures with a squared-distance filter, not range queries.

**Rebuild ordering:** The spatial index rebuild must run AFTER species_table is populated (footprint data comes from SpeciesData in config). The species_table is #[serde(skip)] and rebuilt from config after deserialization. If the spatial index rebuild runs before species_table is populated, the footprint lookup for large creatures will fail. Same ordering constraint as the nav graph rebuild — both depend on config-derived data being available.

**Draft:** docs/drafts/combat_military.md (§4 "Creature Spatial Index")

**Related:** F-projectiles

#### F-spell-berserk — Berserk frenzy buff (damage up, uncontrollable)
**Status:** Todo

Buff an allied (or enemy) creature: increased damage and attack speed,
but the target attacks the nearest creature regardless of allegiance
(friend or foe) for the duration. High risk/reward — best used on
expendable conjured creatures or enemies near their own allies. Uses
both the buff system (stat modifiers) and creature control (AI override).

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-buff-system, F-creature-control, F-spell-system
**Related:** F-war-magic

#### F-spell-blink — Short-range teleport spell
**Status:** Todo

Short-range teleport (5-10 voxels). Instant cast, high mana cost,
moderate cooldown. Destination must be a walkable voxel (validated
against nav graph). Line-of-sight not required. Useful for
repositioning healers, escaping melee, or reaching elevated platforms.
One of the lighter spells to implement — mostly nav validation.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Related:** F-war-magic

#### F-spell-cloak — Invisibility spell on self or nearby allies
**Status:** Todo

Invisibility on self, or AoE cloak on nearby allies. Enemies cannot
detect cloaked creatures unless they get very close (PER-based detection
check). Cloak breaks on attack or spell cast. Duration-limited or
mana-drain-over-time. Ties into the stealth system (F-stealth) and
detection mechanics (F-per-detection).

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system, F-status-effects
**Related:** F-per-detection, F-stealth, F-war-magic

#### F-spell-ench-arrow — Enchanted arrow shot with mana cost and hit effects
**Status:** Todo

Special arrow shot that costs mana and applies an effect on hit (burn
DOT, slow, pierce through multiple targets, etc.). Uses normal archery
stats (STR/DEX) plus mana. Could be autocastable (every Nth arrow is
enchanted, or always enchanted while autocast is on and mana permits).
Bridges the archery and magic systems — an elf doesn't need to be a
pure mage to use magic in combat.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-buff-system, F-spell-system
**Related:** F-projectiles, F-shoot-action, F-war-magic

#### F-spell-gust — Gust AoE knockback cone spell
**Status:** Todo

AoE knockback in a cone in front of the caster. Pushes creatures away.
No damage on its own, but pushing enemies off platforms causes fall
damage (requires F-creature-gravity). Cheap mana cost, moderate
cooldown. Manual cast only — directional spells don't autocast well.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Unblocked by:** F-creature-gravity
**Related:** F-war-magic

#### F-spell-ice-shard — Ice Shard ranged magic projectile with autocast
**Status:** Todo

Mana-fueled ranged projectile. Creates a sim projectile like arrows but
uses magic stats (INT) instead of physical stats (STR/DEX). Flat mana
cost per shot, no ammo consumed. Autocastable: elf fires at enemies in
range like an archer but mana-limited instead of ammo-limited. Key
distinction from archery: no equipment dependency, different stat
scaling, mana as the limiting resource.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Related:** F-projectiles, F-war-magic

#### F-spell-mend — Mend healing spell with autocast healer AI
**Status:** Todo

Single-target channeled heal on a friendly creature. Mana cost per tick
of healing (not upfront). Heal amount scales with caster's INT. When
autocast is enabled, the elf behaves like a StarCraft Medic: follows
nearby injured friendlies, prioritizes lowest HP%, heals automatically,
does not initiate combat. Manual cast: click target ally, caster walks
to them and channels. Self-heal TBD (leaning no).

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Related:** F-war-magic

#### F-spell-mind-ctrl — Temporary mind control of enemy creature
**Status:** Todo

Temporarily take control of an enemy creature. High mana cost, duration
contested (caster INT vs target WIL — strong-willed targets break free
faster). Controlled creature fights for the caster's side with
independent AI. When the effect ends, original allegiance restores.
Manual cast only.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-creature-control, F-spell-system
**Related:** F-war-magic

#### F-spell-rootbind — Rootbind immobilize spell (contested duration)
**Status:** Todo

Single-target immobilize spell. Roots entangle the target, preventing
movement. Duration is contested: scales with caster's INT and inversely
with target's STR (stronger targets tear free faster). Flat upfront mana
cost. Not autocastable — stuns are too tactically valuable to spend
automatically. Target can still attack in melee range while rooted.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Related:** F-status-effects, F-war-magic

#### F-spell-summon — Conjure temporary allied creature
**Status:** Todo

Conjure a temporary allied creature at a target location. High mana
cost, long cooldown. The summoned creature has independent AI and fights
for the caster's side. Disappears when duration expires or killed.
Different summon types (elephant, giant hornet, bee swarm) could be
separate spells or one spell with variants. Uses the conjured creatures
infrastructure (F-conjured-creatures).

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-conjured-creatures, F-spell-system
**Related:** F-war-magic

#### F-spell-thornbriar — Thornbriar zone spell (slow + damage area)
**Status:** Todo

Area-of-effect terrain spell. Grows a patch of thorny bushes at a target
location. Creatures moving through the area take damage per tick and move
at reduced speed. Duration-limited (bushes wither). Useful for blocking
chokepoints, slowing charges, or creating kill zones for archers.
Area targeting: click a point, affects a radius.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system, F-terrain-manip
**Related:** F-war-magic

#### F-status-effects — Generic creature status effect system
**Status:** Todo

Generic system for temporary status effects on creatures. Each effect has
an ID, source creature, remaining duration (ticks), and mechanical
modifier. First effect: Immobilized (prevents movement, used by
Rootbind). System is generic to support future effects (slowed, burning,
poisoned, cloaked, berserk, etc.). Status effects stored in a sim table,
ticked down each heartbeat, removed on expiry.

**Draft:** docs/drafts/war_magic.md

**Blocks:** F-spell-cloak, F-spell-system
**Related:** F-buff-system, F-spell-rootbind, F-war-magic

#### F-stealth — Camouflage and stealth mechanics
**Status:** Todo

Forest-based concealment mechanics. Elves can hide in foliage, set ambushes,
use terrain for cover. Detection depends on movement, cover density, and
observer perception (F-per-detection). Enables ambush tactics, hidden
sentries, and scouting. Cloaks/capes (F-cloak-slot) improve concealment.
Could interact with F-fog-of-war for asymmetric information.

**Related:** F-cloak-slot, F-fog-of-war, F-per-detection, F-spell-cloak

#### F-terrain-manip — Temporary voxel/zone placement with expiry
**Status:** Todo

System for spells to place temporary voxels or effect zones in the world
that expire after a duration. Zones have per-tick effects on creatures
inside them (damage, slow, etc.). Temporary voxels may block or impede
pathing. On expiry, voxels are removed and nav graph is restored. First
user: Thornbriar spell. Future: ice walls, fire zones, magical barriers.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-spell-system
**Blocks:** F-spell-thornbriar

#### F-voxel-exclusion — Creatures cannot enter voxels occupied by hostile creatures
**Status:** Done · **Phase:** 3

Creatures should not be able to enter a voxel already occupied by a hostile creature (and vice versa). Currently multiple creatures freely share voxels regardless of faction. Needs pathfinding and/or movement-step checks to enforce. Edge case: if creatures are already sharing a voxel when hostility begins, behavior is TBD (push apart, allow temporary overlap, etc.).

#### F-war-animals — Train tamed creatures for combat
**Status:** Todo

Train tamed creatures for combat — attack commands, combat AI, charging
behavior. War-trained animals can be assigned to military groups and
respond to attack-move orders. Species determines combat style (wolves
bite, elephants trample, birds dive).

**Related:** F-cavalry, F-civ-pets

#### F-war-magic — War magic (combat spells)
**Status:** Todo

Offensive and defensive magic usable in combat. First pass: three spells
(Mend, Rootbind, Ice Shard) with per-elf mana pools and SC-style
command card controls. Creatures with sufficient mana and training can
cast spells as active abilities (bound via F-ability-hotkeys). Spells
use mental stats (INT, WIL) for effectiveness. Spell learning via
F-bldg-library; debug grant for initial development. Future spells
include conjured creatures, mind control, cloaking, thornbriar,
enchanted arrows, and blink teleport.

**Draft:** docs/drafts/war_magic.md

**Related:** F-ability-hotkeys, F-bldg-library, F-elf-mana-pool, F-mana-system, F-spell-berserk, F-spell-blink, F-spell-cloak, F-spell-ench-arrow, F-spell-gust, F-spell-ice-shard, F-spell-mend, F-spell-mind-ctrl, F-spell-rootbind, F-spell-summon, F-spell-system, F-spell-thornbriar, F-status-effects

### World Expansion & Ecology

#### F-bigger-world — Larger playable area
**Status:** Done

Increase world size to 1024×255×1024. The tree and terrain start at
~y=50, leaving room below for underground content (caves, drow, mining).
Noisy hilly dirt terrain extends to the full map extent. Camera starts
correspondingly higher.

No chunk streaming needed — the RLE voxel storage (F-rle-voxels),
RLE-aware mesh gen (F-mesh-gen-rle), and LookupMap nav spatial index
(F-nav-gen-opt) already scale with surface complexity rather than world
volume. The tiling texture system (F-tiling-tex) eliminates per-face
atlas overhead that would otherwise be prohibitive at this scale.

**Unblocked by:** F-megachunk, F-nav-gen-opt, F-rle-voxels, F-tiling-tex
**Related:** F-lesser-trees, F-multi-tree, F-world-map, F-zone-world

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

**Unblocked:** F-enemy-raids
**Related:** F-civ-pets, F-dwarf-fort-gen, F-enemy-raids, F-fruit-variety, F-settlement-gen, F-traders

#### F-cultural-drift — Inter-tree cultural divergence
**Status:** Todo · **Phase:** 7 · **Refs:** §7, §18

Elves on different trees develop distinct traditions, art styles, and
social norms over time.

**Blocked by:** F-multi-tree, F-personality

#### F-day-night — Day/night cycle and pacing
**Status:** Todo · **Refs:** §27

Length of in-game day. Affects pacing, fruit production, sleep schedules.
Open design question (§27).

**Blocks:** F-night-predators
**Related:** F-night-predators

#### F-dwarf-fort-gen — Underground dwarf fortress generation
**Status:** Todo

Procedural generation of underground dwarf fortresses. Multi-level
excavated halls, workshops, living quarters, treasury vaults, defensive
chokepoints. Significantly more complex than surface settlement generation
due to 3D interior layout, structural considerations, and the need to
carve into existing terrain rather than place on top of it. Separate from
F-settlement-gen due to complexity.

**Related:** F-civilizations, F-settlement-gen, F-zone-world

#### F-forest-ecology — Forest floor ecology (flora, fauna, foraging)
**Status:** Todo

The forest floor as a living system — mushrooms, undergrowth, ferns, animal
trails, fallen logs, streams. Provides foraging resources (herbs, fungi,
small game), affects pathfinding and visibility, and makes the ground level
visually rich rather than flat terrain. Interacts with F-lesser-trees,
F-herbalism, and F-seasons (seasonal variation in flora).

**Related:** F-herbalism, F-insect-husbandry, F-lesser-trees, F-seasons, F-wild-grazing

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

#### F-lesser-trees — Lesser trees (non-sentient, resource/ecology)
**Status:** Done

Non-sentient trees that populate the forest floor — smaller than the great
tree, providing ecological variety. Not bonded to a spirit, cannot host
elven construction. May be harvestable for resources in future. Interact
with F-uplift-tree (can be awakened into a great tree).

**Phase 1 (this PR):** Worldgen placement of ~500 lesser trees across
the full world via rejection-sampled random positions. Config-driven via
`LesserTreeConfig` (count, min distances, max placement attempts). Six
tree profiles of varying size and shape: deciduous, conifer, tall
straight, thick oak, bushy, and sapling. Trees use the same energy-based
generation algorithm as the great tree but with much smaller profiles
sized for real-life trees at 2m/voxel (~3-20 voxels tall). Both the main
tree and lesser trees are sunk into the terrain surface so trunks emerge
naturally from the ground. All trees (great and lesser) stored in the
`trees` SimDb table with no owner, no mana, no fruit.

**Future:** Poisson disk sampling for better spatial distribution,
harvestable wood, fruit-bearing lesser trees, species variety, interaction
with F-uplift-tree.

**Unblocked:** F-uplift-tree
**Related:** F-bigger-world, F-forest-ecology, F-uplift-tree, F-zone-world

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
**Unblocked by:** F-tree-db
**Related:** F-bigger-world, F-multiplayer, F-settlement-gen, F-tree-capacity, F-tree-db, F-tree-species, F-uplift-tree, F-zone-world

#### F-rm-floor-extent — Remove floor_extent and ForestFloor layer
**Status:** Done

Remove the vestigial `floor_extent` config field and the `ForestFloor`
voxel type. The terrain system (hilly dirt via value noise) now covers
the world without needing a separate green rectangle under the dirt.

Touches: config.rs (remove field + default), tree_gen.rs (terrain
generation bounds), world.rs (init_terrain_parallel), nav.rs
(ForestFloor edge type references), mesh_gen.rs (ForestFloor geometry
handling), species.rs (allowed_edge_types), raid.rs (perimeter band
calculation), types.rs (VoxelType enum variant), sim tests.

#### F-root-network — Root network expansion and diplomacy
**Status:** Todo · **Phase:** 7 · **Refs:** §2

Player grows roots toward other trees. Diplomacy phase: mana offerings
convince trees to join the network. Expands buildable space and perception
radius.

**Blocked by:** F-multi-tree
**Unblocked by:** F-mana-system
**Related:** F-fog-of-war, F-forest-radar

#### F-settlement-gen — Procedural NPC settlement generation
**Status:** Todo

Procedural generation of NPC civilization settlements when a zone
transitions from Seed to Active. Each civ species has a distinct
settlement style:

- **Elves:** Great tree with platforms, walkways, and grown structures
  (reuses existing tree gen and construction systems).
- **Humans:** Ground-level buildings — houses, walls, market squares,
  roads. Wood and stone construction.
- **Goblins:** Crude ground-level camps — palisades, firepits, huts.
  Possibly cave-adjacent.
- **Orcs:** Fortified ground-level compounds — thick walls, towers,
  training grounds.
- **Dwarves:** See F-dwarf-fort-gen (underground fortresses are
  complex enough to warrant a separate feature).

Generation uses civ metadata (population, wealth, age, culture tags)
to determine settlement size, building types, and defensive structures.
The result is placed into the zone's voxel grid with pre-built structures
registered in SimDb. NPC creatures are spawned with appropriate equipment
and assignments.

**Open question:** Whether visited zones preserve voxel state permanently
or regenerate from updated civ metadata on revisit is UNDECIDED. Permanent
preservation is simpler conceptually and avoids weird resets, but has
memory/save-size implications for a world with many visited zones.
Regeneration allows background-state evolution (population grows, buildings
added/repaired) to be reflected visually, but risks discarding player
actions (battle damage, stolen loot, placed structures). This is a major
architectural decision that affects F-zone-world's background tick design.

**Related:** F-civilizations, F-dwarf-fort-gen, F-enemy-raids, F-multi-tree, F-zone-world

#### F-tree-disease — Tree diseases and parasites
**Status:** Todo

Threats to the tree itself — fungal infections, boring insects, parasitic
vines. Requires elves to diagnose (herbalism or tree-tending skill) and
treat (pruning, poultices, controlled burns). Untreated disease can spread
to other parts of the tree, weaken structural integrity, or reduce mana
generation. A different kind of crisis from raids — slow-building and
requiring specialized knowledge rather than military force.

**Related:** F-herbalism

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

#### F-weather — Weather within seasons
**Status:** Todo · **Refs:** §27

Rain, wind, storms within seasons. Could affect mood, fire spread, and
construction difficulty. Open design question (§27).

**Related:** F-cloak-slot, F-fire-ecology, F-infra-decay, F-seasons

#### F-wild-grazing — Wild animal herbivorous food cycle
**Status:** Todo

Wild herbivorous animals graze on grass and forage for wild fruit instead
of starving. Grazing consumes ground-level vegetation, foraging targets
wild fruit sources. Different species prefer different food sources.
Fixes the current problem where wild animals all starve to death.
Foundation for both forest ecology and domesticated animal feeding.

**Blocks:** F-animal-husbandry, F-herding
**Related:** F-forest-ecology

#### F-worldgen-framework — Worldgen generator framework
**Status:** Done

Worldgen entry point called during StartGame that runs generators in
defined order (tree → fruits → civs → knowledge). Dedicated worldgen
PRNG seeded from world seed. WorldgenConfig subsection of GameConfig
grouping FruitConfig and CivConfig. Small plumbing feature — establishes
the pattern for generator sequencing.

**Draft:** `docs/drafts/elfcyclopedia_civs.md` §Worldgen Framework

#### F-zone-world — Zone-based world with fidelity partitioning
**Status:** Todo

Zone-based world architecture within a single unified sim. The world is
partitioned into spatial zones, each a bounded voxel grid, but all zones
share one SimDb, one save file, one deterministic simulation.

**Zone states:**
- **Active:** Full voxel sim — pathfinding, creature actions, construction,
  combat. This is the current game, applied per-zone.
- **Background:** Coarse heartbeat — population changes, repair/expansion
  progress, resource accumulation. No pathfinding, no voxel-level movement.
  Used for NPC civ towns, unoccupied areas.
- **Seed:** Never visited. Exists as worldgen parameters + civ metadata.
  Deterministically generated into full voxels on first activation.

**DB changes:** Zone table in SimDb (zone_id, zone_state, world_map_pos,
terrain_type, owning_civ, seed, etc.). zone_id column added to creature,
structure, item, and other spatially-located tables.

**Fidelity transitions:** Deterministic rules govern when zones change state.
Active→Background when no player-relevant creatures remain. Seed→Active
on first player creature entry (or other deterministic trigger). Background
zones tick on a coarse schedule. All transitions must be deterministic for
multiplayer/replay compatibility.

**Inter-zone travel:** Creatures moving between zones have a world-map
travel phase with nontrivial duration. Zones are not topologically
connected at edges — implied uninteresting space between them. Encounters
en route (e.g., intercept a raiding party) generate a battle zone on the
fly from terrain type at the world map location.

**No separate sims:** Same sim code, same event queue, same DB. The zone
is a simulation fidelity hint, not a separate world.

**Related:** F-bigger-world, F-dwarf-fort-gen, F-enemy-raids, F-forest-radar, F-lesser-trees, F-military-campaign, F-multi-tree, F-settlement-gen, F-tree-db, F-world-map

### Soul Mechanics & Magic

#### F-elf-mana-pool — Per-elf mana pool wired to WIL/INT stats
**Status:** Done

Wire WIL and INT stats to per-elf mana pool size and mana regeneration
rate. WIL determines mp_max (exponential scaling: +10 WIL = 2x pool).
INT determines mana regen rate. Both use the existing stat multiplier
table. This activates the first mechanical hooks for the currently-inert
mental stats. Prereqs: F-mana-system (done), F-creature-stats (done).

**Draft:** docs/drafts/war_magic.md

**Unblocked:** F-spell-system
**Related:** F-creature-stats, F-mana-scale, F-mana-system, F-war-magic

#### F-forest-radar — Forest awareness radar (world map detection)
**Status:** Todo

Great trees extend awareness through the surrounding forest, detecting
entity movement on the world map outside their home zone. Thematically
tied to the tree's affinity for the surrounding forest canopy and root
network. Requires research (F-bldg-library) and costs continual mana
expenditure (F-mana-system) to maintain. Detection radius scales with
mana investment. Reveals incoming raiding parties, trader caravans, and
wildlife migration on the world map, giving the player advance warning
and time to prepare or intercept.

**Related:** F-bldg-library, F-mana-system, F-root-network, F-world-map, F-zone-world

#### F-magic-items — Magic item personalities and crafting
**Status:** Todo · **Phase:** 8+ · **Refs:** §22

Magic items with emergent personalities from their crafting circumstances
and the souls/emotions imbued in them.

**Related:** F-cloak-slot, F-crafting, F-soul-mech

#### F-mana-scale — Rescale mana to human-readable values and ticks_per_mp_regen
**Status:** Todo

Rescale the mana system from the current 1e15 internal scale to
human-comprehensible values (e.g. mp_max ~100–400, similar to HP).
Replace mana_per_tick with ticks_per_mp_regen (matching the
ticks_per_hp_regen pattern) so regen is expressed as "ticks per 1 MP"
rather than "fractional MP per tick." This simplifies debugging,
tooltips, and config tuning. Requires updating all mana consumers
(construction drain, overflow-to-tree conversion, future spell costs).

**Related:** F-elf-mana-pool

#### F-soul-mech — Death, soul passage, resurrection
**Status:** Todo · **Phase:** 8+ · **Refs:** §19

Elf death, soul passage into trees, possible resurrection, and
soul-powered constructs (golems, animated defenses).

**Related:** F-creature-death, F-funeral-rites, F-incapacitation, F-magic-items

#### F-spell-system — Core spell casting infrastructure (SpellId, commands, mana costs)
**Status:** Todo

Core spell casting infrastructure. SpellId enum, SimCommand::CastSpell
and SimCommand::SetAutocast commands, per-creature spell knowledge
(which spells an elf has learned), autocast state (per-creature
per-spell toggle), mana cost validation, cooldown tracking. This is the
shared foundation — individual spells (Mend, Rootbind, Ice Shard) build
on top. Spell learning deferred to F-bldg-library; for now spells can
be granted via debug command.

**Draft:** docs/drafts/war_magic.md

**Blocked by:** F-status-effects
**Blocks:** F-buff-system, F-conjured-creatures, F-creature-control, F-spell-berserk, F-spell-blink, F-spell-cloak, F-spell-ench-arrow, F-spell-gust, F-spell-ice-shard, F-spell-mend, F-spell-mind-ctrl, F-spell-rootbind, F-spell-summon, F-spell-thornbriar, F-terrain-manip
**Unblocked by:** F-elf-mana-pool
**Related:** F-ability-hotkeys, F-war-magic

#### F-uplift-tree — Uplift lesser tree into bonded great tree
**Status:** Todo

A major magical act: the player tree spirit (or an elf coven) awakens a
lesser tree into a new great tree capable of bonding elves and hosting
construction. Enables expansion beyond a single tree. The uplifted tree
becomes a new entity in F-tree-db and can participate in F-multi-tree.

**Unblocked by:** F-lesser-trees
**Related:** F-lesser-trees, F-multi-tree, F-tree-db

### UI & Presentation

#### B-doubletap-groups — Double-tap selection group recall inconsistently triggers camera center
**Status:** Todo

Double-tapping a number key (1–9) to recall a selection group and center
the camera works inconsistently — sometimes the camera centers, sometimes
it doesn't. The input system is event-driven (no lost events), so the
likely cause is UI focus stealing: the first tap recalls the group and
shows a panel (creature info or group panel), and a focused control in
that panel may consume the second keypress via _gui_input before it
reaches _unhandled_input in selection_controller.gd.

Moving number key handling to _input() (which fires before GUI focus)
fixes the double-tap but breaks the crafting UI which needs number key
input in text fields. The fix needs to be smarter — e.g., check whether
a text-entry control has focus before intercepting number keys, or use
_shortcut_input with an InputMap action that panels can selectively
block.

**Related:** F-selection-groups

#### B-escape-menu — Rename pause_menu to escape_menu and block hotkeys/buttons while it's open
**Status:** Done

Two related fixes for the ESC menu:

1. **Rename pause_menu to escape_menu.** The current name is misleading — in multiplayer the menu doesn't pause the game. Rename pause_menu.gd → escape_menu.gd and update all references (_pause_menu vars, signal names, docstrings, CLAUDE.md mentions).

2. **Block hotkeys and buttons while the escape menu is visible.** Currently in multiplayer (where the tree isn't paused), all gameplay hotkeys (B, T, U, M, I, Y, Space, F1–F3, etc.) and toolbar buttons still fire behind the overlay. The escape menu should suppress these while it's open — either by consuming all key input or by having the toolbar/main check an "escape menu is open" flag.

#### B-first-notification — First notification not displayed (ID 0 skipped by polling cursor)
**Status:** Done

#### B-modifier-hotkeys — Hotkeys should not fire when modifier keys (Ctrl/Shift/Alt) are held
**Status:** Done

Most gameplay hotkeys (B, T, U, M, I, Y, Space, F1–F3, F12, ?) fire even when Ctrl/Shift/Alt are held. They should generally be suppressed when modifiers are active, but some cases may need individual consideration (e.g., Ctrl+1–9 for selection groups already uses modifiers intentionally). Go through each hotkey handler in action_toolbar.gd, main.gd, construction_controller.gd, and selection_controller.gd on a case-by-case basis.

#### B-qem-deformation — QEM decimation visual artifacts
**Status:** In Progress

Chamfer-only mode: the three-pass mesh decimation pipeline (coplanar retri, collinear boundary collapse, QEM edge-collapse) produced visible deformations — triangles bridging across crease boundaries between differently-angled chamfer surfaces, creating obvious bumps, dents, and misshapen surfaces.

**Root cause (identified via OBJ mesh export and analysis):**

The QEM edge-collapse pass was collapsing edges that sit on creases between differently-oriented chamfer surfaces (e.g., an edge chamfer at 45° meeting a corner chamfer at 35°). Each individual collapse had near-zero QEM error (both vertices are near the accumulated planes) and passed the normal-flip check (dot > 0), but the resulting triangle spanned two surfaces at a non-canonical angle — an orientation that doesn't exist in the original chamfered voxel mesh. In a real game chunk, 18 out of 626 decimated triangles (2.9%) had non-canonical normals, with angular deviations of 13–25°.

**Fix (2026-03-25):**

Added a canonical normal check to `collapse_would_flip` in `mesh_decimation.rs`. Chamfered voxel meshes have exactly 26 possible triangle normal directions (6 cardinal, 12 edge-chamfer, 8 corner-chamfer). The check rejects any collapse that would produce a surviving triangle whose normal doesn't match one of these 26 directions (within cos(10°) ≈ 0.985 threshold). This prevents cross-surface bridging without affecting triangle reduction on legitimate coplanar surfaces. The check is gated on `!smoothing_enabled()` so it won't interfere with future smooth-mode LoD decimation.

**Status: NOT FULLY FIXED.** The canonical normal check (applied to all three pipeline stages: retri, collinear, and QEM) fixes the majority of visible deformation artifacts but problems remain. The remaining issues appear regardless of whether QEM-only mode is used, indicating the QEM pass itself still has cross-surface bridging that the canonical normal check doesn't catch.

**IMPORTANT: The canonical normal check is a HACK, not a root-cause fix.** There is a latent bug in how the decimation pipeline handles material boundaries and/or chunk boundaries that causes cross-surface bridging. The canonical normal check papers over it by rejecting the bad output, but only works for chamfer-only mode where the set of valid normals is known. The same underlying bug would produce similar artifacts when decimating smooth-mode meshes (future LoD), where arbitrary normals are valid and we have no canonical set to check against. The bug is not fully fixed until the actual root cause (likely in boundary handling) is identified and corrected.

**Other fixes made during investigation:**
1. Retri collinear centroid — `coplanar_region_retri` created degenerate zero-area fan triangles when a region's boundary polygon was a thin strip.
2. QEM near-degenerate threshold — raised from 1e-10 to 1e-6, added relative area shrinkage guard.
3. Retri sliver fan triangles — added aspect ratio check for fan triangles.
4. QEM unbounded slivers on coplanar surfaces — added aspect ratio check to collapse_would_flip.

**Debug tooling added:**
- OBJ mesh export button (debug toolbar) — exports chunk mesh with and without decimation for comparison in mesh viewers.
- QEM-Only toggle — skips retri+collinear to isolate QEM behavior.
- Point-in-mesh surface sampling tests — raycasting-based inside/outside deformation detection.
- Canonical normal validation tests — verify all triangle normals match the 26 chamfer directions.
- Fuzz testing with randomized smooth heightmaps (200+ seeds).

**Related:** B-chamfer-nonmfld, F-mesh-lod

#### B-start-paused-ui — start_paused_on_load UI desync and missing new-game support
**Status:** Todo

start_paused_on_load pauses the sim on load but has two issues:

1. UI desync: the toolbar shows speed as x1 and unpaused even though the
   sim is paused. The speed controls and status bar do not reflect the
   paused state set by the config.

2. Missing new-game support: the setting only triggers on save loads, but
   should also pause on new game start.

**Related:** F-config-ui

#### F-ability-hotkeys — RTS-style bindable ability hotkeys on creatures
**Status:** Todo

RTS-style ability buttons on selected creatures, with bindable keyboard
shortcuts. When a creature with abilities is selected, ability buttons appear
in the UI (like StarCraft's command card). Abilities include combat magic
(F-war-magic), special species abilities, and potentially other active skills.
Hotkeys are displayed on the buttons and can be rebound.

**Related:** F-spell-system, F-war-magic

#### F-ai-sprites — AI-generated sprite art pipeline
**Status:** Todo · **Phase:** 8+ · **Refs:** §24

Replace placeholder sprites with AI-generated layered art: base body
templates + composited clothing/hair/face layers for visual variety.

#### F-alt-deselect — Alt+click to remove from selection
**Status:** Done · **Phase:** 5

Alt+click a unit to remove it from the current selection. Counterpart
to Shift+click to add. Standard RTS modifier pattern.

**Related:** F-rts-selection

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

#### F-bldg-transparency — Toggle building roof visibility (hide/show)
**Status:** Done · **Phase:** 2

A toggle button that hides building roofs so the player can see and
click on elves inside enclosed structures. Implemented together with
F-ghost-above on a shared branch since both features share UI surface
area (right-edge icon toolbar) and rendering concerns.

**UI:** Square icon button on a vertical toolbar strip along the right
screen edge. Procedurally drawn icon (GDScript `_draw()`): a simple
house shape — roof solidly filled when roofs are visible, roof as a
dotted outline with nothing inside when hidden. Hover tooltip, no text
label, no keyboard shortcut.

**Rendering:** Buildings are rendered as QuadMeshes grouped by face type
(Wall, Window, Door, Ceiling, Floor) via `building_renderer.gd`. Hiding
roofs = toggling visibility on the Ceiling `MultiMeshInstance3D` nodes.
No mesh regen or shader work needed.

**Click-through:** When roofs are hidden, clicking where a roof would be
must select the elf underneath, not the building. The Rust-side
selection/ray intersection logic needs a flag indicating roof-hidden
state so it skips ceiling faces during hit testing.

**State:** Ephemeral — resets each session, not persisted in save files.

**Related:** F-ghost-above, F-roof-click-select, F-wireframe-ghost, F-zlevel-vis

#### F-boundary-decim — Mesh decimation at chunk boundaries
**Status:** Todo

Chamfer-only mode: chunk boundary vertices are currently pinned (never decimated) to prevent seams between adjacent chunks. This leaves the boundary as a dense band of original-subdivision triangles while the interior is beautifully simplified — very visible in wireframe mode on hilly terrain. Only relevant for chamfer-only (non-smooth) rendering; if smoothing is enabled, the decimation pipeline is different.

Possible approaches:
- Cross-chunk coordination: only decimate a boundary vertex if both adjacent chunks agree it's safe. Requires knowing the neighbor chunk's mesh state, which complicates the currently-independent per-chunk pipeline.
- Collinear-only boundary decimation: allow the collinear boundary collapse pass to operate on chunk-boundary vertices (since it only removes vertices on straight lines between neighbors, this might be safe even without coordination). Keep QEM away from boundary vertices. The collinear pass is deterministic given the same boundary geometry, so both chunks would make the same decision independently.
- T-junction tolerance: allow T-junctions at chunk boundaries (one side decimated, other not). May cause hairline cracks depending on how Godot handles them. Needs investigation.
- Expanded border: increase the smooth mesh border so decimation can run on a larger overlapping region, with both chunks producing identical results in the overlap. May be expensive.

Key concern: T-junctions. If one chunk decimates a boundary edge midpoint and the adjacent chunk doesn't, the seam has a T-junction (one side has vertex A-B-C, other side has A-C with B missing). Whether this causes visible cracks depends on float precision at shared vertex positions. Needs testing.

The collinear-only approach seems most promising as a first step — it's local, deterministic, and only merges vertices that are provably on the same line. Even if only midpoints on straight boundary edges are merged, it would significantly reduce the boundary density on flat terrain (floors, walls).

**Related:** F-mesh-lod

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

**Related:** F-follow-multi

#### F-command-queue — Shift+right-click to queue commands
**Status:** Done · **Phase:** 5

Shift+right-click appends a command to the selected units' queue
instead of replacing it. An unshifted right-click replaces the queue
as it does today. Queued commands execute sequentially as each one
completes (move: arrived, attack: target dead, attack-move: arrived
at destination).

**Goal:** With an elf selected, right-click on A sends it to A (as
today). Shift+right-click on B then shift+right-click on C queues
B after A and C after B — the elf goes A → B → C in order. An
unshifted right-click at any point cancels the entire queue and
replaces it with the new command.

**Approach — two new fields on the Task table:**

1. `restrict_to_creature_id: Option<CreatureId>` (indexed) — When
   set, only this creature may claim the task. `find_available_task`
   skips tasks whose `restrict_to_creature_id` doesn't match the
   searching creature. Sits alongside the existing `required_species`
   filter.

2. `prerequisite_task_id: Option<TaskId>` (indexed) — When set, the
   task stays unavailable until its prerequisite reaches `Complete`.
   For a queue A → B → C, B points to A and C points to B (a linked
   list). `find_available_task` skips tasks whose prerequisite isn't
   complete.

**Queue creation (GDScript → SimCommand):**

- Unshifted right-click: issue the command as today (single task,
  no restrict/prerequisite fields). Before creating it, cancel/remove
  any existing queued tasks for the creature that have
  `origin == PlayerDirected` (query by `restrict_to_creature_id`
  filtered to player-directed tasks only, so future non-command
  restricted tasks are unaffected).
- Shift+right-click: create the new task with
  `restrict_to_creature_id` set to the commanded creature and
  `prerequisite_task_id` set to the creature's current tail task
  (its `current_task` if no queue exists, or the last queued task).

**Queue survival (CRITICAL design rule):** The command queue must
survive autonomous interruptions. Only two things should clear the
queue:

1. An unshifted player command (explicit replacement).
2. Completing all queued tasks naturally.

The queue must NOT be cleared by:
- Hostile auto-engage (and GoTo should not auto-engage at all — see
  below).
- Fleeing (once the creature recovers composure, it should resume
  its queued commands).
- Any other autonomous interruption.

Death is a special case: the creature is gone, so cleanup is fine,
but this is resource cleanup, not queue cancellation in the gameplay
sense.

**Implementation:** `cleanup_and_unassign_task` currently calls
`cancel_dependent_tasks`, which wipes the entire queue on any
interruption. This is too aggressive. The cascade should only
happen when the interruption source is a player command replacement
(i.e., the `cancel_creature_queue` path triggered by an unshifted
command). For autonomous interruptions (flee, hostile pursuit,
hunger/sleep preemption), the queue should be preserved:

- When a creature is autonomously interrupted mid-queue (e.g., flee),
  `cleanup_and_unassign_task` should NOT call `cancel_dependent_tasks`.
  Instead, the dependent tasks stay Available with their prerequisite
  still pointing to the now-Complete interrupted task. Since the
  prerequisite is Complete, `find_available_task` will pick up the
  next queued task once the creature becomes idle again after the
  interruption resolves.

- The key distinction is the interruption source: player-command
  preemption (via `command_directed_goto` etc.) explicitly calls
  `cancel_creature_queue` before preempting. Autonomous interruption
  should not cascade.

**GoTo should not auto-engage hostiles:** In standard RTS behavior,
a move command (GoTo) does not stop to fight — the unit walks past
danger. Only attack-move stops to engage hostiles en route. Currently,
the activation pipeline may trigger hostile auto-engage even during
a player-directed GoTo, which would interrupt the GoTo (and with the
old cascade behavior, wipe the queue). The fix:

- During `execute_task_behavior` for GoTo tasks with
  `origin == PlayerDirected`, skip the hostile auto-engage check.
  The creature walks to its destination regardless of nearby hostiles.
- This matches RTS convention: right-click = "go here no matter
  what", F-click (attack-move) = "go here but fight anything you
  see".
- Autonomous GoTo (e.g., going home to sleep) can still auto-engage
  since the creature isn't under direct player control.

**Cancellation cascade (revised):** `cancel_dependent_tasks` is
called from `cleanup_and_unassign_task`. The revised behavior:

- Remove the `cancel_dependent_tasks` call from
  `cleanup_and_unassign_task` entirely.
- Player-command handlers already call `cancel_creature_queue`
  before preempting, which handles the "replace queue" case.
- `handle_creature_death` already calls `cancel_creature_queue`
  after `interrupt_task`, which handles the death case.
- For autonomous interruptions (flee, hunger, sleep), the queue
  survives because neither `cancel_dependent_tasks` nor
  `cancel_creature_queue` is called.

**Completion flow:** When a task completes normally, no special
cascade is needed — dependent tasks simply become available because
their prerequisite is now `Complete`. `find_available_task` will
pick them up on the creature's next activation. This also handles
queue resumption after autonomous interruption: the interrupting
task (flee, eat, sleep) completes, the creature becomes idle,
`find_available_task` finds the next queued task whose prerequisite
(the interrupted GoTo, now Complete) is done.

**Related:** F-rts-selection

#### F-config-file — Game config file (user://config.json)
**Status:** Done · **Phase:** 2

General game configuration file at user://config.json. Created with
defaults on first launch, read on startup, written when settings change.
Implemented as a GDScript autoload (GameConfig singleton) so any script
can read settings.

Settings:
- player_name: String (default "") — persistent player display name
  (migrated from the old user://player.cfg used by F-player-identity)
- start_paused_on_load: bool (default false) — load saves in paused state

The autoload exposes get_setting/set_setting methods (set_setting
auto-saves) and override_setting(key, value) for test harness use —
tests can override config values in memory without touching the file,
keeping test runs side-effect-free.

File format: flat JSON object with string keys. Unknown keys preserved
on read (forward compatibility). Missing keys filled from defaults.
Null values for known keys are treated as missing (default used instead).

**Unblocked:** F-config-ui
**Related:** F-bridge-integ-tests, F-player-identity

#### F-config-ui — Settings UI panel (main menu + pause menu)
**Status:** Done · **Phase:** 2

Settings panel accessible from both the main menu and the pause menu.
Displays and edits values from F-config-file's GameConfig autoload.

Initial UI:
- Player name text field
- "Start paused on load" checkbox
- Save / Cancel buttons

The panel is a generic settings container that other features can add
sections to. F-controls-config-C adds the keybinding section here.

Main menu gets a "Settings" button. Pause menu gets a "Settings" button
alongside the existing Save/Load/Resume/Quit buttons.

**Unblocked by:** F-config-file
**Unblocked:** F-controls-config-C
**Related:** B-start-paused-ui, F-controls-config

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

**Related:** F-binding-conflicts, F-config-ui, F-controls-config-A, F-controls-config-B, F-controls-config-C, F-edge-scroll, F-game-speed-fkeys, F-home-camera, F-keybind-help, F-mmb-pan, F-modifier-keybinds, F-mouse-elevation, F-selection-groups

#### F-controls-config-A — ControlsConfig autoload and handler migration
**Status:** Todo · **Phase:** 2

Create ControlsConfig autoload with all bindings defined as data.
Each binding has key, category, label, context, and optional alt_key,
physical flag, hidden flag. API: is_action(event, name) for event
callbacks, is_pressed(name) for polling (delegates to InputMap for
movement actions), get_label_suffix(name) for dynamic button labels.

Migrate every input handler to query ControlsConfig: action_toolbar,
orbital_camera, construction_controller, selection_controller,
placement_controller, escape_menu, main_menu, multiplayer_menu,
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
**Unblocked by:** F-config-ui
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

#### F-dappled-light — Dappled light effect via scrolling noise on ground shader
**Status:** Todo

Scrolling noise texture multiplied into the ground/bark shader brightness to simulate light filtering through the canopy. Sampled at world XZ coordinates with slow time-based offset. The bark/ground shaders already do world-space texture lookups, so adding a noise layer is minimal work. No CPU cost, no mesh changes — pure shader effect.

**Related:** F-day-night-color

#### F-day-night-color — Color grading shift by time of day
**Status:** Todo

Shift Environment tonemap / white balance / DirectionalLight3D color over a day-night cycle. Warm golden tones at dawn/dusk, cool blue at night, neutral midday. Zero rendering cost — just tweaking Environment resource properties over time. Requires a time-of-day system in the sim or at least in GDScript. Pairs well with F-dappled-light (noise intensity could vary by time) and F-distance-fog (fog color shifts with sky).

**Related:** F-dappled-light, F-distance-fog

#### F-dblclick-select — Double-click to select all of same military group
**Status:** Done · **Phase:** 5

Double-click a creature to select all visible creatures in the same
military group. Civilians (no military group) are treated as their
own implicit group. This lets players quickly grab "all archers" or
"all spearelves" based on groups they've defined.

**Related:** F-rts-selection, F-selection-groups

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

#### F-distance-fog — Depth-based atmospheric fog/haze
**Status:** Todo

Depth-based fog that fades distant geometry toward a sky/haze color. Can use Godot's built-in Environment fog or a simple shader-based depth fade. Hides LOD transitions (relevant for F-megachunk draw distance), gives depth cues, makes the forest feel large. Essentially free — per-fragment lerp based on depth.

**Related:** F-day-night-color, F-megachunk, F-mesh-lod

#### F-edge-outline — Edge highlighting shader (depth/normal discontinuity)
**Status:** Todo

Screen-space post-process or per-material shader that darkens edges at depth/normal discontinuities (Sobel filter on depth+normal buffer). Highlights silhouettes of branches, platforms, and structures against the sky. Makes the world readable at distance without extra geometry. One full-screen pass, minimal cost.

#### F-edge-scroll — Configurable edge scrolling (pan, rotate, or off)
**Status:** Todo · **Phase:** 5

Moving the mouse to screen edges moves the camera. Three configurable
modes:
- Pan: edges scroll the camera horizontally (classic RTS)
- Rotate: edges rotate/tilt the camera
- Off: disabled (default)

Edge scrolling auto-disables when the mouse is over a UI panel to
prevent accidental camera movement while using menus.

**Related:** F-controls-config

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

#### F-equipment-color — Equipment sprites use item resolved color
**Status:** Done

Equipment sprite overlays use the item's resolved color (from
F-item-color) to tint drawn equipment. A dyed-red breastplate renders
red; an undyed oak breastplate renders oak-brown. The per-equipment
drawing functions in `elven_canopy_sprites` accept a color parameter
sourced from `item_color(stack)`.

**Unblocked by:** F-equipment-sprites, F-item-color

#### F-equipment-sprites — Dynamic sprite customization for equipment
**Status:** Done

Creature sprites dynamically reflect equipped items — weapons, tools,
clothing, and armor are drawn as overlays on the base procedural sprite
in the `elven_canopy_sprites` crate.

Design decisions:
- Per-species anchor point table (hand, head, torso, legs, feet positions).
  Code scaffolding supports multiple species, but only elves get anchor
  data initially. Species without defined anchors show their base sprite
  unchanged even if the sim says they have equipment.
- Each equipment type (helmet, breastplate, greaves, gauntlets, boots,
  weapons) gets a drawing function that paints onto the sprite image at
  the species-specific anchor offsets.
- Role-based outfits (warrior armor, mage robes, etc.) are replaced by
  equipment visuals, not layered on top.
- Composited sprites are cached in Rust keyed on (creature identity,
  equipped item set). The gdext bridge exposes final composited textures
  to Godot.

**Unblocked by:** F-clothing, F-rust-sprites
**Unblocked:** F-equipment-color

#### F-face-tint — Directional face tinting by normal (top warm, bottom cool)
**Status:** In Progress

Tint voxel faces based on normal direction: top faces slightly warmer (sky light), side faces neutral, bottom faces cooler/darker (indirect light). A single dot(normal, up) in the fragment shader, essentially free. Gives strong sense of natural directional lighting. Could also be baked into vertex colors at mesh gen time for zero shader cost.

#### F-follow-multi — Camera zoom-to and follow for multi-selections
**Status:** Todo · **Phase:** 5

Camera zoom-to and follow mode for multi-unit selections. The camera
tracks the centroid of the selected group, updating each frame.
Zoom level stays at the player's current setting (no auto-zoom to
bounding box). WASD breaks follow as with single-unit follow.

**Related:** F-cam-follow, F-selection-groups

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

#### F-game-speed-fkeys — Move game speed controls to F1/F2/F3
**Status:** Done · **Phase:** 5

Move game speed controls from 1/2/3 to F1/F2/F3, freeing the number
keys for selection groups (F-selection-groups). Space remains
pause/resume.

**Unblocked:** F-selection-groups
**Related:** F-controls-config

#### F-ghost-above — Hide voxels above camera focus height
**Status:** Done · **Phase:** 2

A toggle button that hides all voxels above the camera's focus Y-level,
letting the player see the level they're looking at without upper
platforms, branches, and tree structure obscuring the view. Implemented
together with F-bldg-transparency on a shared branch since both features
share UI surface area (right-edge icon toolbar) and rendering concerns.

**UI:** Square icon button on a vertical toolbar strip along the right
screen edge. Procedurally drawn icon (GDScript `_draw()`): a tree with a
horizontal dotted line across the middle — top half solidly filled when
showing all voxels, top half as a dotted outline with nothing inside
when upper voxels are hidden. Hover tooltip, no text label, no keyboard
shortcut.

**Cutoff source:** `orbital_camera.gd` exposes `position.y` (the orbit
pivot) and `get_focus_voxel()` which floors to voxel coordinates. The
cutoff Y is derived from the floored camera focus Y.

**Rendering approach — mesh regen with Y cutoff (approach A):** Pass an
optional Y cutoff into `generate_chunk_mesh()` in `mesh_gen.rs`. Voxels
above the cutoff are treated as air during face culling, so the exposed
top faces at the cut boundary are generated naturally (the player sees
the top of wood/dirt, not hollow interiors). Only chunks that span the
old or new cutoff Y need rebuilding. Since the camera Y snaps to voxel
centers, the cutoff changes in discrete steps, not every frame, keeping
rebuild cost manageable.

**Why mesh regen, not shader discard:** The existing mesh generator
aggressively culls interior faces (opaque↔opaque neighbor = no face).
A shader-only approach (fragment discard above Y) would expose hollow
interiors where those culled faces are missing. Mesh regen with the
cutoff parameter correctly generates boundary faces.

**Rebuild pipeline:** The dirty-chunk system already exists
(`MeshCache.mark_dirty_voxels()` → `update_dirty()` →
`tree_renderer.gd._rebuild_chunk()`). When the cutoff Y changes, mark
chunks at the old and new Y levels as dirty and let the existing pipeline
handle the rest.

**State:** Ephemeral — resets each session, not persisted in save files.

**Related:** F-bldg-transparency, F-zlevel-vis

#### F-godot-setup — Godot 4 project setup
**Status:** Done · **Phase:** 0 · **Refs:** §3

Godot 4 project with GDExtension configuration.

#### F-home-camera — Home key to center camera on tree
**Status:** Done · **Phase:** 5

Press Home to snap the camera focal point to the center of the
player's tree. Keeps current zoom and pitch — only repositions the
focal point.

**Related:** F-controls-config

#### F-keybind-help — Keyboard shortcuts help overlay
**Status:** Done · **Phase:** 2

A help panel (toggled via toolbar button or ? key) showing all keyboard
shortcuts and mouse controls: camera orbit/zoom/pan, speed controls, ESC
chain, construction mode keys, etc. Pure GDScript UI — no sim changes.

**Related:** F-build-queue-ui, F-controls-config, F-controls-config-C

#### F-labor-panel — DF/Rimworld-style labor assignment UI
**Status:** Todo

A DF/Rimworld-style labor assignment panel where the player can toggle
task categories per creature. Grid of creatures × task types with
checkboxes. Controls which elves and tamed animals will pick up which
kinds of work. Replaces ad-hoc per-creature task restrictions.

**Related:** F-civ-pets, F-task-tags

#### F-leaf-sway — Foliage vertex sway shader (wind simulation)
**Status:** Todo

Vertex displacement in the leaf shader — offset leaf vertices by a sine wave keyed on world position and time. Zero CPU cost, no mesh regeneration. Amplitude modulated by height (higher = more sway) to simulate wind gradient. Makes the canopy feel alive.

#### F-leaf-tuning — Leaf visual fine-tuning and interior decisions
**Status:** Todo

Fine-tune leaf visual quality and decide on leaf interior rendering.

**Shader tuning:** The leaf noise shader (leaf_noise.gdshader) uses procedural value noise for alpha scissor and color/brightness variation. Current settings work but could be improved — sharper value transitions (more pixel-art-like), better color palette, per-tree-species color variation.

**Interior decision:** Leaf blobs currently render as shells only (leaf↔leaf faces culled). This may look too thin for large canopies. Options to revisit: partial culling (only deeply buried faces), depth-based alpha density, or keeping some interior layers for visual thickness.

**Related:** F-tiling-tex, F-visual-smooth

#### F-lod-sprites — LOD sprites (chibi / detailed)
**Status:** Todo · **Phase:** 8+ · **Refs:** §24

High-detail anime sprites at close zoom, low-detail chibi at far zoom.
Deferred until camera zoom range demands it.

#### F-main-menu — Main menu UI
**Status:** Done · **Refs:** §26

Main menu with New Game, Load, and Quit buttons.

#### F-mana-depleted-vfx — Visual feedback for mana-depleted work actions
**Status:** Done · **Refs:** §11

Visual feedback when a creature performs a wasted work action due to
insufficient mana. Anime-esque confusion icons (e.g., question marks, swirls)
floating above the creature's head. Provides player feedback for why
construction has stalled.

**Unblocked by:** F-mana-system

#### F-mesh-gen-rle — RLE-aware chunk mesh generation
**Status:** Done

Optimize chunk mesh generation to iterate column spans directly instead
of per-voxel get() calls. Column groups share the XZ footprint with mesh
chunks (16×16), so mesh gen can iterate spans and clip to each chunk's
Y range rather than querying 16×16×16 = 4096 individual voxels.

Also benefits heightmap generation (trivial: last non-Air span's top_y)
and any future bulk voxel queries.

Depends on the bulk iteration API (column_spans / chunk_columns) from
F-rle-voxels.

**Unblocked by:** F-rle-voxels

#### F-mesh-lod — Mesh level-of-detail for distant chunks
**Status:** Done

**Done:** Three-pass mesh decimation pipeline for chamfer-only mode: (1) coplanar region flood-fill with centroid-fan re-triangulation, (2) collinear boundary vertex collapse, (3) QEM edge-collapse with near-zero threshold. Achieves 84-99% triangle reduction on typical shapes while preserving watertightness and volume. Enabled by default. Togglable via debug panel.

**Not done (future work):** Distance-based LOD tiers — generating multiple mesh resolutions per chunk and swapping based on camera distance. This is the actual "level of detail" part. Would use higher QEM error thresholds for distant chunks, but introduces complications: cache management for multiple LOD meshes per chunk, smooth LOD transitions to avoid popping, interaction with the megachunk spatial hierarchy, and the QEM deformation bug (B-qem-deformation) becomes more visible at higher thresholds. The current near-zero-threshold pipeline is a prerequisite (reduces the full-detail mesh) but the multi-tier LOD system is a separate effort.

**Known issues:** QEM pass produces rare large deformations in unidentified geometric configurations (B-qem-deformation, chamfer-only). Chunk boundary vertices are pinned, leaving dense triangle bands at chunk seams (F-boundary-decim, chamfer-only).

**Unblocked by:** F-visual-smooth
**Related:** B-chamfer-nonmfld, B-qem-deformation, F-boundary-decim, F-distance-fog, F-megachunk, F-mesh-cache-lru, F-visual-smooth

#### F-mesh-par — Parallel off-main-thread chunk mesh generation with camera-priority
**Status:** Todo

Move chunk mesh generation off the main thread onto the rayon worker pool. Currently mesh gen blocks the main thread; parallelizing it across rayon workers would reduce frame hitches when many chunks need rebuilding (e.g., after construction, tree growth, or camera movement). Chunks visible to the camera should be prioritized (submitted last to exploit rayon's LIFO work-stealing, or split into a high-priority batch that completes before speculative work begins). Ideally also speculatively generate meshes for chunks near the camera or within its frustum, since the camera could pan there at any moment. Relates to F-megachunk (spatial hierarchy) and F-mesh-cache-lru (caching).

**Related:** F-megachunk, F-mesh-cache-lru, F-voxel-ao

#### F-minimap — Minimap with tree silhouette and creature positions
**Status:** Done · **Phase:** 2

Top-down (XZ) zoomable minimap in the bottom-right corner. Pure
rendering/UI — reads existing sim data, no new sim logic.

**Projection:** Top-down (XZ). Side-view (XY silhouette) is a separate
future item.

**Scope & zoom:** Zoomable with discrete steps (~5–6 levels), from
close-in to full world. Mouse wheel when cursor is over the minimap
captures scroll (does not zoom the camera). Tiny +/− icon buttons with
tooltips as a discoverable fallback.

**Follow mode:** Toggleable via a small icon button (tree icon = centered
on main tree, eye icon = following camera focal point). Tooltip on hover.
When tree-centered, a faint crosshair marks the camera position.

**Rendering:** Custom `_draw()` on a Control node. Terrain/tree texture
cached and regenerated only on voxel change. Creature overlay redrawn
every frame (cheap — just points/sprites on a small canvas).

**Creatures:** Representation scales with zoom level — sprites when
zoomed in, small dots at medium zoom, single pixels or hidden when
zoomed way out. Selected units rendered more prominently (brighter /
larger).

**Z-levels:** Voxels above the camera's focal height rendered at reduced
opacity (ghostly).

**Camera frustum:** Outlined on the minimap.

**Interaction:** Click to jump camera. Drag to pan the minimap view. No
command issuing (no right-click move orders) — may be added later.

**Appearance:** Thin styled border. Bottom-right corner. Fixed size,
square, ~15% of viewport height (resolution-dependent). No
collapse, resize, or hotkeys.

**Architecture:** The minimap should be an instance of general
map-rendering code, not a one-off widget, to enable future reuse (full
map screen, side-view panel, etc.).

**Related:** F-zlevel-vis

#### F-mmb-pan — Ctrl+MMB drag to pan camera horizontally
**Status:** Done · **Phase:** 5

Ctrl+middle-mouse-button drag to pan the camera horizontally.
Mirrors Ctrl+scroll wheel for vertical movement (F-mouse-elevation),
making Ctrl the consistent "alternate axis" modifier for mouse controls.
Plain MMB drag remains orbit/tilt as before.

**Related:** F-controls-config, F-mouse-elevation

#### F-modifier-keybinds — Modifier key combinations in bindings
**Status:** Todo · **Phase:** 2

Support modifier key combinations (Ctrl+X, Shift+Click, etc.) in
ControlsConfig bindings and the rebinding UI. Data model already
supports modifiers array from F-controls-config-A; this feature
adds the UI for capturing and displaying modifier combos.

Depends on F-controls-config-C (rebinding UI must exist first).

**Blocked by:** F-controls-config-C
**Related:** F-controls-config

#### F-mouse-elevation — Ctrl+mouse wheel to move camera elevation
**Status:** Todo · **Phase:** 5

**Related:** F-controls-config, F-mmb-pan

#### F-new-game-ui — New game screen with tree presets
**Status:** Done · **Refs:** §26

Seed input, tree parameter sliders, preset buttons for different tree
shapes.

#### F-orbital-cam — Orbital camera controller
**Status:** Done · **Phase:** 0 · **Refs:** §23

Orbit, zoom, pan. Smooth interpolation. Follow mode for creatures.

#### F-path-ui — Path management UI and notifications
**Status:** In Progress · **Phase:** 4 · **Refs:** §18

UI for path management. Path info displayed in creature info panel (current
path, tier, level, XP progress). Player-facing controls for assigning
combat paths and specializations. Notifications for tier transitions,
self-assignments, and Attunement warnings. Elf roster view showing path
distribution at a glance (who's flexible, who's committed, who's locked in).
Specialization picker when an elf reaches the branching threshold.

**Unblocked by:** F-path-core

#### F-pause-menu — In-game pause overlay
**Status:** Done · **Refs:** §26

ESC-triggered pause menu with Resume, Save, Load, and Quit options.

#### F-recipe-search — Recipe catalog search/filter
**Status:** Done · **Phase:** 4

Add a search/filter text box to the recipe catalog picker so players
can quickly find recipes by name without browsing the category tree.

Scope:
- GDScript UI: text input at the top of the recipe picker that filters
  the visible tree as the player types
- No sim changes needed — display_name is already available

Split out from F-recipe-hierarchy which handles the tree/hierarchy UI.

**Related:** F-recipe-hierarchy

#### F-roof-click-select — Roof click selects building, not elf underneath
**Status:** Done · **Phase:** 2

When the player clicks on a roof voxel of a building, select the building
(opening the structure panel) rather than selecting an elf who is inside
the building underneath the roof. The roof acts as a click shield for the
building interior. Elves on top of the roof or outside the building are
still directly selectable. Pairs with F-bldg-transparency which lets the
player hide roofs to click on elves inside.

**Related:** F-bldg-transparency, F-select-struct

#### F-rts-selection — RTS box selection and multi-creature commands
**Status:** Done

Godot-side UI work. Box selection (click-drag rectangle) in selection_controller.gd. Multi-creature selection state (client-local, not sim state). Group info panel (portraits/icons, count by species). Right-click context commands: ground → GoTo, hostile creature → AttackCreature. All commands dispatch SimAction variants for each selected creature. Selection state not saved, not synced in multiplayer.

**Done so far:** Stable creature ID addressing — replaced fragile (species, index) with CreatureId UUID strings throughout the full pipeline (SimBridge, selection_controller, creature_info_panel, main.gd, tooltip_controller, units_panel, task_panel). New SimBridge APIs: `get_creature_positions_with_ids()`, `get_creature_info_by_id()`, `is_hostile_by_id()`. Box selection with click-drag rectangle overlay (CanvasLayer ColorRect, screen-space projection). Multi-creature selection state (Array of creature IDs). Shift+click/drag for additive selection toggle. Group info panel: scrollable list of selected creatures with sprites, names, species, and activity; clicking a row selects just that creature; mutual exclusion with single-creature info panel. Box select filters to player-civ creatures (RTS convention). Dead creature pruning from selection (single and multi). Right-click context commands: attack hostile, move-to friendly/ground (ported from main's species/index API to UUID-based, works for multi-select). Selection highlight rings: faction-colored (blue player, yellow neutral, red hostile) flat rings at creature feet, show through terrain via no_depth_test, sprites render on top via render_priority. Two sizes for 1x1 and 2x2 footprint creatures.

**Not yet done:** Selection count indicator.

**Draft:** docs/drafts/combat_military.md (§2)

**Draft:** docs/drafts/combat_military.md (§2)

**Related:** F-alt-deselect, F-command-queue, F-dblclick-select, F-move-spread, F-selection-bar, F-tab-cycle

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

#### F-rust-sprites — Move sprite generation to new elven_canopy_sprites crate
**Status:** Done

Port procedural sprite generation from GDScript (sprite_factory.gd) into
a new `elven_canopy_sprites` crate. This is a pure Rust library with no
Godot dependency — it outputs raw RGBA8 pixel buffers (`Vec<u8>`). Depends
on `elven_canopy_sim` (species types, item types) and `elven_canopy_prng`
(deterministic hashing). Consumed by `elven_canopy_gdext` (thin wrapper
to convert pixel buffers into Godot Image/ImageTexture) and the
elfcyclopedia server (eliminating duplicated fruit drawing code).

Scope: all 10 creature species + fruit, ~1500 lines of GDScript drawing
code. Includes porting the drawing primitives (circle, ellipse, rect,
set_px, color helpers) and all per-species sprite functions. Color
palettes (hair colors, skin tones, etc.) live as constants in the crate.
sprite_factory.gd is deleted or reduced to a thin call-through once
complete.

**Unblocked:** F-creature-biology, F-equipment-sprites
**Related:** F-creature-biology, F-fruit-sprites

#### F-select-struct — Selectable structures with interaction UI
**Status:** Done · **Phase:** 3

Click-to-select completed structures (platforms, buildings, ladders, etc.)
with an info panel showing structure type, dimensions, health/stress, and
structure-specific actions. Extends the existing creature selection system
to handle structure entities. Foundation for per-structure interaction like
rope ladder furling, building furnishing, and structure demolition.

**Related:** F-demolish, F-elf-assign, F-roof-click-select, F-rope-retract, F-selection, F-struct-names, F-structure-reg

#### F-selection — Click-to-select creatures
**Status:** Done · **Refs:** §26

Ray-based selection with billboard sprite hit detection. ESC to deselect.
Input precedence chain with placement and pause systems.

**Related:** F-creature-tooltip, F-select-struct

#### F-selection-bar — Bottom-of-screen selection bar (SC2-style)
**Status:** Todo · **Phase:** 5

Replace the right-side selection panel with a persistent bottom-of-screen
selection bar (SC2-style). Shows portraits/icons for each selected unit
and structure icons, with info on hover and group commands. Unifies
creature and structure selection into a single consistent UI element.

Speculative — larger UI rearchitecture touching creature_info_panel.gd,
structure_info_panel.gd, selection_controller.gd, and overall layout.

**Related:** F-rts-selection

#### F-selection-groups — Ctrl+number selection groups with double-tap camera center
**Status:** Done · **Phase:** 5

SC2-style selection groups using number keys 1-9:
- Ctrl+number: save current selection as group N
- Shift+number: add current selection to group N
- Number: recall group N
- Double-tap number: recall group N and center camera on group centroid (no zoom change, no follow)

Groups can contain both creatures and structures. Stored in the sim
per-player (keyed by player username from F-player-identity) so they
persist across save/load and don't collide in multiplayer.

Configurable camera locations (SC2 camera hotkeys) may share the same
system or use a parallel binding set.

Requires F-game-speed-fkeys (done) to free the number keys.
Requires F-player-identity for per-player persistent storage.

**Unblocked by:** F-game-speed-fkeys, F-player-identity
**Related:** B-doubletap-groups, F-controls-config, F-dblclick-select, F-follow-multi, F-player-identity

#### F-shadow-cull — Shadow-only rendering for culled chunks in light direction
**Status:** Done

Chunks culled by frustum/height hiding currently become fully invisible, which removes them from Godot's shadow map pass. This causes canopy above the camera to stop casting shadows on the ground below. Fix: instead of setting `.visible = false` on culled chunks that are above or diagonal-in-light-direction from the camera frustum, set their `shadow_casting_setting` to `SHADOW_CASTING_SETTING_SHADOWS_ONLY`. They render into the shadow map (depth only, cheap) but skip the color pass. Chunks behind/below the camera that can't cast shadows into the visible area can still be fully hidden. Requires knowing the DirectionalLight3D direction to determine which culled chunks are shadow-relevant.

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

#### F-tab-cycle — Tab to cycle focus through units in selection
**Status:** Todo · **Phase:** 5

When multiple units are selected, Tab cycles focus through individual
creatures, showing their info panel without deselecting the rest.
Shift+Tab cycles in reverse.

**Related:** F-rts-selection

#### F-task-panel-groups — Task panel grouped by origin + creature names
**Status:** Done · **Phase:** 2

Group task panel cards into three sections by origin: Player Directives
(build, goto, furnish), Automated Management (future), and Autonomous
Decisions (eat, sleep). Show creature Vaelith names on assignee zoom
buttons instead of hex IDs. Adds `TaskOrigin` enum to `task.rs` with
`PlayerDirected`, `Autonomous`, and `Automated` variants.

**Modified files:** `task.rs`, `sim.rs`, `sim_bridge.rs`, `task_panel.gd`

#### F-task-panel-sprites — Creature sprites in tasks panel and activity cards
**Status:** Todo

Show creature portrait sprites next to assignee names in the tasks panel
and next to participant names in activity cards. Use the existing
SpriteGenerator.species_sprite(species, index) pattern from
units_panel.gd with a sprite cache keyed by creature_id.

#### F-tiling-tex — Prime-period tiling textures for bark and ground
**Status:** Done

Replace per-face atlas texture generation with a prime-period tiling
system. Three independent caches (A, B, C) each generate monochrome
face tiles from trig-based formulas that tile at different prime periods
per axis:

- Cache A: periods (11, 3, 7) for (x, y, z)
- Cache B: periods (7, 5, 11)
- Cache C: periods (5, 7, 5)

No two caches share the same period on the same axis, so alignment
artifacts never coincide. Y periods are smaller (less vertical variation
needed, especially for ground) to keep cache sizes down: A=231, B=385,
C=175 tiles total. Each cache also has irrational-ish per-axis phase
offsets (e.g., x+3.72) so sine zeroes don't align at the origin.

Tiles are generated lazily and cached by modular coordinates. The shader
combines the three monochrome textures and applies a color ramp (same
ramps as current voxel_color()). Face orientation maps UV axes to
different world axes, adding another dimension of variety.

Fully replaces the current FaceTileCache / FaceAtlas / per-chunk atlas
pipeline in texture_gen.rs and the PendingFace / build_atlas_and_fixup
code in mesh_gen.rs. Leaf surfaces are unaffected (they use a separate
alpha-scissor texture).

**Unblocked:** F-bigger-world
**Related:** F-leaf-tuning

#### F-tree-info — Tree stats/info panel
**Status:** Done · **Phase:** 2

Panel showing the player's tree statistics: total voxels, height, branch
count, leaf count, fruit production rate, mana level (once F-mana-system
exists), and carrying capacity. The player *is* the tree but currently has
no introspective view of their own state. Could be a toggleable overlay
or a persistent sidebar element.

**Related:** F-creature-info, F-mana-system

#### F-two-click-build — Two-click construction designation (click start, click end)
**Status:** Todo

Replace the current click-drag-release construction designation with a
two-click workflow: click to set the start corner, move mouse to preview,
click again to set the end corner. More precise and less prone to
accidental designations.

**Related:** F-construction

#### F-voxel-ao — Per-vertex ambient occlusion baked into chunk meshes
**Status:** Todo

Per-vertex ambient occlusion baked into chunk meshes at generation time. For each face vertex, sample the 3 corner-adjacent voxels (the classic 0-1-2-3 smooth voxel AO algorithm). Store AO factor per vertex — either in vertex color alpha or a dedicated attribute — and let the GPU interpolate smoothly across each face. Zero per-frame cost since AO is baked into the mesh and only recomputed when the chunk is dirtied. Cross-chunk border sampling uses the same neighbor-chunk reads already done for face culling. The shader multiplies AO into the final color to darken corners, crevices, undersides of branches, and interior spaces. High visual impact for minimal computational cost — transforms flat-shaded voxels into something with depth and presence.

**Interaction with F-visual-smooth:** The cubic 0-1-2-3 corner AO algorithm assumes axis-aligned geometry with vertices on grid corners. When smooth rendering lands, vertices sit at interpolated positions along voxel edges. The AO algorithm must adapt to a hybrid approach: for each smooth-mesh vertex, find the nearest voxel position(s), sample the 3×3×3 neighborhood in the voxel grid, and compute occlusion weighted by distance and the vertex normal (soft falloff instead of hard binary). This is still bounded and cacheable — same core idea, just with continuous rather than discrete sampling. Implement cubic AO first, then adapt when F-visual-smooth lands.

**Supplementary SSAO:** Godot's built-in screen-space AO (`Environment.ssao_enabled`) can supplement baked AO for dynamic objects (elves, creatures) that don't have per-vertex AO. Works regardless of cubic vs smooth geometry.

**Related:** F-mesh-par, F-visual-smooth

#### F-world-boundary — World boundary visualization
**Status:** Todo · **Phase:** 2

Visual indication of the voxel world's finite boundaries. The world grid has
fixed dimensions but nothing shows the player where the edges are. Could be
subtle ground grid lines, edge fog, fading terrain, or a visible border
when the camera approaches the edge. Prevents confusion when placing
construction near world limits.

#### F-world-map — World map view
**Status:** Todo

A world map UI showing the broader region — other trees, civilizations,
terrain. Used for strategic decisions like where to send expeditions,
tracking incoming raids, and understanding the geopolitical landscape.

**Related:** F-bigger-world, F-forest-radar, F-military-campaign, F-zone-world

#### F-zlevel-vis — Z-level visibility (cutaway/toggle)
**Status:** Done · **Refs:** §27

How to show lower platforms when upper ones occlude them. Transparency,
cutaway, or hide-upper-levels toggle. Open design question (§27).

**Related:** F-bldg-transparency, F-ghost-above, F-minimap

### Sim Engine

#### B-assembly-timeout — Activity assembly timeout not enforced
**Status:** Todo

ActivityConfig::assembly_timeout_ticks is defined (default 300,000) but
never checked. An activity stuck in Assembling phase (e.g., participants
died or got lost en route) will wait indefinitely. Should check elapsed
time in the Assembling branch of the activation loop and cancel the
activity if the timeout has been exceeded, similar to how
check_activity_pause_timeout handles the Paused phase.

**Related:** F-group-activity

#### B-chamfer-nonmfld — Chamfer produces non-manifold edges for diagonally-adjacent voxels
**Status:** Todo

The chamfer pass in `smooth_mesh.rs` produces non-manifold edges (edges shared by 3+ triangles) when two voxels are diagonally adjacent — sharing only an edge, not a face. This is a distinct issue from the QEM deformation bug (B-qem-deformation).

**How it happens:** When two voxels share only an edge (e.g., voxels at (x,y,z) and (x+1,y,z+1) with neither (x+1,y,z) nor (x,y,z+1) present), each voxel generates exposed faces on its sides. The chamfer subdivides these faces and deduplicates vertices by position. Along the shared edge, both voxels' face vertices merge, creating edges shared by triangles from two different faces on two different planes — a non-manifold configuration.

**Where it occurs in terrain:** Any heightmap where adjacent columns create a "checkerboard" height pattern triggers this. For example, a 2×2 area where h(0,0)=2, h(1,0)=1, h(0,1)=1, h(1,1)=2: the y=1 voxels at (0,0) and (1,1) are diagonally adjacent with no face-adjacent neighbor at the same height. This is common in natural-looking terrain with scattered single-step height variations.

**Evidence from testing:** In B-qem-deformation fuzz testing, 49/50 random heightmap seeds (height range 1–4) and 99/100 smooth heightmap seeds (max ±1 step) produced non-manifold chamfer output before a diagonal-gap-filling workaround was added to the test heightmap generator.

**Impact:** Non-manifold edges cause rendering artifacts (z-fighting, self-intersection, overlapping geometry) that could look like "bumps, dents, or misshapen surfaces." The QEM decimation pass has a non-manifold guard (`collapse_would_create_non_manifold`) that prevents creating NEW non-manifold edges, but cannot fix ones inherited from the chamfer input.

**Possible fix approaches:**
1. **Gap-filling in terrain gen:** Ensure no diagonal-only voxel adjacency exists by filling gaps (raise one neighbor column to eliminate the checkerboard pattern). The B-qem-deformation test suite uses this approach in `smooth_random_heightmap`.
2. **Chamfer-level fix:** Detect diagonal-only adjacency during face generation and either skip the conflicting faces or merge them into a single surface.
3. **Post-chamfer cleanup:** Add a non-manifold edge resolution pass before decimation.

**Related:** B-qem-deformation, F-mesh-lod

#### B-dead-enums — Remove dead GrownStairs/Bridge code and add explicit enum discriminants
**Status:** Done

GrownStairs, Bridge (VoxelType), and Stairs, Bridge (BuildType) exist in code but there is no way to produce them in-game. They are not planned for implementation in the foreseeable future. Remove all code referencing these variants — enum definitions, match arms, trait impls, tests, and any other references.

Additionally, add explicit integer discriminants to all serializable enums (at minimum VoxelType and BuildType) so that inserting or removing variants in the future does not silently change the serialized representation of existing variants.

#### B-sim-floats — Remaining f32/f64 in sim logic threaten determinism
**Status:** Done

Several sim-logic code paths still use f32/f64 arithmetic, which is not
guaranteed deterministic across platforms/compilers. Key areas:

- **Tree struct** (sim/mod.rs): health, mana_stored, mana_capacity,
  fruit_production_rate, carrying_capacity, current_load — all f32.
  Mana overflow calculation uses f64.
- **Task progress/total_cost** (task.rs, db.rs): f32 accumulated with
  float arithmetic throughout construction, crafting, furnishing, etc.
- **Mana generation** (config.rs): mana_base_generation_rate: f32,
  mana_mood_multiplier_range: (f32, f32), starting_mana/capacity: f32.
- **Pathfinding** (pathfinding.rs, nav.rs): f32 distances, g_scores,
  heuristic. Derived from integer coords via sqrt.
- **Combat/movement** (combat.rs, movement.rs): delay = (distance *
  speed as f32).ceil().
- **Greenhouse** (greenhouse.rs): next_f32() for fruit spawn chance.
- **Needs/activation**: mope duration cast to f32.

Worldgen floats (tree_gen.rs, structural.rs, texture_gen.rs) are run
once at init with a seeded PRNG — lower risk but still non-portable.
Rendering-only floats (mesh_gen, interpolated_position, raycast) are
safe since they don't affect sim truth.

#### B-task-civ-filter — Tasks lack civilization-level eligibility filtering
**Status:** Done

Tasks have required_species but no civ_id filter. Any creature of the right species can claim any available task regardless of civilization membership. A hostile goblin could theoretically claim a player-civ construction task. Tasks need a civ_id field and find_available_task needs to check it.

**Related:** F-group-activity

#### F-activation-revamp — Replace manual event scheduling with automatic reactivation
**Status:** Todo · **Phase:** 5

Revamp the creature activation/event system so that creatures do not need to manually schedule their next activation event. The current pattern — where every code path in execute_task_behavior, execute_attack_move, execute_attack_target_at_location, etc. must explicitly call event_queue.schedule() or risk leaving the creature permanently inert — is a persistent source of bugs. Any new return path that forgets to schedule a reactivation silently kills the creature's AI. Design a system where creatures are automatically reactivated unless explicitly suspended (e.g., waiting on an action timer).

Additional issues to address:
- **Shoot resolution bypasses autonomous combat check (B-ranged-shoot-walk):** After a Shoot action resolves at activation.rs:308-319, if a heartbeat-assigned task (EatBread/Sleep) was sneaked onto the creature during the cooldown, execute_task_behavior is called for that task and the function returns — skipping the flee check and autonomous combat check entirely. This causes autonomous archers to alternate between shooting and performing survival tasks instead of continuing to fight. The root cause is that the Shoot/MeleeStrike resolution path enters execute_task_behavior for any task the creature has, while the Move resolution path correctly falls through to the flee and autonomous combat checks.
- **Missing ranged cooldown wait in try_combat_against_target:** Melee on cooldown returns true and schedules reactivation (combat.rs:713-731), preventing the caller from walking. Ranged has no such handling — any try_shoot_arrow failure returns false, causing the caller to immediately walk toward the target. This asymmetry can cause ranged combatants to take unnecessary walk steps between shots.

**Related:** F-event-loop, F-task-assign-opt

#### F-adventure-mode — Control individual elf (RPG-like)
**Status:** Todo · **Phase:** 8+ · **Refs:** §26

Control a single elf in first/third-person perspective within the
same simulation. RPG-like exploration mode.

#### F-async-sim — Async sim: decouple sim thread from render thread via delta channel
**Status:** Todo

Move sim stepping off the main thread to decouple sim tick time from frame budget. Currently the bridge steps the sim synchronously inside the Godot _process() callback — sim cost directly reduces FPS.

**Approach:** Sim runs on its own thread (or rayon pool), producing lightweight render-relevant deltas each tick rather than full state duplication:
- **Voxel diffs:** List of (coord, old_type, new_type). Usually empty; small on construction/growth; full snapshot only on game load. Replaces the current dirty-chunk query — the render side knows exactly which chunks to regenerate.
- **Creature snapshots:** Position, action, HP, equipment per creature. Pre-built vec emitted by sim each tick, replacing on-demand bridge queries like get_all_creatures_summary().
- **Events:** Notifications, projectile spawns, deaths — already event-driven via SimEvent.

Sim posts deltas to a channel; render thread picks up the latest batch each frame. If sim is faster than rendering, intermediate states are skipped (fine — just want latest). If sim is slower, same state renders again (nothing visually changed).

**Commands:** UI actions (build, move, attack) go from render thread to sim thread via a second channel. Small and infrequent.

**Incremental migration path:**
1. Add sim tick timing to the bridge/status bar to measure current cost (cheap diagnostic).
2. Move sim step to a background thread, post creature snapshots, keep voxel reads synchronous behind a read lock (almost never contended since voxels rarely change).
3. Migrate voxel diffs to the channel, eliminating the last synchronous sim queries.

**Not urgent** — current sim is fast. This becomes important as creature count, pathfinding complexity, and task processing grow.

#### F-core-types — VoxelCoord, IDs, SimCommand, GameConfig
**Status:** Done · **Phase:** 0 · **Refs:** §5, §7

Core data types with deterministic UUID generation from PRNG.

#### F-crate-structure — Two-crate sim/gdext structure
**Status:** Done · **Phase:** 0 · **Refs:** §3, §4

Sim crate has zero Godot dependencies. Compiler-enforced separation
enables headless testing, fast-forward, and replay verification.

#### F-event-loop — Event-driven tick loop (priority queue)
**Status:** Done · **Phase:** 1 · **Refs:** §6

Discrete event simulation with priority queue. Empty ticks are free.
1000 ticks per simulated second.

**Related:** F-activation-revamp, F-sim-speed

#### F-game-session — Game session autoload singleton
**Status:** Done · **Refs:** §26

Godot autoload persisting seed and tree config across scene transitions.

#### F-gdext-bridge — gdext compilation and Rust bridge
**Status:** Done · **Phase:** 0 · **Refs:** §3

GDExtension bridge crate exposing sim to Godot. SimBridge node with
methods for commands, queries, and rendering data.

#### F-group-activity — Multi-worker activity coordination layer
**Status:** Done

Coordination layer above the task system for activities that require
multiple participants. Activities own tasks (GoTo for assembly) rather
than replacing them. Lifecycle: Recruiting → Assembling → Executing →
Complete. Execution doesn't start until required participants have
arrived at their positions. New tables: Activity, ActivityParticipant,
plus kind-specific extension tables.

**Draft:** docs/drafts/group_activities.md

**Unblocked:** F-choir-build, F-group-chat, F-group-dance
**Related:** B-assembly-timeout, B-task-civ-filter, F-choir-harmony, F-combat-singing

#### F-immediate-commands — Immediate command application (zero-tick updates)
**Status:** Done · **Phase:** 2

Currently, all SimCommands are buffered in `GameSession.pending_commands` and
only applied when an `AdvanceTo` message flushes them through `SimState.step()`.
This means that while paused, UI actions (changing recipes, toggling crafting,
renaming structures, issuing move commands, etc.) have no visible effect until
the player unpauses. This is awkward and unintuitive.

**Goal:** Make all commands apply immediately when they arrive, regardless of
pause state. The game session should apply commands to the sim as soon as they
are received via `SessionMessage::SimCommand`, not buffer them for the next
tick advance. `AdvanceTo` continues to drive scheduled event processing and
tick advancement, but with an empty command list (commands already applied).

**Key architectural insight:** The `GameSession` layer sits between the relay
and the simulator. Commands queue through the relay to the session, which
updates the sim (and any internal version numbering) without changing the
sim's tick number. The UI sees changes at next redraw, even while paused.

**What needs to change (session layer, not sim):**

- `GameSession.process(SimCommand)` should call `sim.apply_command()` directly
  instead of pushing to `pending_commands`. Assign current tick to the command.
- Events produced by `apply_command()` must be returned from `process()` — this
  already returns `Vec<SessionEvent>`, so the plumbing exists.
- `step()` called from `AdvanceTo` receives an empty command slice; it still
  processes scheduled events and advances ticks normally.
- `pending_commands` buffer may be removable entirely (or retained only for
  multiplayer's relay-ordered path).

**Multiplayer consideration:** In multiplayer, commands arrive with the relay's
canonical tick via `Turn` messages. The relay-ordered buffering model must be
preserved there — immediate application is for the single-player `apply_or_send`
path. The SP/MP branch already exists in `SimBridge.apply_or_send()`.

**Scope:** This is a session/bridge-layer change. `SimState.apply_command()` is
already a standalone pure mutation method with no dependency on `step()` or tick
state. The sim crate itself should need no changes.

**Related:** F-multiplayer, F-multiplayer (relay ordering)., F-session-sm, F-session-sm (session architecture)

#### F-megachunk — MegaChunk spatial hierarchy with draw distance and frustum culling
**Status:** Todo

Spatial hierarchy for efficient chunk rendering at large world sizes.

**MegaChunk:** A 16×16 horizontal group of chunk columns. Stores only
chunks that have renderable geometry (not pure air, not pure solid
underground). Provides coarse AABB for fast frustum culling — test one
MegaChunk AABB instead of hundreds of individual chunk AABBs.

**Draw distance:** Configurable radius (in MegaChunks or voxels) around
the camera. MegaChunks outside the radius have their chunk meshes
hidden. Chunk meshes are cached in memory with an LRU eviction policy
under a configurable memory budget — panning back to a recently visible
area is instant, but distant meshes are freed under memory pressure.

**Frustum culling hierarchy:** Camera frustum tested against MegaChunk
AABBs first (coarse), then individual chunk AABBs within visible
MegaChunks (fine). At 1024×1024 that's ~4K MegaChunk columns — trivial
to frustum-test each frame.

**LOD (deferred):** After F-visual-smooth lands, add automatic geometry
decimation for distant chunks — blurry blobby hills instead of per-voxel
detail. Not needed for initial implementation; draw distance alone
provides the performance knob.

**Chunk mesh lifecycle:** Meshes are created on demand when a chunk
enters draw distance, cached when it leaves, and evicted LRU when the
memory budget is exceeded. Both draw distance and memory budget are
user-configurable settings.

**Unblocked:** F-bigger-world
**Related:** F-distance-fog, F-mesh-lod, F-mesh-par, F-visual-smooth

#### F-mesh-cache-lru — LRU cache for chunk meshes at different Y cutoffs
**Status:** Todo · **Phase:** 2

**Related:** F-mesh-lod, F-mesh-par

#### F-modding — Scripting layer for modding support
**Status:** Todo · **Refs:** §27

Plugin/scripting system for custom structures, elf behaviors, invader
types. Open design question (§27).

#### F-nav-perf — Optimize nav graph generation performance
**Status:** Done

Optimize nav graph generation performance on 1024x255x1024 worlds.

**Done so far (branch `feature/F-nav-perf`):**
- SmallVec<[NavEdgeId; 8]> for NavNode.edge_indices (eliminates ~1M heap allocs, 99.9% of nodes fit inline)
- Flat column_index replacing LookupMap spatial index (O(1) column lookup, no hashing)
- Expanding-box find_nearest_node (O(1) typical vs O(N) linear scan)
- NavEdgeId used throughout function signatures (type safety)
- Removed current_node from Creature (correctness fix — NavNodeIds are ephemeral)
- Removed Serialize/Deserialize from nav types
- Parallel seed scan + validation with rayon
- Parallel layered BFS for node discovery (sort+dedup between layers for determinism)
- Per-column seed dedup via SmallVec (eliminated 200ms LookupMap dedup)
- Pre-allocated nodes/edges vecs in with_world_size()
- Edge discovery moved into parallel BFS (both regular and large nav graphs): BFS chunks emit canonicalized edge pairs for existing neighbors, par_sort_unstable+dedup, parallel validation (face-blocking/edge type/distance), sequential insertion sorted by (from_slot, direction_index) to preserve deterministic edge_indices ordering

**Profiling results (1024x255x1024, ~1M nodes, ~8M edges):**
- Seed scan+dedup+validate+insert: ~130ms (parallelized, fast)
- BFS node discovery + edge pair collection: ~500ms (parallelized)
- Edge pair dedup (par_sort_unstable): ~800ms
- Edge validation (parallel): ~70ms
- Edge insertion (sequential): ~400ms
- Total regular: ~1.25s (down from ~1.9s before edge-in-BFS, ~6s original)
- Large nav graph: similar improvement pattern

**What didn't help:**
- SmallVec 16 (profiling showed 99.9% of nodes have exactly 8 edges)
- Accumulating edges during BFS layers then bulk-inserting (same speed, broke edge_indices ordering which caused 9 test failures in combat/archery)
- HashSet for edge pair dedup (2.4s vs 0.8s for par_sort_unstable — hashing overhead exceeds sort)

**Remaining potential optimization:**
- **Parallel partitioned edge insertion (potentially unsafe).** Pre-compute all edges as (from_slot, to_slot, EdgeType, distance) tuples. Pre-size the edges vec and write edge data into disjoint slices (one per rayon chunk). For updating NavNode.edge_indices, sort computed edges by from_slot, partition into contiguous ranges, and have each thread update a disjoint set of NavNodes. This avoids data races but requires careful use of split_at_mut or unsafe indexing. False sharing at chunk boundaries is negligible with large chunks. This is complex and needs careful correctness validation — the edge_indices ordering affects PRNG-dependent sim behavior (wander direction, flee direction, archer positioning).

#### F-parallel-dedup — Radix-partitioned parallel dedup (elven_canopy_utils)
**Status:** In Progress

New `elven_canopy_utils` crate with radix-partitioned parallel dedup algorithm. Scatters items by hash into power-of-2 buckets (bitmask assignment), deduplicates each bucket independently via hashbrown HashTable with precomputed hashes, collects results. Configurable bucket count and generic hasher (`parallel_dedup_with`). Sequential fallback below 10k items. Criterion benchmarks comparing bucket counts (32/64/128/256) × hashers (std/ahash/fxhash) × item types (u64/[u64;6]/String) × sizes (1k–10M). Currently slower than `par_sort_unstable + dedup` — needs tuning. **Draft:** `docs/drafts/parallel_dedup.md`

#### F-rle-voxels — RLE column-based voxel storage
**Status:** Done

Replace the flat `Vec<VoxelType>` voxel storage with a compressed
column-based representation. Each column stores a sorted list of
`(VoxelType, top_y)` spans — each span's voxel type extends upward until
the next span's start. The topmost Air span is implicit (not stored).
World height capped to ≤256 so heights fit in a byte.

Columns are grouped in 16×16 groups (matching chunk alignment). Each group
holds inline arrays of span offsets (`[u16; 256]`) and span counts
(`[u8; 256]`), plus a single heap allocation for all span data. Columns
are allocated with slack (multiples of 4 spans / 8 bytes) so most
single-voxel edits rewrite in place without repacking the group.

**Goals:**
- 10–30× memory reduction for large, mostly-empty worlds
- Enable 1024×128×1024+ playable areas without GB-scale RAM
- `get_voxel` via binary/linear search on spans (fast for typical 2–5 span columns)
- `set_voxel` splits/merges spans in place; repacks group only on overflow
- Bulk iteration (mesh gen, nav build) can traverse spans directly

**API:** `get_voxel` / `set_voxel` interface unchanged externally. Internals
of VoxelWorld completely replaced.

**Draft:** `docs/drafts/rle_voxels.md`

**Unblocked:** F-bigger-world, F-mesh-gen-rle, F-nav-gen-opt

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

**Related:** F-immediate-commands, F-multiplayer

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

#### F-split-sim — Split monolithic sim.rs into domain sub-modules
**Status:** Done

#### F-tree-db — Trees as DB entities with elf-tree bonding
**Status:** Done

Trees as first-class entities in SimDb with columns for position, species,
mana_stored, mana_capacity, etc. Each elf has a bonded_tree_id foreign key.
Prerequisite for F-multi-tree: the current implicit "the one tree" must become
an explicit DB row before multiple trees can coexist. Also enables per-tree
mana pools, per-tree elf rosters, and tree-specific stats/upgrades.

**Unblocked:** F-multi-tree
**Related:** F-multi-tree, F-uplift-tree, F-zone-world

#### F-tree-gen — Procedural tree generation (trunk+branches)
**Status:** Done · **Phase:** 1 · **Refs:** §8

Trunk is first branch — all segments use same growth algorithm with
different params. Cross-section bridging ensures 6-connectivity. Voxel
type priority prevents overwrites.

### Tabulosity

#### B-tab-serde-tests — Fix tabulosity test compilation under feature unification
**Status:** Done

#### F-child-table-pks — Convert child tables to natural compound primary keys
**Status:** Done

Convert child tables in SimDb from synthetic auto-increment primary keys to natural compound primary keys. The auto-increment IDs on these tables are never referenced outside of `types.rs` and `db.rs` — they exist only to satisfy the "every table needs a PK" requirement. Replacing them with natural keys eliminates dead ID types, makes the schema self-documenting, and in the case of 1:1 extension tables, allows direct `.get(&parent_id)` lookups instead of `by_parent_id(...).into_iter().next()`.

**Tables to convert — natural compound PKs (no auto-increment needed):**

- **CreatureTrait** → `(creature_id, trait_kind)`. Already has a unique compound index enforcing this invariant. `CreatureTraitId` is never used outside `types.rs`/`db.rs`. The existing `modify_unchecked(&row.id, ...)` calls in `sim/tests.rs` become `modify_unchecked(&(creature_id, trait_kind), ...)`, which is arguably clearer.

- **CivRelationship** → `(from_civ, to_civ)`. Relationship table with two FK columns that together form a natural unique key. `CivRelationshipId` is never used outside `types.rs`/`db.rs`.

- **TaskHaulData** → `(task_id)`. 1:1 extension table. Accessor `task_haul_data()` becomes `.get(&task_id)` instead of `by_task_id(...).next()`. One `modify_unchecked(&data.id, ...)` call in `logistics.rs` becomes `modify_unchecked(&data.task_id, ...)`.

- **TaskSleepData** → `(task_id)`. 1:1 extension table. Never modified after insertion. Same `.get()` simplification.

- **TaskAcquireData** → `(task_id)`. 1:1 extension table. Never modified after insertion. Same pattern.

- **TaskCraftData** → `(task_id)`. 1:1 extension table. One `modify_unchecked` call in `crafting.rs`. Also has `active_recipe_id` field (not a FK-as-PK, just a regular field).

- **TaskAttackTargetData** → `(task_id)`. 1:1 extension table. Never modified after insertion.

- **TaskAttackMoveData** → `(task_id)`. 1:1 extension table. Never modified after insertion.

**Tables to convert — compound PK with auto-increment tiebreaker (requires F-tab-nonpk-autoinc):**

- **Thought** → `(creature_id, seq)` with `seq` as `#[auto_increment]`. A creature can have multiple thoughts of the same kind on the same tick, so no combination of existing fields forms a unique key. `ThoughtId` is never referenced outside `types.rs`/`db.rs`.

- **TaskStructureRef** → `(task_id, seq)` with `seq` as `#[auto_increment]`. No unique index on `(task_id, role)` in the schema — the one-ref-per-role invariant is application-layer convention only. `TaskStructureRefId` is never referenced outside `types.rs`/`db.rs`.

- **TaskVoxelRef** → `(task_id, seq)` with `seq` as `#[auto_increment]`. Same situation as TaskStructureRef — `role` is indexed independently but not in a unique compound index with `task_id`. `TaskVoxelRefId` is never referenced outside `types.rs`/`db.rs`.

- **TaskBlueprintRef** → `(task_id, seq)` with `seq` as `#[auto_increment]`. Currently 1:1 per task, but we anticipate multiple blueprints per task in the future. `TaskBlueprintRefId` is never referenced outside `types.rs`/`db.rs`.

- **LogisticsWantRow** → `(inventory_id, seq)` with `seq` as `#[auto_increment]`. A single inventory can have multiple wants for the same item kind with different material filters, so no combination of existing fields is guaranteed unique. `LogisticsWantId` is never referenced outside `types.rs`/`db.rs`.

- **ItemSubcomponent** → `(item_stack_id, seq)` with `seq` as `#[auto_increment]`. An item stack can have multiple subcomponents with the same `(component_kind, material)` at different qualities, so those fields don't form a unique key. `ItemSubcomponentId` is never referenced outside `types.rs`/`db.rs`.

- **EnchantmentEffect** → `(enchantment_id, seq)` with `seq` as `#[auto_increment]`. Design allows multiple identical effects on the same enchantment. `EnchantmentEffectId` is never referenced outside `types.rs`/`db.rs`.

**Save compatibility:** The `seq` field on converted tables will use `#[serde(rename = "id")]` so that old saves (which serialized the field as `"id"`) deserialize correctly into the renamed field. The auto-increment counter initialization (from F-tab-nonpk-autoinc) computes `max(field) + 1` when no counter is present in the save data, ensuring the sequence picks up where the old auto-PK left off and doesn't collide with existing row values.

**Tables NOT converted (and why):**

- **Notification** — `NotificationId` is never referenced, but there's no natural key. `(tick, message)` is plausible but not structurally guaranteed unique (two events at the same tick could produce identical messages). No benefit to a compound PK here.

- **ActiveRecipeTarget** — `ActiveRecipeTargetId` is used in `SimCommand::SetRecipeOutputTarget` and threaded through the GDScript bridge. Converting would require the UI to pass a multi-field key instead of a single ID. Not worth the disruption.

- **Furniture** — No guaranteed uniqueness on `(structure_id, coord)` — the same voxel could hold multiple furniture items.

- **ItemStack** — `ItemStackId` is heavily referenced across 7+ files (combat, inventory, activation, needs, etc.). Fundamental entity, not a child table.

- **Inventory** — Same as ItemStack; heavily referenced, not a child table.

**Unblocked by:** F-tab-nonpk-autoinc
**Related:** F-compound-pk, F-tab-nonpk-autoinc, F-tab-parent-pk

#### F-compound-pk — Compound (multi-column) primary keys
**Status:** Done

Support compound (multi-column) primary keys in Tabulosity tables, analogous to existing compound secondary indexes. Currently all tables require a single-field `#[primary_key]`. Compound PKs would allow tables like `creature_traits(creature_id, trait_kind) → value` to use the natural key as the PK without a synthetic auto-increment ID.

**Related:** F-child-table-pks, F-tab-nonpk-autoinc

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

#### F-tab-hash-idx — Hash-based indexes in Tabulosity derive macro
**Status:** Done

Integrate `InsOrdHashMap` into Tabulosity's derive macro so users can
declare hash-based indexes on table fields and opt into hash-based primary
key storage. Currently all indexes are `BTreeSet`-backed with O(log n)
lookup; hash indexes give O(1) exact lookup with deterministic
insertion-order iteration.

**Draft:** `docs/drafts/F-tab-hash-idx.md`

**Key design decisions:**
- `#[indexed(hash)]` / `#[index(..., kind = "hash")]` for hash secondary indexes
- `#[table(primary_storage = "hash")]` for `InsOrdHashMap<PK, Row>` primary storage
- `OneOrMany<PK, Inner>` enum for non-unique hash indexes — inline PK for
  single-entry groups (common case), BTreeSet or InsOrdHashMap for multi-entry
- Compound hash indexes supported; partial matches degrade to O(n) scan
- Mixed BTree+hash indexes on same field supported (must have different names)
- Hash indexes serialized directly (not skip+rebuild) to preserve insertion order
- Existing `rebuild_indexes()` renamed to `post_deser_rebuild_indexes()` (skips
  hash indexes); new `manual_rebuild_all_indexes()` rebuilds everything
- QueryOpts ordering ignored for hash indexes (matches real DB behavior)
- Range queries on hash indexes panic at runtime with clear message
- `modify_unchecked_range` not generated for hash primary (compile error)

**Unblocked by:** F-tab-ordered-idx
**Related:** F-tab-indexmap-fork, F-tab-ordered-idx

#### F-tab-indexmap-fork — Forked IndexMap with tombstone compaction (alternative to F-tab-ordered-idx)
**Status:** Todo

Higher-effort alternative to F-tab-ordered-idx: fork the `indexmap` crate and add
tombstone-based removal with periodic compaction, instead of IndexMap's current
O(n) shift-remove or order-disrupting swap-remove. This would avoid key
duplication (IndexMap stores entries in a single vec, with the hash table pointing
into it) and give better cache locality on iteration (no hash lookups needed).

Probably not worth the effort given that F-tab-ordered-idx is simpler and the key
duplication cost is minor for typical table sizes. But if profiling ever shows
iteration performance or memory overhead from key duplication is a bottleneck,
this is the path. The encapsulated interface from F-tab-ordered-idx means this
can be dropped in without touching Tabulosity internals.

**Related:** F-tab-hash-idx, F-tab-ordered-idx

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

#### F-tab-nonpk-autoinc — Non-PK auto-increment fields in tabulosity
**Status:** Done

Add support for a single `#[auto_increment]` field per table that is NOT the primary key. Currently, auto-incrementing is only available on single-column primary keys (`#[primary_key(auto_increment)]`). This feature decouples auto-incrementing from primary-key status, allowing a table to have a compound PK while still getting an automatically assigned unique value for one of its fields.

**Motivation:** Many child tables have a natural compound primary key (e.g., `(parent_id, seq)`) but need a globally unique tiebreaker column because the parent-scoped fields alone don't guarantee uniqueness. For example, a creature can have multiple thoughts of the same kind on the same tick, so `(creature_id, kind, tick)` isn't a valid PK — but `(creature_id, seq)` is, if `seq` auto-increments.

**Design decisions:**
- **One auto-increment field per table.** This matches every major relational database: MySQL explicitly forbids multiple auto-increment columns, PostgreSQL technically allows multiple `SERIAL` columns but only because they're syntactic sugar for independent sequences — no database actually supports or encourages multiple auto-increment fields per table. One is always sufficient.
- **Global sequence, not per-parent.** The counter is table-wide (a single `next_<field>` value), not scoped to each parent ID. Global uniqueness is simpler to implement and what databases universally do. Per-parent numbering (seq restarts at 0 for each parent value) can be done at the application layer if desired.
- **Field-level attribute.** The existing `#[primary_key(auto_increment)]` stays as-is for backward compatibility. The new `#[auto_increment]` attribute goes on a regular (non-PK) field. The table codegen produces an `insert_auto`-style method that fills in the auto-increment field and returns it.
- **Serde.** The table serializes/deserializes `next_<field>` alongside the rows array, just as auto-PK tables already serialize `next_id`. On deserialization, if the counter is missing from the data (e.g., loading an old save where this field used to be the PK and `next_id` was stored instead), the table must compute `max(field) + 1` across all loaded rows to initialize the counter. This prevents the counter from restarting at a low value and colliding with existing row values.
- **Interaction with compound PKs.** A compound PK can include the auto-increment field as one of its columns. The table's insert method takes all non-auto-increment PK fields from the caller, fills in the auto-increment field, and constructs the full compound key.

**Scope:** Tabulosity crate only (`tabulosity` + `tabulosity_derive`). No sim changes.

**Unblocked:** F-child-table-pks
**Related:** F-child-table-pks, F-compound-pk, F-tab-parent-pk

#### F-tab-ordered-idx — Deterministic-iteration hash index with tombstone skip
**Status:** Done

Deterministic-iteration hash index for Tabulosity. Wraps a `HashMap<K, usize>`
+ `Vec<Entry<K, V>>` where entries are either live `(K, V)` pairs or tombstones.
Iteration walks the vec in insertion order, skipping tombstones via O(1) span
jumps (important for the common "get me 1 thing, don't care which" case).
Compaction (rebuild vec, drop all tombstones, update HashMap usizes) triggers
when `vec.len() - map.len()` exceeds a threshold ratio. Compaction preserves
original insertion order for determinism stability regardless of compaction
policy changes.

**Entry representation:**
`enum Entry<K, V> { Live(K, V), Tombstone { span_start: usize, after_span: usize } }`
Relies on `(K, V)` being at least as large as two usizes in practice (true for
most DB-style keys+values). If `(K, V)` is smaller, the enum is still correct,
just slightly larger.

**Tombstone skip structure:** Tombstones store `(span_start, after_span)` —
absolute vec indices forming a linked skip structure within contiguous tombstone
spans. Interior tombstones exist only to drop their `(K, V)` data; their fields
are garbage and never read. Only span boundaries are read:

- First tombstone in span: `span_start` points to itself (identifies it as span
  start), `after_span` is the index of the next live entry (or `vec.len()`).
- Last tombstone in span: `span_start` points to the first tombstone in the
  span, `after_span` points past the span (identifies it as span end).
- A span of 1: both fields are valid (it's both first and last).

**Removal algorithm** (removing entry at vec index `i`):
1. Replace `vec[i]` with `Tombstone { span_start: i, after_span: i + 1 }`
   (a span of 1).
2. If `i + 1` is in bounds and is a tombstone, read its `after_span` to find
   the rightmost tombstone of that span. Otherwise, we are the rightmost
   tombstone of our span.
3. If `i - 1` is in bounds and is a tombstone, read its `span_start` to find
   the leftmost tombstone of that span. Otherwise, we are the leftmost
   tombstone of our span.
4. Update the leftmost tombstone's `after_span` to the rightmost tombstone's
   `after_span`. Update the rightmost tombstone's `span_start` to the leftmost
   tombstone's `span_start`.

All steps are O(1). No cascading updates, no interior tombstone reads.

**Iteration:** On hitting a tombstone, read its `after_span`; if it equals
`vec.len()`, iteration is done, otherwise jump to that index.

**Example:** Vec after inserting A,B,C,D,E,F then removing B, C, and D
(in any order):
```
[Live(A), Tomb{ss:1, as:4}, Tomb{--,--}, Tomb{ss:1, as:4}, Live(E), Live(F)]
```
Interior tombstone at index 2 has garbage fields — it exists only to drop the
`(K, V)` that was there. Iterator at index 1 reads `after_span=4`, jumps to
index 4 (E).

**Interface encapsulation:** The data structure must expose only an opaque
iteration + lookup interface. Nothing internal to Tabulosity should know about
the vec or tombstone structure — just that the index supports O(1) lookup and
deterministic iteration. This allows a future drop-in replacement (e.g., a
forked IndexMap with compaction, see F-tab-indexmap-fork) without changing any
Tabulosity internals.

**Open design questions:**
- Compaction threshold: e.g., compact when tombstones > 50% of vec length, or
  when tombstones > N (absolute). TBD based on profiling.
- Serde: the vec (with tombstones removed) should be serialized alongside the
  table to preserve insertion order across save/load.

**Unblocked:** F-tab-hash-idx
**Related:** F-tab-hash-idx, F-tab-indexmap-fork

#### F-tab-parent-pk — Tabulosity: allow parent PK as child table PK for 1:1 relations
**Status:** Done

**Related:** F-child-table-pks, F-tab-nonpk-autoinc

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

### Multiplayer

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

**Related:** F-immediate-commands, F-mp-chat, F-mp-checksums, F-mp-integ-test, F-mp-mid-join, F-mp-reconnect, F-multi-tree, F-relay-multi-game, F-relay-release, F-save-load, F-session-sm

#### F-player-identity — Persistent player identity with username
**Status:** Done · **Phase:** 2

Persistent player identity system replacing the vestigial PlayerId UUID.

Core changes:
- Remove PlayerId type (UUID-based singleton on SimState)
- New Player table in SimDb: { name: String, civ_id: Option<CivId> }
- SimCommand::player_id → SimCommand::player_name: String
- Tree::owner: Option<PlayerId> → Tree::owner: Option<CivId>
- Single-player: one Player entry created automatically
- Multiplayer: Player entry created per human on join

Client side:
- First launch prompts for username (no config.json player_name)
- Username saved to user://config.json via GameConfig, reused across sessions
- Host sends username from config (no more hardcoded "Host")
- Relay rejects duplicate usernames within a session

The Player table provides the per-player identity key needed for
F-selection-groups and camera location hotkeys — data that must be
saved per-player so multiplayer participants don't clobber each other.

**Unblocked:** F-selection-groups
**Related:** F-config-file, F-selection-groups

#### F-relay-multi-game — Relay server supports multiple simultaneous games
**Status:** Done · **Phase:** 8

Extend the relay server to host multiple simultaneous game sessions.
Each session has its own independent lobby, command queue, turn counter,
and connected-client list. Clients specify which session to join (or
create) during the handshake. The relay multiplexes all sessions on a
single listening port. Requires per-session isolation so that a crash
or desync in one game cannot affect others. Depends on F-relay-release
(standalone relay must exist first) and F-multiplayer (core relay
protocol).

**Related:** F-multiplayer, F-relay-release

#### F-relay-release — Standalone relay server release build
**Status:** Done · **Phase:** 8

Build the `elven_canopy_relay` crate as a standalone headless binary
with a release profile. Add a `scripts/build.sh relay` (or similar)
target that produces an optimized, stripped binary suitable for
deployment on a dedicated server. Include any necessary Cargo profile
tuning (LTO, codegen-units=1, strip=true) for minimal binary size and
maximum performance.

**Related:** F-multiplayer, F-relay-multi-game

### Testing Infrastructure

#### B-fragile-tests — Audit and harden tests against PRNG stream shifts and worldgen changes
**Status:** Todo

Audit and harden sim tests against PRNG stream shifts and worldgen changes.

Many combat and projectile tests are fragile: they rely on specific
creature stat values produced by a particular PRNG seed, so any change
to PRNG consumption during worldgen or creature spawn causes cascading
test failures. These tests pass for the wrong reason — they happen to
get the right random numbers, not because they've isolated the behavior
under test.

**Incident 1 (F-attack-evasion, 2026-03-24):** Adding the evasion
hit-check (12 extra PRNG calls per attack) shifted the PRNG stream,
breaking 28 combat tests that asserted exact damage values. All 28
needed `zero_creature_stats` + `force_guaranteed_hits` to make them
deterministic regardless of PRNG state.

**Incident 2 (quasi-normal-util, 2026-03-24):** Extracting the
quasi-normal distribution function changed the internal sampling range
used during creature stat generation (from [-stdev, stdev] to
[-100M, 100M] with scaling). This shifted the PRNG stream during
spawn, breaking 14 more combat tests that still depended on the
specific stat rolls from seed 42. Same fix: zero stats + force hits.

**Incident 3 (leaf density, earlier):** The test_config already pins
`leaf_density` and `leaf_size` with the comment "Pin leaf config so
tests don't break when visual defaults change." This was added after
fruit tests broke when tree growth parameters changed — fruit positions
depend on leaf voxel positions which depend on tree growth which
depends on PRNG state. The pin was a targeted fix but the underlying
pattern (tests depending on specific worldgen output) persists. The
fruit tests remain fragile — they are merely pinned, not hardened.

**The general problem:** Tests that use `test_sim(42)` inherit a full
worldgen result — tree shape, creature stats, nav graph, fruit
positions — and some tests implicitly depend on specific details of
that worldgen output. When anything upstream in the PRNG stream changes
(new PRNG calls, different sampling ranges, reordered operations), the
entire worldgen output shifts and these tests break.

**What "hardened" looks like:**
- Combat tests should always `zero_creature_stats` on both attacker and
  defender, then set only the specific stats the test needs. The
  `force_guaranteed_hits` helper should be used whenever exact damage
  values are asserted.
- Tests that assert positions or coordinates should use positions
  derived from the test setup (e.g., "place creature at X, check
  result at X+1") not positions inherited from worldgen.
- Tests that depend on tree shape (fruit positions, nav graph
  connectivity) should either pin all relevant config parameters or
  build a minimal test-specific world rather than relying on the
  cached seed-42 sim's exact tree.
- No test should break when: (a) a new PRNG call is added anywhere in
  worldgen/spawn, (b) an existing PRNG sampling range changes, (c)
  tree growth parameters or algorithms change, (d) species stat
  distributions change.

**Audit scope:** All tests in `elven_canopy_sim/src/sim/tests.rs` that
use `test_sim(42)` and make assertions about exact numeric values
(HP, damage, positions, counts of specific items). The combat tests
fixed in incidents 1-2 above were hardened against stat-related PRNG
shifts specifically, but may still be fragile to other worldgen
changes. Fruit tests (incident 3) were never hardened — only pinned.
The audit should examine all test categories for remaining fragility,
not assume any are fully hardened.

#### F-ai-test-harness — Remote game control for AI-driven testing (Puppet)
**Status:** Done

Remote game control for AI-driven testing ("Puppet"). Three components:
a GDScript TCP server autoload (puppet_server.gd, activated by
PUPPET_SERVER=<port> env var, inert otherwise), shared UI helpers
(puppet_helpers.gd, also used by GUT integration tests), and a
standalone Python CLI (scripts/puppet.py, stdlib-only) that manages
the full lifecycle — launch under xvfb-run or --visible, communicate,
kill.

**Done:** 18 built-in RPC methods — observe (game-state, list-panels,
is-panel-visible, read-panel-text, find-text, collect-text, tree-info,
list-structures, ping) and act (click-at-world-pos, press-key,
press-button, press-button-near, step-ticks, set-sim-speed,
move-camera-to, quit). Helper extraction from integration tests into
shared puppet_helpers.gd. 33 unit tests (test_puppet.gd). Python CLI
with launch (xvfb-run or --visible), kill (graceful+SIGTERM+SIGKILL),
list, -g session targeting. Orphan guard (300s default). Localhost-only
TCP binding. Godot output logged to .tmp/puppet-<id>.log. End-to-end
validated: menu navigation, game-state queries, UI text scraping,
tree info, military panel, mana tracking across ticks.

**Not yet implemented (from spec):** list-creatures (needs new bridge
method), creature-info (bridge has get_creature_info_by_id but no RPC
glue), select-creature (needs SelectionController glue), eval escape
hatch (arbitrary GDScript execution), -- command chaining in puppet.py.

**Desired features (discovered through use):**
- wait-for-ready: block until bridge is available after scene load
  (currently must poll ping manually, 30-120s on constrained systems)
- wait-until: server-side poll for a condition (panel visible, text
  appears) — eliminates client-side retry loops
- collect-text filtering: --node-name flag to return only matching
  node names (e.g., collect-text UnitsPanel --node-name NameLabel)
- step-ticks-and-wait: yield a frame after stepping so UI is fresh
  before responding
- scene-info: return current scene path/name (menu vs game) so callers
  know what state they're in without trying bridge calls
- read-all-by-name: return all nodes matching a name, not just first
  (solves the NameLabel collision problem)

**Guide:** docs/puppet_guide.md — keep updated when adding RPCs or
discovering gotchas.

**Draft:** docs/drafts/F-ai-test-harness.md

**Related:** F-bridge-integ-tests, F-gdscript-tests

#### F-bridge-integ-tests — Integration tests for gdext bridge functions
**Status:** Done

Integration tests that exercise gdext bridge functions from GDScript in a
running Godot instance. These catch type mismatches at the FFI boundary
(e.g. Array<GString> vs VarArray), argument passing bugs, and
GDScript-to-Rust round-trip issues that pure Rust tests can't detect.
Heavier than unit tests — requires launching Godot headless. Add a CI
job and a `scripts/build.sh integtest` target. Motivated by the
F-move-spread segfault where an Array type mismatch crashed at runtime.

**Draft:** docs/drafts/F-bridge-integ-tests-and-ai-test-harness.md

**Related:** F-ai-test-harness, F-config-file, F-gdscript-tests

#### F-gdscript-tests — GDScript unit tests (GUT or built-in)
**Status:** Done

Set up a lightweight GDScript unit testing framework (GUT or Godot 4.6's
built-in test runner). Cover pure GDScript logic: UI state machines,
coordinate math, selection helpers, input mode transitions. Add a
`scripts/build.sh gdtest` target and a CI job. These tests don't need
the sim or bridge — just GDScript in isolation.

**Related:** F-ai-test-harness, F-bridge-integ-tests

#### F-test-perf — Test performance audit: per-test timing
**Status:** Todo

Audit test suite for slow tests and add per-test timing visibility.

**Rust:** Install `cargo-nextest` (`cargo install cargo-nextest`) and wire it into `build.sh`. Drop-in replacement for `cargo test` that shows per-test durations. Alternatively, the built-in `--report-time` flag works on nightly (`cargo test -- -Z unstable-options --report-time`).

**GDScript/GUT:** GUT already tracks `time_taken` per test internally but doesn't print it to console. Two options: (1) enable JUnit XML export via `"junit_xml_file"` in `.gutconfig.json` — each `<testcase>` gets a `time` attribute; (2) patch GUT's `logger.gd` or `summary.gd` to print `time_taken` inline.

**Goal:** Identify slow tests, decide which should be `#[ignore]`d or optimized, and make timing a routine part of test output.

### Platform

#### F-mobile-support — Mobile/touch platform support
**Status:** Todo · **Phase:** 9

Touch-based input and UI adaptation for mobile phones and tablets. Covers
camera controls (pinch-to-zoom, two-finger rotate, single-finger pan),
selection (tap-to-select, lasso gesture for multi-select), command input
(explicit move/attack/attack-move buttons instead of right-click), and a
mobile-density UI layout (full-screen slide-in panels, larger touch targets,
simplified HUD). No keyboard hotkeys — all actions need touch-accessible
equivalents (toolbars, radial menus, gesture shortcuts).

**Draft:** `docs/drafts/mobile_support.md`

