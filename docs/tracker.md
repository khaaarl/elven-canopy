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
[~] B-fragile-tests        Audit and harden tests against PRNG stream shifts and worldgen changes
[~] B-ghost-chunks         Ghost chunks in distance remain visible after they should be hidden
[~] B-qem-deformation      QEM decimation visual artifacts
[~] F-creature-skills      Creature skill system (17 universal skills with path-gated advancement)
[~] F-enemy-ai             Hostile creature AI (goblin/orc/troll behavior)
[~] F-face-tint            Directional face tinting by normal (top warm, bottom cool)
[~] F-fruit-variety        Procedural fruit variety and processing
[~] F-multiplayer          Relay-coordinator multiplayer networking
[~] F-notifications        Player-visible event notifications
[~] F-parallel-dedup       Radix-partitioned parallel dedup (elven_canopy_utils)
[~] F-path-ui              Path management UI and notifications
[~] F-ssao                 Screen-space ambient occlusion toggle
```

### Todo

```
[ ] B-dijkstra-perf        Unbounded Dijkstra in nearest-X searches scales poorly on large graphs
[ ] B-doubletap-groups     Double-tap selection group recall inconsistently triggers camera center
[ ] B-flying-flee          Flying creatures flee by random wander instead of directionally
[ ] F-ability-hotkeys      RTS-style bindable ability hotkeys on creatures
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
[ ] F-bldg-library         Magic learning building (library/spire)
[ ] F-bldg-storehouse      Storehouse (item storage)
[ ] F-blueprint-mode       Layer-based blueprint selection UI
[ ] F-boundary-decim       Mesh decimation at chunk boundaries
[ ] F-branch-growth        Grow branches for photosynthesis/fruit
[ ] F-bridges              Bridge construction between tree parts
[ ] F-buff-system          Generic timed stat modifier buffs on creatures
[ ] F-build-queue-ui       Construction queue/progress UI
[ ] F-building-civ         Building civilization ownership and civ-filtered building access
[ ] F-building-door        Player-controlled building door orientation
[ ] F-cascade-fail         Cascading structural failure
[ ] F-cavalry              Mount tamed creatures as cavalry
[ ] F-choir-build          Choir-based construction singing
[ ] F-choir-harmony        Ensemble harmony in construction singing
[ ] F-civ-knowledge        Civilization knowledge system (fruit tiers, discovery)
[ ] F-civ-pets             Non-elf civ members and pets
[ ] F-cloak-slot           Cloak/cape equipment slot
[ ] F-combat               Combat and invader threat system
[ ] F-combat-opinions      Respect and fear from combat events
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
[ ] F-dappled-light        Dappled light effect via scrolling noise on ground shader
[ ] F-day-night            Day/night cycle and pacing
[ ] F-day-night-color      Color grading shift by time of day
[ ] F-defense-struct       Defensive structures (ballista, wards)
[ ] F-demolish             Structure demolition
[ ] F-dining-furnishing    Dining hall tables and seating furnishings
[ ] F-dinner-party         Organized group dining social activity
[ ] F-docs-overhaul        Reorganize and consolidate docs/ directory
[ ] F-dwarf-fort-gen       Underground dwarf fortress generation
[ ] F-dye-application      Apply dye to equipment at workshop
[ ] F-dye-mixing           Dye color mixing recipes
[ ] F-dye-palette          Named color palette system for dyes
[ ] F-edge-outline         Edge highlighting shader (depth/normal discontinuity)
[ ] F-elaborate-social     Elaborate casual social interactions (visible pauses, variety, personality)
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
[ ] F-formal-bonds         Formal bonds (marriage, parent-child, fiance)
[ ] F-fov-slider           FOV slider in settings
[ ] F-fruit-pigments       More natural fruit pigment colors (secondaries on fruit parts)
[ ] F-fruit-prod           Basic fruit production and harvesting
[ ] F-fruit-sprite-ui      Fruit sprites in inventory/logistics/selection UI
[ ] F-funeral-rites        Funeral rites and mourning
[ ] F-genetics             Creature genetics (additive SNP bitfield genomes with inheritance)
[ ] F-grass-rendering      Advanced grass and terrain surface rendering
[ ] F-greenhouse-revamp    Greenhouse planter growth cycle and pluck tasks
[ ] F-group-chat           Group chat social activity
[ ] F-hedonic-adapt        Asymmetric hedonic adaptation
[ ] F-herbalism            Herbalism and alchemy
[ ] F-herding              Manage animal groups with pens and grazing areas
[ ] F-infra-decay          Infrastructure decay with automated maintenance
[ ] F-insect-husbandry     Beekeeping and insect husbandry
[ ] F-instinctual-flee     Instinctual flee thresholds (species-level fear overrides)
[ ] F-interleaved-astar    Interleaved A* for efficient nearest-among-N-candidates pathfinding
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
[ ] F-mesh-perf-2          Mesh pipeline performance optimization, round 2
[ ] F-military-campaign    Send elves on world expeditions
[ ] F-military-org         Squad management and organization
[ ] F-mobile-support       Mobile/touch platform support
[ ] F-modding              Scripting layer for modding support
[ ] F-modifier-keybinds    Modifier key combinations in bindings
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
[ ] F-pet-names            Named tamed animals
[ ] F-phased-archery       Phased archery (nock/draw/loose) with skill-gated mobility
[ ] F-poetry-reading       Social gatherings and poetry readings
[ ] F-population           Natural population growth/immigration
[ ] F-proc-poetry          Procedural poetry via simulated annealing
[ ] F-quality-filters      Quality filters for logistics wants and active recipes
[ ] F-raid-detection       Raid detection gating and stealth spawning
[ ] F-raid-polish          Raid polish: military groups, provisions for long treks
[ ] F-random-seeds         Parameterized random-seed testing for hardened sim tests
[ ] F-recipe-any-mat       Any-material recipe parameter support
[ ] F-rescue               Rescue and stabilize incapacitated creatures
[ ] F-retire-events        Retire event queue: poll-based heartbeats and periodic systems
[ ] F-romance              Romantic relationships and courtship
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
[ ] F-social-dance         Dance integration with social opinions
[ ] F-social-graph         Relationships and social contagion
[ ] F-social-intensity     Context-dependent social impression strength tuning
[ ] F-social-prefer        Social preference in group activity recruitment
[ ] F-social-ui            Social tab on creature info panel
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
[ ] F-unified-pursuit      Unify pursue_closest_target ground/flight paths via find_nearest
[ ] F-uplift-tree          Uplift lesser tree into bonded great tree
[ ] F-vaelith-expand       Expand Vaelith language for runtime use
[ ] F-vertical-garden      Vertical gardens on the tree
[ ] F-voxel-ao             Per-vertex ambient occlusion baked into chunk meshes
[ ] F-want-categories      Categorical want specifications (any footwear, any melee weapon)
[ ] F-war-animals          Train tamed creatures for combat
[ ] F-war-magic            War magic (combat spells)
[ ] F-weather              Weather within seasons
[ ] F-wild-bushes          Wild fruit bushes at ground level
[ ] F-wild-foraging        Wild animal foraging for fruit
[ ] F-wild-fruit           Wild fruit growing on bushes and ground-level plants
[ ] F-windows-compat       Windows compatibility for dev tooling
[ ] F-winged-elf           Winged elf species variant with flight-only movement
[ ] F-wireframe-ghost      Wireframe ghost for overlap preview
[ ] F-wood-stats           Wood-type material variation for crafted items
[ ] F-world-boundary       World boundary visualization
[ ] F-world-map            World map view
[ ] F-zone-world           Zone-based world with fidelity partitioning
```

### Done

```
[x] B-assembly-timeout     Activity assembly timeout not enforced
[x] B-carve-dirt           Carving pure dirt voxels rejected as nothing to carve
[x] B-carve-perf           Carving dirt causes severe CPU stall, possibly structural checks
[x] B-chamfer-nonmfld      Chamfer produces non-manifold edges for diagonally-adjacent voxels
[x] B-combat-move-stats    Combat movement timing ignores creature stats
[x] B-dead-enums           Remove dead GrownStairs/Bridge code and add explicit enum discriminants
[x] B-dead-max-gen         Remove vestigial max_gen_per_frame field and ~40 test calls
[x] B-dead-node-panic      Panic on dead nav node in pathfinding
[x] B-dead-owner-items     Dead creature items retain ownership, becoming invisible to all systems
[x] B-dine-orphan-task     DineAtHall speculative task leaves orphaned Complete rows
[x] B-dining-perf          Dining hall search causes intermittent multi-second pauses
[x] B-dirt-not-pinned      Dirt unpinned in fast structural validator
[x] B-erratic-movement     Erratic/too-fast creature movement after move commands
[x] B-escape-menu          Rename pause_menu to escape_menu and block hotkeys/buttons while it's open
[x] B-first-notification   First notification not displayed (ID 0 skipped by polling cursor)
[x] B-floating-dirt        Floating dirt still treated as ground by structural validator
[x] B-flying-arrow-chase   Flying creatures excluded from arrow-chase
[x] B-flying-tasks         Flying creatures skip task system entirely
[x] B-hostile-detect-nav   detect_hostile_targets panics on flying targets (NavNodeId u32::MAX hack)
[x] B-leaf-diagonal        Leaf blobs sometimes only diagonally connected, looks bad
[x] B-mesh-global-cfg      Mesh pipeline global atomics cause test flakiness risk
[x] B-modifier-hotkeys     Hotkeys should not fire when modifier keys (Ctrl/Shift/Alt) are held
[x] B-music-floats         Excise f32/f64 from music composition for determinism
[x] B-preview-blueprints   Preview treats blueprints as complete
[x] B-quit-crash           Crash on quit from in-flight rayon mesh workers
[x] B-raid-spawn           Raiders sometimes spawn inside map instead of at perimeter
[x] B-sim-floats           Remaining f32/f64 in sim logic threaten determinism
[x] B-spawn-creature       spawn_creature test helper finds first creature of species, not newly spawned
[x] B-start-paused-ui      start_paused_on_load UI desync and missing new-game support
[x] B-tab-serde-tests      Fix tabulosity test compilation under feature unification
[x] B-task-civ-filter      Tasks lack civilization-level eligibility filtering
[x] B-unsafe-db-calls      Replace _no_fk and modify_unchecked calls with safe database-level methods
[x] B-win-freeze           Periodic ~3s freezes on Windows (debug build)
[x] F-activation-revamp    Replace manual event scheduling with automatic reactivation
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
[x] F-bldg-dining          Dining hall
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
[x] F-casual-social        Opportunistic passing social interactions
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
[x] F-creature-sex         Creature sex/gender field
[x] F-creature-stats       Creature stats (str/agi/dex/con/wil/int/per/cha)
[x] F-creature-tooltip     Hover tooltips for world objects
[x] F-dance-self-org       Elves self-organize dances
[x] F-dblclick-select      Double-click to select all of same military group
[x] F-debug-menu           Move spawn/summon into debug menu
[x] F-distance-fog         Depth-based atmospheric fog/haze
[x] F-dye-crafting         Dye pressing from pigmented fruit components
[x] F-dynamic-pursuit      Dynamic repathfinding for moving-target tasks
[x] F-edge-scroll          Configurable edge scrolling (pan, rotate, or off)
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
[x] F-mesh-par             Parallel off-main-thread chunk mesh generation with camera-priority
[x] F-mesh-pipeline-perf   Mesh pipeline performance optimization (mesh gen, chamfer, decimation)
[x] F-military-armor       Military equipment auto-equip and slot validation
[x] F-military-equip       Military group equipment acquisition
[x] F-military-groups      Military group data model and configuration
[x] F-minimap              Minimap with tree silhouette and creature positions
[x] F-mmb-pan              Ctrl+MMB drag to pan camera horizontally
[x] F-mood-system          Mood with escalating consequences
[x] F-mouse-elevation      Ctrl+mouse wheel to move camera elevation
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
[x] F-skill-check-helper   Unified skill_check helper and game-mechanics doc
[x] F-social-opinions      Interpersonal opinion table and social skill checks
[x] F-spatial-index        Creature spatial index for voxel-level position queries
[x] F-spawn-toolbar        Spawn toolbar and placement UI
[x] F-split-sim            Split monolithic sim.rs into domain sub-modules
[x] F-split-sim-tests      Split sim tests.rs into per-module test files
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
[x] F-taming               Tame neutral creatures via Scout-path elves
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
[x] F-unified-pathing      Unified pathfinding API for ground (nav graph) and flying (voxel grid) creatures
[x] F-visual-smooth        Smooth voxel surface rendering
[x] F-voice-subsets        Variable voice count (SATB subsets)
[x] F-voxel-exclusion      Creatures cannot enter voxels occupied by hostile creatures
[x] F-voxel-fem            Voxel FEM structural analysis
[x] F-voxel-textures       Per-face Perlin noise voxel textures
[x] F-wild-grazing         Wild animal herbivorous food cycle
[x] F-worldgen-framework   Worldgen generator framework
[x] F-wyvern               Wyvern hostile flying creature (2×2×2)
[x] F-zlevel-vis           Z-level visibility (cutaway/toggle)
```

---

## Detailed Items

Full descriptions grouped by area. Each item includes design doc references,
draft docs, and blocking relationships where relevant.

### Construction

#### B-carve-dirt — Carving pure dirt voxels rejected as nothing to carve
**Status:** Done

#### B-carve-perf — Carving dirt causes severe CPU stall, possibly structural checks
**Status:** Done

**Related:** B-floating-dirt

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
**Status:** Done · **Phase:** 4

Communal dining building where elves eat meals. Two hunger thresholds:
food_dining_threshold_pct (new, higher) triggers dining hall seek;
food_hunger_threshold_pct (existing, lowered) triggers carried food /
foraging. Dining gives mood boost (AteDining); non-dining eating gives
small penalty (AteAlone). Tables have implicit seats; capacity =
tables × dining_seats_per_table. Food stocked via logistics wants.
Elf reserves seat + food item, paths to table, eats instantly on
arrival. Interrupted elves release reservations, food preserved.

**Draft:** docs/drafts/F-bldg-dining.md

**Related:** F-bldg-concert, F-bldg-kitchen, F-dining-furnishing, F-dinner-party, F-food-chain, F-food-quality-mood, F-slow-eating

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

#### F-dining-furnishing — Dining hall tables and seating furnishings
**Status:** Todo

Better dining room furnishing with proper tables and seating. Currently
dining halls use generic eating locations — there's no concept of tables
with chairs arranged around them. This feature would introduce table
furnishings that organize seats spatially, so elves sit across from or
beside each other at a table rather than at arbitrary spots in a room.

**Why:** Visual and mechanical fidelity — a dining hall should look like
a place where people eat together, not just a room with food in it.
Table arrangement could also feed into social mechanics (e.g., who you
sit next to matters more than who's across the room).

**Scope:** New furnishing types (table, chair/bench), spatial seat
assignment relative to tables, and rendering. May interact with dinner
party seat selection (F-dinner-party) and future social preference
systems (F-social-prefer).

**Related:** F-bldg-dining, F-dinner-party, F-furnish, F-sung-furniture, F-unfurnish

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

**Related:** F-batch-craft, F-bldg-dormitory, F-building, F-dining-furnishing, F-greenhouse-revamp, F-unfurnish

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

#### F-mesh-pipeline-perf — Mesh pipeline performance optimization (mesh gen, chamfer, decimation)
**Status:** Done

Pure code-level single-threaded performance optimization of the chunk mesh pipeline: mesh generation, chamfering, and decimation. Bit-fiddling optimization work — algorithmic structure stays the same, but inner loops get faster. Each chunk is already processed on its own thread (F-mesh-par); this work targets the per-chunk cost. No rayon or intra-chunk parallelism.

**Phase 1 — Snapshot regression harness and benchmarks:**

Build a test harness that captures mesh pipeline outputs (vertices, normals, indices, colors) as snapshots, then compares them before and after code changes to verify output equivalence within floating-point tolerance (~1e-5). Also create criterion benchmarks for the mesh pipeline (none exist yet in `elven_canopy_sim`) so that Phase 2 agents can measure performance impact.

Snapshot sources:
- Hand-built test chunks: single voxel, flat slab, L-shape, staircase, diagonal adjacency, mixed material, fully solid, fully empty, thin wall, overhang.
- Building chunks: chunks containing constructed buildings (walls, floors, platforms, dormitories, etc.) — buildings introduce novel constraints on mesh construction (thin walls, multi-material junctions, interior/exterior face adjacency).
- World-generated chunks: run worldgen with a fixed seed, extract a representative sample of `ChunkNeighborhood`s (surface-heavy, underground, canopy, trunk-adjacent, sparse, building-containing) and freeze them as test fixtures.

What's captured per snapshot:
- Raw mesh output (positions, normals, colors, indices) from mesh generation.
- Post-chamfer `SmoothMesh` output.
- Post-decimation `SmoothMesh` output.
- Vertex count, triangle count, and bounding box as quick-check metadata.

Comparison: position/normal components with configurable epsilon (default 1e-5), index buffers exactly, color values exactly. Quick-check metadata compared first for fast failure.

**Phase 2 — Iterative agent-driven optimization:**

Each optimization attempt is performed by a fresh agent in a loop. The agents share a persistent optimization diary at `.tmp/mesh-perf-diary.md` that records every attempt — idea, rationale, result, and whether the change was kept or reverted.

**Agent loop procedure:**

Each agent:
1. Reads the optimization diary and the current codebase to understand what has been tried before.
2. Devises a novel optimization idea that hasn't been attempted, or revisits a previously-failed idea only if enough subsequent code changes have landed that the context is materially different.
3. Appends the idea and rationale to the diary before starting work.
4. Implements the optimization.
5. Runs the snapshot regression harness to verify correctness (identical output within epsilon).
6. Runs criterion benchmarks to measure performance impact.
7. **If the optimization improves performance and passes correctness:** commits and pushes the change.
8. Waits for CI via `scripts/wait-for-ci.sh`. If CI fails, investigates and attempts to fix (which may require re-running benchmarks to confirm the fix doesn't regress performance). If unrecoverable, treats this as a failure — reverts the commit and records the failure reason in the diary.
9. **If CI passes:** records the improvement in the diary (benchmark numbers before/after).
10. **If it doesn't improve performance or breaks correctness:** reverts all changes and records the failure reason in the diary.
11. The agent terminates.

The next agent spawns and repeats from step 1. This continues until the user stops or **10 consecutive agent attempts fail**, whichever comes first. The loop can plausibly run for hours (e.g., overnight).

A "failure" is any of: no measurable performance improvement, correctness regression (snapshots don't match), CI failure that can't be fixed, or a revert for any other reason. Only a committed, pushed, CI-green optimization with measured improvement counts as success and resets the consecutive failure counter.

**Optimization search space (non-exhaustive):**

The agents are not limited to the suggestions below — any single-threaded optimization is fair game:
- Mesh gen: face visibility checks, neighbor lookups via the dense voxel array, vertex/normal/color construction, RLE column span iteration, memory allocation patterns.
- Chamfer: vertex displacement toward anchored-neighbor centroids, saddle-skip heuristic for diagonal neighbors, the solid-then-leaf two-phase pass, adjacency queries on the `SmoothMesh` connectivity graph.
- Decimation: the full pipeline is coplanar region re-triangulation → collinear boundary vertex collapse → QEM edge collapse. Targets include QEM matrix operations, edge collapse candidate selection, priority queue overhead, topology bookkeeping, boundary/color/anchor preservation checks. Only the final mesh output needs snapshot verification — intermediate sub-pass outputs don't need to be frozen.
- Lookup tables, branchless logic, branch elimination in hot loops.
- Cache-friendly data layouts, struct-of-arrays vs array-of-structs.
- SIMD intrinsics for vector/matrix operations (QEM matrices, vertex displacement, normal computation).
- Alternative data structures (e.g., spatial hashing, flat arrays replacing tree structures, arena allocation).
- Reduced allocations, buffer pooling, scratch space reuse.
- Algorithmic micro-optimizations (early exits, tighter bounds, redundant-work elimination).
- Inlining hints, cold/hot path separation.

**Correctness gate:** Every optimization must pass the snapshot regression harness — identical output within tolerance on all test chunks, both hand-built and world-generated.

**Note:** B-qem-deformation and B-chamfer-nonmfld are known bugs in the chamfer/decimation pipeline. If either gets fixed, the expected snapshot outputs will change and the frozen comparisons will need to be regenerated.

**Related:** F-mesh-perf-2, F-smooth-perf

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

**Related:** F-mesh-pipeline-perf, F-visual-smooth

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

**Related:** F-choir-build, F-dining-furnishing, F-item-quality, F-mana-system

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

**Related:** F-demolish, F-dining-furnishing, F-furnish

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

#### B-floating-dirt — Floating dirt still treated as ground by structural validator
**Status:** Done

Dirt voxels are unconditionally pinned (immovable ground) in the structural
model (`structural.rs:1073`). The only protection against carving all
supporting dirt is `designate_carve`'s y>0 bedrock guard, which prevents
carving the y=0 layer but allows carving all dirt above it.

This means a player can carve a horizontal tunnel through dirt at y=1,
isolating a column of dirt above it. That floating dirt column is still
treated as ground by the structural validator — any structure resting on
it (tree trunk, platforms, buildings) would pass structural checks despite
having no actual path to bedrock.

**Scenario:**
1. Terrain has dirt from y=0 to y=3.
2. Player carves a horizontal ring at y=1 around a 3x3 column.
3. The y=2-3 dirt above the ring is now floating (no face-adjacent path
   to y=0 bedrock through dirt).
4. A tree trunk on that floating dirt passes structural validation because
   the dirt is pinned.

**Root cause:** The structural model assumes all dirt is bedrock-equivalent.
There is no connectivity check from dirt voxels back to the actual bedrock
layer (y=0).

**Possible fix:** A dirt voxel should only be pinned if it can reach the
bedrock layer (y=0) through a contiguous path of dirt. The world is
1024x1024x~50 dirt voxels (~52M total), so any approach touching "all dirt"
is prohibitively expensive. The check needs to be fast for the common case
(unbroken dirt column) and bounded for the uncommon case (carved terrain).

Preferred approach: **RLE-aware downward-biased A\* search.**

- **Common case O(1):** VoxelWorld stores columns as RLE spans
  (`world.rs:column_spans()`). If the column at (x,z) has a dirt span
  covering y=0 through the carved voxel's Y, the dirt trivially reaches
  bedrock — one span lookup, no search needed.

- **Carved-column case:** If the column has been carved (multiple spans),
  use A\* from the dirt voxel toward y=0 through face-adjacent dirt,
  with a heuristic strongly biased downward (h = manhattan distance to
  nearest y=0 voxel). The downward bias means A\* finds bedrock quickly
  in typical terrain without exploring laterally.

- **RLE column jumping:** When A\* expands a node at (x,y,z), instead of
  stepping one voxel at a time in -Y, read the column spans at (x,z) to
  determine the full extent of the dirt span containing y, and jump
  directly to the bottom of that span. This makes vertical traversal
  through uncarved columns O(1) regardless of column height.

- **Disconnected dirt (worst case):** If the dirt is truly floating (no
  path to bedrock), A\* terminates after exhausting the connected dirt
  component. This component is bounded by the carve geometry — for a
  small carve isolating a small island, the search is small.

**Interaction with B-carve-perf:** The perf fix skips stress analysis for
all-dirt carves (since removing pinned voxels can't overstress above-ground
structure). Once this bug is fixed, the "is this dirt actually ground?"
check would need to run during dirt carving to detect newly-floating dirt
and trigger gravity/collapse on the disconnected island. The perf
optimization remains valid — skip stress, but add the dirt connectivity
check.

F-carve-holes (carving feature).

**Related:** B-carve-perf, B-carve-perf (perf fix skips stress for dirt carves)

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

#### B-dijkstra-perf — Unbounded Dijkstra in nearest-X searches scales poorly on large graphs
**Status:** Todo · **Refs:** §4

Several nearest-X search functions use `dijkstra_nearest()` with the creature
as source and target locations as multi-target sinks. Dijkstra explores outward
with no heuristic, so when targets are far away or rare, it searches the entire
reachable nav graph. This is O(V log V + E) on the graph regardless of target
distance.

**Affected callers (all in needs.rs unless noted):**

- `find_nearest_fruit()` (line 52) — nearest harvestable fruit
- `find_nearest_bed()` — **fixed** by B-dining-perf (now uses per-candidate A*)
- `find_nearest_dining_hall()` — **fixed** by B-dining-perf (now uses per-candidate A*)
- `find_available_task()` (activation.rs:690) — nearest claimable idle task

**Preferred fix:** Same pattern as B-dining-perf — gather candidates first
(cheap filter), then run point-to-point A* to each candidate. A* with the
existing chebyshev heuristic terminates much faster for point-to-point queries.
The number of candidates is typically small (a few fruit trees, a few beds),
so multiple A* calls will still be far cheaper than one unbounded Dijkstra.

**Priority:** Lower than B-dining-perf. Fruit and beds are more numerous and
more evenly distributed than dining halls, so the Dijkstra tends to find them
sooner. But on large maps with sparse resources, the same O(entire graph)
worst case applies.

**Verification:** F-sim-perf-timing instrumentation will surface these if they
become a bottleneck.

**Related:** B-dining-perf, F-interleaved-astar, F-unified-pathing

#### B-dining-perf — Dining hall search causes intermittent multi-second pauses
**Status:** Done · **Refs:** §4

Intermittent 2-second pauses observed during gameplay, likely when a creature
triggers `find_nearest_dining_hall()` (needs.rs:621). The function has two
scaling concerns:

1. **Full scan of task_voxel_refs:** Line 636 does `iter_all()` over the entire
   `task_voxel_refs` table to count occupied dining seats. This grows with total
   active tasks across all systems, not just dining tasks. However, this scan
   is only meaningful when there are valid dining hall candidates, so the fix
   is ordering: gate it after the cheap structure/food check, not before.

2. **Unbounded Dijkstra on the nav graph:** Line 691 calls `dijkstra_nearest()`
   with the creature's position as source and all candidate table nodes as
   targets. Dijkstra explores the graph outward from the source with no
   heuristic, so if dining halls are far away or the graph is large, it searches
   the entire reachable graph before finding the nearest target. This is the
   likely main cost — a large nav graph with distant dining halls means
   exploring thousands of nodes per hungry creature, and multiple creatures can
   get hungry in the same tick window.

**Preferred fix:**

1. **Reorder the function:** First, gather candidate dining halls using the
   cheap structure scan (filter for food + free seats). If no candidates,
   return None immediately — no seat counting, no pathfinding.

2. **Gate the seat count scan:** Only run the `task_voxel_refs` iteration
   after confirming there are dining halls with food. Do not add a new
   tabulosity index for this — the write-side cost on every task voxel ref
   mutation outweighs the read-side benefit for this infrequent query. The
   existing `iter_all()` filtered by role is fine once it's behind the cheap
   gate.

3. **Replace Dijkstra with candidate-first A\*:** Instead of multi-target
   Dijkstra from the creature outward, run point-to-point A* (already
   implemented in pathfinding.rs with chebyshev heuristic) to each candidate
   dining hall and pick the one with lowest cost. A* with a good heuristic
   terminates much faster than Dijkstra for point-to-point queries, and dining
   halls are rare enough that the number of A* calls will be small (typically
   1–3). If there's exactly one candidate, a single A* call suffices.

The same Dijkstra pattern exists in `find_nearest_fruit()`, `find_nearest_bed()`,
and `find_available_task()` — those are tracked separately in B-dijkstra-perf.

**Verification:** Once F-sim-perf-timing is implemented, the timing
instrumentation should catch this if it recurs. A targeted before/after
benchmark (time `find_nearest_dining_hall` on a world with distant dining halls)
would also confirm the fix.

**Related:** B-dijkstra-perf, F-interleaved-astar, F-unified-pathing

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

#### F-interleaved-astar — Interleaved A* for efficient nearest-among-N-candidates pathfinding
**Status:** Todo · **Refs:** §4

A general-purpose "find nearest among N candidates" pathfinding algorithm that
interleaves multiple A* searches to terminate early when a close candidate is
found and distant candidates can be pruned.

**Algorithm:**

1. **Pre-filter by heuristic:** Compute `h(creature, candidate)` for all
   candidates and sort ascending. Any candidate whose heuristic lower bound
   already exceeds the best completed path cost can be skipped without
   searching.

2. **Interleaved expansion:** Maintain separate A* open sets for remaining
   candidates. On each step, expand the candidate with the globally smallest
   `f` value. When any search completes with cost `x`, prune all other
   searches whose minimum `f ≥ x` — they can't beat the known solution.

3. **Early out:** If only one candidate survives pruning, switch to normal
   single-target A* for that candidate (no interleaving overhead).

**Degrades gracefully:** One candidate = normal A*. Obvious closest candidate
by heuristic = others pruned without graph work. Worst case (equidistant
candidates) = no worse than sequential A* to each.

**Max-path-len parameter:** Like all pathfinding functions (see F-unified-pathing),
takes a `max_path_len` parameter — the maximum number of edges the resulting
path may traverse. Cul-de-sacs don't consume the allowance; only the final
path length matters. Per-candidate, not shared — the caller shouldn't need to
multiply their ceiling by the number of candidates.

**Must support both pathfinding modes:**

- **Nav graph (ground creatures):** A* on the nav graph with edge-type
  filtering and species-specific traversal costs (walk/climb/ladder TPV),
  using chebyshev heuristic. This replaces the current `dijkstra_nearest()`
  calls.

- **Voxel grid (flying creatures):** A* on the 3D voxel grid with footprint
  clearance checks (1×1×1 for hornets, 2×2×2 for wyverns). The existing
  flight A* already works this way — the interleaved version needs the same
  neighbor generation and clearance logic.

Both modes use the same interleaving/pruning logic; only the neighbor
generation and cost functions differ. A clean design would be generic over
a trait or closure that provides `neighbors(node)` and `heuristic(node, goal)`.

**API sketch:** Both the interleaved A* and multi-target Dijkstra exist as
standalone public functions that callers can reach for directly:
- `nearest_astar_navgraph(...)` / `nearest_astar_fly(...)` — interleaved A*
- `nearest_dijkstra_navgraph(...)` / `nearest_dijkstra_fly(...)` — multi-target Dijkstra

A savvy caller who knows which strategy fits their use case can call the right
one directly. The `nearest_navgraph` / `nearest_fly` wrapper functions from
F-unified-pathing pick between them automatically (interleaved A* in the common
case, Dijkstra when it would be more efficient, e.g., many nearby candidates).
Both return the same result type.

**Users:** `find_nearest_dining_hall()`, `find_nearest_fruit()`,
`find_nearest_bed()`, `find_available_task()`, and any future nearest-X
searches for both ground and flying creatures.

**Unblocked by:** F-unified-pathing
**Related:** B-dijkstra-perf, B-dining-perf

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

#### F-unified-pathing — Unified pathfinding API for ground (nav graph) and flying (voxel grid) creatures
**Status:** Done · **Refs:** §4

Unify the two separate pathfinding implementations — `pathfinding.rs` (nav graph
for ground creatures) and `flight_pathfinding.rs` (voxel grid for flyers) — into
a single file with a consistent API, plus a single test file.

**Current state:** The two files have independent A* implementations with
different signatures, different result types, and no shared abstraction. Callers
must know which system to use and handle them separately. Some operations exist
for one mode but not the other (e.g., `dijkstra_nearest` exists only for nav
graph; flight pathfinding has no multi-target search).

**Goal — two layers:**

1. **Sibling functions with parallel APIs:** Each pathfinding operation has a
   nav-graph version and a voxel-grid version with consistent naming:
   - `astar_navgraph(graph, start, goal, species_data, max_path_len, allowed_edges)` — replaces both current `astar()` and `astar_filtered()` (the only difference between them is edge filtering; use `Option<&[EdgeType]>` to unify)
   - `astar_fly(world, start, goal, footprint, max_path_len)` — current `astar_fly()`
   - `nearest_dijkstra_navgraph(...)` — renamed from current `dijkstra_nearest()`
   - `nearest_navgraph(graph, start, candidates, species_data, max_path_len)` — thin wrapper around `nearest_dijkstra_navgraph` for now (F-interleaved-astar will later add A* as an alternative strategy the wrapper can pick)
   - `nearest_fly(world, start, candidates, footprint, max_path_len)` — new, currently missing

   Where one sibling exists and the other doesn't, add the missing one.

2. **Unified wrappers:** Functions that take a creature (or enough creature
   info to determine travel mode) and dispatch to the appropriate sibling:
   - `astar_for(sim, creature_id, goal, max_path_len)` → calls `astar_navgraph` or `astar_fly`
   - `nearest_for(sim, creature_id, candidates, max_path_len)` → calls `nearest_navgraph` or `nearest_fly`

   These are convenience wrappers so callers don't need to branch on creature
   type. They live alongside the siblings, not replacing them — callers that
   already know the travel mode can call the sibling directly. The unified file
   may depend on sim types (species, creature) as needed for these wrappers.

**Unified result type:** All pathfinding functions return a `PathResult` that
contains VoxelCoords for the path, plus *optionally* NavNodeIds and NavEdgeIds
(populated for nav-graph paths, empty for flight paths). Callers use whichever
fields they find convenient. This replaces the current situation where nav-graph
and flight paths have incompatible result types.

**Max-path-len parameter:** All pathfinding functions take a `max_path_len`
parameter — the maximum number of edges the resulting path may traverse. The
search discards any node whose edge count from the start exceeds this limit.
This is a path-length cutoff, not a work budget — a path of 40 edges is always
found if `max_path_len` is 50, regardless of how many dead ends or cul-de-sacs
the search explores along the way. The caller picks a number comfortably above
the longest path they'd ever want (e.g., manhattan distance in voxels + buffer
for detours), and the search returns None if no path exists within that length.
If there is no obvious max for a given call site, pass `u32::MAX` or similar.

**File structure:** One unified file for all pathfinding business code, one
unified file for all pathfinding tests. The current `pathfinding.rs` and
`flight_pathfinding.rs` merge into the single file.

**Scope:** This is a refactoring of existing code plus adding missing
functionality (e.g., `nearest_fly`). All existing callers of `astar()`,
`astar_filtered()`, `dijkstra_nearest()`, and `astar_fly()` should be migrated
to the new API. The old functions can become private or be removed once all
callers are migrated.

F-interleaved-astar depends on this — the interleaved nearest-among-N algorithm
should be implemented once using the sibling functions, not duplicated for each
travel mode.

**Unblocked:** F-interleaved-astar
**Related:** B-dijkstra-perf, B-dining-perf

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

**Related:** F-animal-husbandry, F-civ-pets, F-genetics

#### F-animal-husbandry — Tamed animal needs and care
**Status:** Todo

Care and maintenance of tamed animals — feeding (supplied food, trough
filling), shelter (stables, pens), health (injury treatment, disease).
Animals have needs that must be met or they become unhappy, unhealthy,
or revert to wild. Elves with Beastcraft skill perform husbandry tasks.

**Unblocked by:** F-wild-grazing
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

Encompasses: war training, mounting and cavalry, animal needs (food,
shelter), breeding, labor assignment panel for controlling which creatures
do which tasks. Pet ownership is an explicit formal bond (like marriage
or parent-child), stored in the formal bonds system (F-formal-bonds).

**Unblocked by:** F-taming
**Related:** F-animal-bonds, F-animal-breeding, F-animal-husbandry, F-cavalry, F-civilizations, F-formal-bonds, F-herding, F-labor-panel, F-pack-animals, F-pet-names, F-task-tags, F-war-animals

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

#### F-creature-sex — Creature sex/gender field
**Status:** Done

Optional sex/gender field on creatures. Initially a simple enum stored
as a trait or direct field on the Creature row. Used by romance and
attraction systems to determine eligible pairs. May also affect sprite
generation and name generation in the future.

**Unblocked:** F-romance
**Related:** F-genetics

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

**Related:** F-apprentice, F-attack-evasion, F-item-quality, F-path-core, F-social-opinions, F-taming

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

#### F-genetics — Creature genetics (additive SNP bitfield genomes with inheritance)
**Status:** Todo · **Refs:** §4

Creature genetics system using additive SNP-based bitfield genomes. Each
creature carries two immutable bitfields: a generic genome (ability scores
as 32-bit weighted-prime-sum SNP regions + Big Five personality as 8-bit
regions) and a species-specific genome (pigmentation via
Value/Saturation/Hue model, morphological categorical traits). Continuous
traits use weighted-sum or bit-count scaled to species mean/stdev
(approximately normal distribution). Categorical traits (hair hue, antler
style) use max-bit-count selection across competing SNP sections with PRNG
tiebreak, plus optional hue blending for adjacent categories. Genome is
immutable after creation; expressed traits stored separately in
creature_traits table and can diverge (dye, injury, aging). Inheritance
via per-bit 50/50 parent selection with small mutation rate.

Storage: Genome { bytes: Vec<u8>, bit_len: u32 } — bitvec crate evaluated
and rejected (unmaintained, broken serde). New creature_genomes tabulosity
table (1:1 with creatures). Serde uses append-only layout with
deterministic SplitMix64 backfill for old saves. New
personality_distributions field on SpeciesData (sibling of
stat_distributions). New SpeciesGenomeConfig on SpeciesData for bit widths
and species-specific SNP layout.

Replaces current quasi_normal stat rolling and BioSeed visual trait
derivation.

Phases: (A) genome infrastructure + bitfield types + serde, (B) wire up
ability scores replacing quasi_normal, (C) Big Five personality axes,
(D) pigmentation VSH model + sprite integration for all 12 species,
(E) inheritance mechanics, (F) species morphology categorical traits.

**Draft:** docs/drafts/genetics.md

**Related:** F-animal-breeding, F-creature-sex

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

#### F-pet-names — Named tamed animals
**Status:** Todo

Tamed animals receive names upon joining the player's civilization.
Name generation could use the Vaelith conlang system (like elf names)
or a separate animal naming scheme. Names appear in the creature info
panel, units panel, and notifications.

**Related:** F-civ-pets, F-taming

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

#### F-skill-check-helper — Unified skill_check helper and game-mechanics doc
**Status:** Done

#### F-slow-eating — Slow eating with interruptible consumption and partial restoration
**Status:** Todo · **Phase:** 4

Eating takes time rather than being instant. Food is consumed gradually
over eat_action_ticks (or a new duration field). If interrupted mid-meal,
food item is destroyed and elf gets partial hunger restoration proportional
to progress. Applies to all eating paths (dining hall, carried food,
foraging). Currently all eating is instant on arrival.

**Related:** F-bldg-dining, F-dinner-party

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
**Status:** Done

Scout-path elves can tame neutral creatures. Toggle a "Tame" button on
a neutral creature's detail panel to create an open task for any
available Scout. Each attempt is a quick action (few seconds) with a
small success chance based on WIL + CHA + Beastcraft skill; the Scout
keeps trying (and gaining Beastcraft XP) until success. Unchecking the
button cancels the task.

Post-taming: creature gets the player's civ_id, default wander behavior,
appears in a Pets/Animals section of the units panel.

**Draft:** docs/drafts/F-taming.md

**Unblocked:** F-civ-pets
**Related:** F-creature-skills, F-pet-names, F-tame-aggro, F-task-tags

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
**Status:** Done

#### B-dine-orphan-task — DineAtHall speculative task leaves orphaned Complete rows
**Status:** Done

The DineAtHall code path speculatively inserts a task before checking if food can be reserved (needed for FK validation on item_stacks.reserved_by). If no food is available, the task is cleaned up via `complete_task()`, which sets it to Complete state rather than removing it. This leaves orphaned Complete DineAtHall tasks in the DB. While not a correctness bug, it creates unnecessary DB clutter. Consider either removing the task on the failure path, or adding a periodic GC for completed tasks with zero progress.

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

#### F-building-civ — Building civilization ownership and civ-filtered building access
**Status:** Todo · **Refs:** §4

Add a `civ_id: Option<CivId>` field to `CompletedStructure` so that every
building is owned by a specific civilization. Use this field to restrict
building access across all building-creature interactions: only creatures
whose `civ_id` matches the building's `civ_id` can use that building.
Buildings with `civ_id: None` are orphaned (unusable by anyone).

**Design decisions:**
- `civ_id: None` means orphaned — no creature can use the building. This
  breaks old saves (pre-existing buildings will have `None` via serde
  default), which is acceptable at this stage of development.
- No cross-civ logistics. Hauling only moves items between buildings of the
  same civ.
- Cross-civ home assignment is rejected (the AssignHome command silently
  fails if creature and home have different civs).
- Greenhouses only produce for their owning civ's logistics pipeline.
- Dances are only organized at and joined at the owning civ's dance halls.

**Current state:** `CompletedStructure` has no civ ownership. The only
civ-awareness is: (a) task-level `required_civ_id` on Craft/Haul tasks
(set to `self.player_civ_id` at creation), which `find_available_task()`
already filters; (b) a `creature.civ_id.is_some()` gate in the dining
heartbeat that prevents wild animals from dining but doesn't distinguish
between rival civs.

**Work needed:**

1. **Data model** — Add `civ_id: Option<CivId>` (indexed, serde default
   `None`) to `CompletedStructure` in `db.rs`. Add `civ_id: Option<CivId>`
   parameter to `CompletedStructure::from_blueprint()`.

2. **Construction** — In `complete_build()` (`construction.rs`), pass the
   player's `civ_id` when creating the `CompletedStructure`. The player's
   civ is available via `self.player_civ_id`.

3. **Dining halls** (`needs.rs:616`) — In `find_nearest_dining_hall()`, add
   a `structure.civ_id == creature_civ_id` filter to the structure iteration
   loop. Remove the existing `creature.civ_id.is_some()` gate in the
   heartbeat (`mod.rs`) since per-structure matching subsumes it.

4. **Dormitory beds** (`needs.rs:554`) — In `find_nearest_bed()`, add
   `structure.civ_id == creature_civ_id` filter to the dormitory iteration.

5. **Assigned home beds** (`needs.rs:508`) — In `find_assigned_home_bed()`,
   after validating the home exists, verify `structure.civ_id ==
   creature_civ_id`. If mismatched, treat as no home (return None).

6. **Home assignment** (`construction.rs:1520`) — In `assign_home()`, after
   validating the structure is a Home, check that `structure.civ_id ==
   creature.civ_id`. If mismatched, return early (reject the assignment).

7. **Logistics heartbeat — building collection** (`logistics.rs:380`) — In
   `process_logistics_heartbeat()`, filter the logistics buildings vec to
   only include buildings matching the player civ. (The heartbeat already
   uses `self.player_civ_id` for task creation, so this is consistent.)

8. **Logistics — fruit availability count** (`logistics.rs:296`) — In the
   fruit availability loop over structures, filter by civ.

9. **Logistics — haul source selection** (`logistics.rs:547, 573`) — In
   `find_source_for_want()` phases 2 and 3 (lower-priority buildings,
   surplus detection), add `structure.civ_id == requester_civ` filter to
   both structure iteration loops. The requester civ comes from the
   destination building's civ_id.

10. **Logistics — personal item sources** (`logistics.rs:656, 703`) — In
    `find_owned_item_source()` and `find_unowned_item_source()`, add
    `structure.civ_id == creature_civ_id` filter to both building iterations.

11. **Greenhouses** (`greenhouse.rs:22`) — In `process_greenhouse_monitor()`,
    filter the greenhouse collection to only include structures whose
    `civ_id` matches a player civ. (Greenhouses owned by no one or a
    non-player civ don't produce.)

12. **Dance halls — debug command** (`activity.rs:103`) — In the debug
    start-dance handler, filter `structure.civ_id` to match the player civ.

13. **Dance halls — volunteer organization** (`activity.rs:1327`) — In
    `try_volunteer_dance_organization()`, add `structure.civ_id ==
    creature_civ_id` filter to the dance hall search loop.

14. **Crafting** (`crafting.rs:474`) — The active recipe scan iterates
    structures by ID. Add a civ filter: only process recipes for structures
    whose `civ_id` matches `self.player_civ_id`. (Task-level
    `required_civ_id` already handles worker filtering, but without this
    check, orphaned buildings would still generate craft tasks.)

15. **Tests:**
    - Creature from civ A cannot dine at civ B's dining hall.
    - Creature from civ A can dine at civ A's dining hall.
    - Wild creature (no civ) excluded from all dining halls.
    - Creature cannot sleep in a different civ's dormitory.
    - Creature cannot be assigned to a different civ's home.
    - Hauling only moves items between same-civ buildings.
    - Orphaned building (`civ_id: None`) is not used by any creature.
    - Greenhouse with non-matching civ does not produce.
    - Dance organization only targets own-civ dance halls.
    - Serde roundtrip for the new `civ_id` field on `CompletedStructure`.

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

**Related:** F-bldg-dining, F-dinner-party, F-item-quality

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

**Blocks:** F-civ-knowledge, F-wild-fruit
**Related:** F-bldg-kitchen, F-civ-knowledge, F-civilizations, F-component-recipes, F-dye-crafting, F-food-chain, F-fruit-extraction, F-fruit-naming, F-fruit-pigments, F-fruit-prod, F-fruit-sprite-ui, F-fruit-sprites, F-fruit-yields, F-greenhouse-revamp, F-logistics-filter, F-recipes, F-textile-crafting, F-wild-fruit

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

**Unblocked by:** F-wild-grazing
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

Uses the social opinion system (F-social-opinions) — an elf's Friendliness
toward their tamed hawk is a row in the same opinion table. A "bonded"
threshold on opinion intensity triggers special bonded-pair behavior.

**Unblocked by:** F-social-opinions
**Related:** F-casual-social, F-civ-pets, F-emotions, F-social-opinions

#### F-apprentice — Skill transfer via proximity
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Elves learn skills by working near skilled elves. Apprenticeship as an
emergent social/economic system.

**Related:** F-creature-skills, F-path-core

#### F-casual-social — Opportunistic passing social interactions
**Status:** Done

Quick, lightweight social interactions that happen opportunistically when
creatures are near each other without interrupting their current activities.
Examples: two elves of the same civ walking past each other might have a
brief chat or flirtation; a Scout might pet a tamed animal in passing.
These micro-interactions upsert social opinions (small Friendliness bumps,
occasional Attraction rolls) and provide a baseline social fabric even
when creatures aren't engaged in formal group activities.

**Scope:** Same-civ creatures only. Cross-civ casual interactions and
animal-petting (F-animal-bonds) are deferred to later work.
Attraction rolls are deferred to F-romance.

**Trigger mechanism: heartbeat-driven**

During each creature's heartbeat, roll an independent random chance
(configurable PPM). On success, scan nearby voxels (Manhattan distance
≤ `casual_social_radius`, default 3 — about 6 meters) for other alive
same-civ creatures. If one or more are found, pick one (deterministic
selection — e.g., nearest, then CreatureId tiebreak) and run a
**bidirectional** interaction: both creatures perform a BestSocial skill
check (`max(Influence, Culture)` + CHA + `quasi_normal(50)`) and the
resulting delta is upserted as Friendliness on the other creature. Both
creatures also attempt skill advancement on their better social skill.

No per-pair cooldown — frequency is controlled entirely by the PPM
chance. Two elves stationed near each other will build a relationship
faster than two who rarely cross paths, which is the desired behavior.

Because both A and B have independent heartbeats, a pair standing
together may trigger from either side. This is intentional and fine
at low PPM values.

The interaction takes **zero sim time** — no pausing, no task
interruption, no action cost. The creature's heartbeat simply includes
a social check alongside its existing need/decay checks. More elaborate
visible interactions (pauses, conversation animations) are deferred to
F-elaborate-social.

**Thoughts**

Each interaction awards a Thought to both creatures:
- Positive net delta (≥ +1): a small positive mood thought
  ("Had a pleasant chat with [Name]").
- Negative net delta (≤ -1): a small negative mood thought
  ("Had an awkward exchange with [Name]").
- Zero delta: no thought.

Thought intensity should be small — this is ambient social warmth/friction,
not a major event.

**Friendship threshold refactoring**

The friendliness_label thresholds (Acquaintance ≥5, Friend ≥15,
Disliked ≤-5, Enemy ≤-15) are currently hardcoded in GDScript
(`creature_info_panel.gd`). As part of this feature:

1. Define a `FriendshipCategory` enum in Rust (Enemy, Disliked, Neutral,
   Acquaintance, Friend — extensible to Close Friend later if needed).
2. Define the threshold values in `SocialConfig` so they're tunable.
3. Add a `friendship_category(intensity) -> FriendshipCategory` function
   in the sim.
4. Expose the thresholds through the sim bridge so GDScript reads them
   from Rust rather than hardcoding its own copy.
5. On each `upsert_opinion` for Friendliness, compare the before/after
   `FriendshipCategory`. If they differ, emit a SimEvent (e.g.,
   `FriendshipChanged { creature_id, target_id, old_category,
   new_category }`). This drives player-visible notifications like
   "Aelindra now considers Thaeron a friend."

**Config fields (SocialConfig)**

- `casual_social_chance_ppm`: probability per heartbeat (e.g., 15,000 = 1.5%)
- `casual_social_radius`: Manhattan distance in voxels (default 3)
- Friendship category thresholds: `friendship_acquaintance_threshold` (5),
  `friendship_friend_threshold` (15), and their negative counterparts
  for Disliked/Enemy.

**Unblocked by:** F-social-opinions
**Unblocked:** F-elaborate-social, F-social-graph
**Related:** F-animal-bonds, F-social-dance, F-social-intensity

#### F-combat-opinions — Respect and fear from combat events
**Status:** Todo

Combat as a source of social opinion changes. Witnessing a creature fight
bravely bumps Respect. Surviving an attack from a creature bumps Fear of
that creature. Fighting alongside someone bumps Friendliness. These use
the social opinion table (F-social-opinions) — no new schema, just new
triggers from combat events.

**Blocks:** F-social-graph
**Unblocked by:** F-social-opinions

#### F-dinner-party — Organized group dining social activity
**Status:** Todo

Organized group dining as a social activity. Unlike routine individual
dining (which only gets incidental casual socialization from proximity),
a dinner party is a coordinated group activity where elves gather, eat
together, and socialize — upserting Friendliness opinions between
participants and providing mood boosts.

**Spontaneous only — no player-directed dinner parties.** An eligible elf
(hungry enough, not on cooldown) rolls a PPM chance during heartbeat.
On success, they become the organizer and create a DinnerParty activity
at a dining hall with sufficient food and free seats. Other elves in the
hunger-eligible range can volunteer to join (Open recruitment mode).

**Timer mechanism (mirrors dance halls).** Per-hall and per-elf cooldowns
prevent constant dinner parties. A newly-furnished hall (one that has
never hosted a completed dinner party) bypasses the hall cooldown so the
player sees a dinner party soon after building the hall. Config fields:
`dinner_party_hall_cooldown_ticks`, `dinner_party_elf_cooldown_ticks`,
`dinner_party_organize_chance_ppm`.

**Revised hunger bands.** To make dinner parties viable, the hunger
thresholds are restructured into a gradient. New config fields control
dinner-party-specific thresholds alongside the existing dining/hunger
fields (which shift downward):

| Food % | Behavior |
|--------|----------|
| 70–100% | Not hungry |
| 60–70% | Willing to **join** an existing dinner party if invited |
| 40–60% | Willing to **organize** a dinner party (also willing to join) |
| 30–40% | **Solo dining** — go eat at a dining hall alone |
| 0–30% | **Emergency** — eat rations/fruit immediately |

New config fields: `food_dinner_party_organize_threshold_pct` (default
60), `food_dinner_party_join_threshold_pct` (default 70). Existing
fields shift: `food_dining_threshold_pct` drops to 40,
`food_hunger_threshold_pct` drops to 30.

**Lifecycle (standard group activity pattern):**

1. *Recruiting* — Organizer selects a dining hall with sufficient food
   (≥ min_count items). Food is reserved from the hall's inventory at
   recruitment time. Open recruitment: idle or low-priority elves in the
   civ whose food level is below the join threshold can volunteer.

2. *Assembling* — Participants get GoTo tasks to assigned seats (using
   the existing TaskVoxelRef / DiningSeat pattern for seat reservations).
   Standard assembly timeout applies — if not enough participants arrive,
   the activity cancels and reserved food is released.

3. *Executing (the dinner)* — Two interleaved concerns:
   - **Eating:** Each participant consumes one reserved food item,
     restoring hunger. A dinner party satisfies the hunger need — an elf
     who attends doesn't need to separately solo-dine.
   - **Socializing:** Over the dinner's duration, each participant makes
     a fixed number of social impression checks (configurable, e.g. 2–3)
     on randomly selected other participants at the table. Uses the
     standard `social_impression` / `upsert_opinion(Friendliness)` pattern
     from casual social. Each check also triggers skill advancement on
     the creature's best social skill (max of Influence, Culture). A
     thought is generated: positive ("Enjoyed dinner with [Name]") or
     negative ("Awkward dinner with [Name]") based on net impression
     delta. An overall mood thought ("Enjoyed a dinner party") is also
     awarded, potentially scaled by average Friendliness toward
     tablemates in the future.
   - Config: `dinner_party_impressions_per_elf` (number of impression
     checks each participant makes), `dinner_party_duration_secs`.

4. *Complete* — Release seat reservations, apply per-elf cooldown,
   clean up activity rows.

**Seat assignment:** Uses the same dining seat locations as conventional
solo dining (existing TaskVoxelRef / DiningSeat pattern). No new
furnishing types needed. Better dining room furnishing with proper
tables and chairs is deferred to a separate tracker item.

**Interaction density:** Each elf makes a fixed number of impression
checks (`dinner_party_impressions_per_elf`) on randomly selected other
participants, regardless of party size. This keeps the cost bounded and
predictable. For a table of 4 with 2 impressions each, that's 8 checks
total — comparable to 8 casual social interactions but concentrated in
one social event.

**Impression strength:** Uses the same `social_impression_delta` function
as casual social for now. Tuning relative strength (e.g., dinner
impressions being stronger than passing-in-the-hallway, or different
effectiveness at different friendship tiers) is deferred to future work
on social intensity scaling.

**Deferred concerns:**
- Food quality affecting mood boost (future, once quality system is
  more mature).
- Preferential invites — organizer biasing toward friends (deferred to
  F-social-prefer as a general group-activity system).
- Better dining room furnishing / table layouts (separate tracker item).
- Impression strength tuning vs casual social (future social balancing).

**Blocks:** F-social-graph
**Unblocked by:** F-group-activity, F-social-opinions
**Related:** F-bldg-dining, F-dining-furnishing, F-food-quality-mood, F-group-chat, F-slow-eating, F-social-dance, F-social-intensity, F-social-prefer

#### F-elaborate-social — Elaborate casual social interactions (visible pauses, variety, personality)
**Status:** Todo

Richer casual social interactions that build on F-casual-social's
behind-the-scenes micro-interactions. Where F-casual-social is invisible
and zero-cost, this feature makes some interactions visible and
meaningful: creatures occasionally pause their current activity for a
longer conversation, exchange gossip, tell stories, argue, or flirt.

Possible scope (loosely specified, to be refined later):
- **Interaction variety:** Different interaction types beyond generic
  "chat" — storytelling (Culture skill), debate (Influence skill),
  gossip (spreads opinions about third parties), commiseration (shared
  negative thoughts create bonding).
- **Visible pauses:** Some interactions cause both creatures to stop
  briefly (a few seconds of sim time). Cosmetic idle animations or
  speech bubbles could accompany these.
- **Cross-civ interactions:** Extend casual social to creatures of
  different civilizations when diplomatic relations permit (e.g.,
  visiting traders, allied elves from other trees).
- **Personality influence:** Extraverted creatures initiate more often
  and with a wider radius; introverted creatures initiate rarely but
  form deeper impressions when they do. Requires F-personality.
- **Context-sensitive interactions:** Creatures working the same task
  or in the same building have different conversation topics than
  those passing on a bridge. Working together could build Respect
  faster than hallway chats.

This is speculative and will be scoped more tightly when F-casual-social
has been in the game long enough to see how it feels.

**Unblocked by:** F-casual-social
**Related:** F-social-intensity

#### F-emotions — Multi-dimensional emotional state
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Emotions as multiple simultaneous dimensions: joy, fulfillment, sorrow,
stress, pain, fear, anxiety. Not a single "happiness" number.

**Blocks:** F-elf-leave, F-hedonic-adapt, F-mana-mood, F-social-graph
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

#### F-formal-bonds — Formal bonds (marriage, parent-child, fiance)
**Status:** Todo

Schema and storage for explicit formal relationships between creatures:
marriage, parent-child, fiance, pet ownership, and similar. Stored in
separate table(s) from the informal opinion system (F-social-opinions).
These are binary facts (married or not), not sliding-scale opinions.
Displayed in the social UI tab alongside informal opinions.

This item covers the table schema and basic CRUD — the behaviors that
create these bonds (courtship, romance, reproduction) are tracked
separately.

**Blocks:** F-romance, F-social-graph
**Related:** F-civ-pets, F-romance, F-social-ui

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

Group chat social activity. Chatting alters social opinions between
participants — upserts Friendliness using max(Influence, Culture) as the
relevant skill check (you're either entertaining with stories or charming
with social maneuvering, whichever you're better at).

**Blocks:** F-social-graph
**Unblocked by:** F-group-activity, F-social-opinions
**Related:** F-dinner-party, F-social-intensity

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
**Related:** F-bldg-concert, F-dance-choreo, F-dance-movespeed, F-dance-scaling, F-dance-self-org, F-music-runtime, F-social-dance

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
**Related:** F-path-civil, F-path-core, F-social-graph, F-social-opinions

#### F-poetry-reading — Social gatherings and poetry readings
**Status:** Todo · **Phase:** 4 · **Refs:** §18, §20

Elves gather for poetry readings, festivals, and social events. Quality of
poetry/music affects mood and mana generation.

**Related:** F-festivals, F-proc-poetry, F-vaelith-expand

#### F-romance — Romantic relationships and courtship
**Status:** Todo

Romantic relationships between creatures. Attraction opinions from the
social opinion system can develop into courtship and flirtation — e.g.,
spending time together, giving gifts, seeking each other out for social
activities. High enough Attraction + Friendliness between two creatures
can lead to formal bonds (fiance, marriage) via F-formal-bonds.

Requires creature sex (F-creature-sex) to be implemented. Courtship
behaviors interact with mood, social activities, and the social
preference system.

**Blocked by:** F-formal-bonds
**Blocks:** F-social-graph
**Unblocked by:** F-creature-sex, F-social-opinions
**Related:** F-formal-bonds

#### F-seasons — Seasonal visual and gameplay effects
**Status:** Todo · **Phase:** 4 · **Refs:** §8, §18

Leaf color changes, snow, seasonal fruit production variation. Gameplay
effects: cold weather increases clothing need, leaf drop reduces canopy
shelter.

**Related:** F-forest-ecology, F-weather

#### F-social-dance — Dance integration with social opinions
**Status:** Todo

Integrate group dance with the social opinions system. Dancing together
upserts Friendliness opinions between dance partners, with intensity
modulated by Culture skill and CHA. Joy gained from dancing is tweaked
upward when dancing with creatures you already like.

**Blocks:** F-social-graph
**Unblocked by:** F-social-opinions
**Related:** F-casual-social, F-dinner-party, F-group-dance, F-social-intensity, F-social-prefer

#### F-social-graph — Relationships and social contagion
**Status:** Todo · **Phase:** 4 · **Refs:** §18

Overarching tracker item for the full social relationship system. The
social graph encompasses interpersonal opinions (F-social-opinions),
opinion sources (F-social-dance, F-casual-social, F-combat-opinions,
F-group-chat, F-dinner-party), social preference in decision-making
(F-social-prefer), UI (F-social-ui), formal bonds (F-formal-bonds),
and romance (F-romance). This item is complete when the constituent
pieces are done and integrated — including emotional contagion (mood
spreading through social connections, weighted by relationship intensity)
which requires F-emotions.

**Blocked by:** F-combat-opinions, F-dinner-party, F-emotions, F-formal-bonds, F-group-chat, F-romance, F-social-dance, F-social-prefer, F-social-ui
**Unblocked by:** F-casual-social, F-social-opinions
**Related:** F-emotions, F-funeral-rites, F-personality

#### F-social-intensity — Context-dependent social impression strength tuning
**Status:** Todo

Tuning the relative strength of social impression sources. Different
social contexts should build relationships at different rates and have
different effectiveness at different friendship tiers. For example,
passing-in-the-hallway (casual social) might be effective at building
acquaintanceship but plateau before reaching friendship, while dinner
parties and dances are effective at deepening existing relationships
toward friendship.

**Why:** Currently all social impression sources use the same
`social_impression_delta` function, which means a hallway chat and a
dinner party have identical social impact per interaction. This feels
wrong — shared experiences like meals and dances should matter more.
But getting the basic mechanics working first is more important than
fine-tuning the numbers.

**Scope:** Per-context impression delta functions or modifiers,
possibly tier-aware scaling (e.g., diminishing returns for casual
social above Acquaintance threshold). Affects casual social
(F-casual-social), dinner parties (F-dinner-party), dance social
(F-social-dance), and any future social interaction sources.

**Related:** F-casual-social, F-dinner-party, F-elaborate-social, F-group-chat, F-social-dance

#### F-social-opinions — Interpersonal opinion table and social skill checks
**Status:** Done · **Phase:** 4 · **Refs:** §18

Asymmetric interpersonal opinion system. Each creature holds a child table
of opinions about other creatures: (creature_id, kind, intensity, other_creature_id).
Creature A's opinion of B can differ from B's opinion of A.

**Kind enum (initial set):** Friendliness, Respect, Fear, Attraction.
Intensity is a signed integer — e.g., negative Friendliness represents a
grudge/dislike. Intensity zero rows can be pruned.

**Skill checks are context-dependent.** The other creature's CHA stat and a
relevant skill modulate the impression they leave — a charismatic creature
with high Culture leaves a stronger positive impression when dancing, while
max(Influence, Culture) applies for casual social interactions (a good
storyteller entertains; a socially savvy creature charms). The specific
skill depends on the activity. A random roll is also part of the formula.

**Pre-game bootstrap:** A new game's starting elves receive several simulated
friendly interactions before game start, so they begin with some existing
relationships and social skill training — emulating that they knew each
other reasonably well before the player arrived.

**Decay:** Periodic sweep moves intensity toward zero by a small amount, so
relationships not reinforced by continued interaction fade naturally.

**Formal relationships** (marriage, parent-child, etc.) are a separate concern
for a future tracker item — this item covers only informal interpersonal
opinions.

**Unblocked:** F-animal-bonds, F-casual-social, F-combat-opinions, F-dinner-party, F-group-chat, F-romance, F-social-dance, F-social-graph, F-social-prefer, F-social-ui
**Related:** F-animal-bonds, F-creature-skills, F-personality

#### F-social-prefer — Social preference in group activity recruitment
**Status:** Todo

Social opportunity scoring in creature decision-making. Elves prefer to
invite creatures they like to group activities (dances, dinner parties,
group chats) rather than activities being open to whoever is free. When
recruiting for a group activity, the organizer biases invitations toward
creatures with higher Friendliness opinions. May also apply to seating
choices, work crew preferences, and other social decisions.

**Blocks:** F-social-graph
**Unblocked by:** F-social-opinions
**Related:** F-dinner-party, F-social-dance

#### F-social-ui — Social tab on creature info panel
**Status:** Todo

Social tab on the creature info panel. Shows other creatures about whom
this creature holds opinions, with human-readable labels derived from
intensity thresholds (e.g., Friendliness above threshold A = "acquaintance",
above threshold B = "friend", negative = "disliked", etc.). In the future,
also displays explicit bonds (marriage, parent-child, fiance) once those
systems exist.

**Blocks:** F-social-graph
**Unblocked by:** F-social-opinions
**Related:** F-formal-bonds

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
**Status:** Done

Idle elves autonomously organize dances at furnished dance halls. This is the first autonomous activity — dances should be both player-created and spontaneous.

**Organizer model:** During idle activation, an elf near a dance hall can decide to organize a dance. The organizer creates the activity (origin: Autonomous, recruitment: Open) and is written as the first participant with role: Organizer. No physical recruitment — other idle elves discover and volunteer via the existing Open recruitment flow.

**Frequency control (all config-driven):**
- Per-hall cooldown: minimum ticks between dances at the same hall.
- Per-elf cooldown: minimum ticks before an elf can organize or join another dance.
- Organize probability: chance per idle activation that an eligible elf near a hall organizes a dance. Should be low enough to not interrupt production, high enough to keep elves happy.

**Venue exclusivity:** Before creating a dance, check ActivityStructureRef for any active activity linked to that hall. If one exists, skip — no two dances on the same hall simultaneously. Applies to both spontaneous and player-created dances.

**First-dance nudge:** Newly-furnished dance halls should trigger their first spontaneous dance quickly (reduced or zero cooldown on fresh halls), so the player gets a visible reward shortly after building one.

**Scope notes:** Recruitment counts (min/desired) currently hardcoded at 3/6 in the debug dance path — may want to make these config-driven or scale with hall size (see F-dance-scaling).

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

#### B-combat-move-stats — Combat movement timing ignores creature stats
**Status:** Done

Combat ground movement timing ignores creature stats (agility/strength).

In `combat.rs`, the edge traversal delay after pathfinding uses
`CreatureMoveSpeeds::new(species_data, 0, 0).tpv_for_edge(edge.edge_type)`,
hardcoding agility=0 and strength=0. This means the actual movement speed
on each edge uses base species values, ignoring the creature's stats.

Meanwhile, `find_path` (which computed the path) uses stat-modified speeds
via `CreatureMoveSpeeds::new(species_data, agility, strength)`. So the path
is optimized for the creature's real speed (e.g., a fast climber might prefer
a shorter climb route), but then the creature traverses each edge at base
speed. This mismatch means path selection and movement timing are inconsistent.

The issue affects two call sites in `combat.rs`:
1. `execute_attack_move` (~line 792): ground movement toward attack-move destination
2. `walk_toward_attack_target` (~line 1168): ground movement toward attack target

Both have the same pattern:
```rust
let tpv = crate::stats::CreatureMoveSpeeds::new(species_data, 0, 0)
    .tpv_for_edge(edge.edge_type);
```


// ... later ...
For reference, `movement.rs` `walk_toward_task` does it correctly:
The fix: look up the creature's agility and strength stats (via `self.trait_int`)
```rust
and pass them to `CreatureMoveSpeeds::new`, matching what `find_path` does
internally and what `walk_toward_task` in `movement.rs` does (which correctly
let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
let speeds = crate::stats::CreatureMoveSpeeds::new(species_data, agility, strength);
let strength = self.trait_int(creature_id, TraitKind::Strength, 0);
let tpv = speeds.tpv_for_edge(edge.edge_type);
uses stat-modified speeds for both pathfinding and traversal timing).
```

This is a pre-existing issue (combat always used raw species speeds), but the
F-unified-pathing refactor made it more visible by introducing `find_path`
which consistently uses stat-modified speeds.

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

#### F-unified-pursuit — Unify pursue_closest_target ground/flight paths via find_nearest
**Status:** Todo

`pursue_closest_target` in `combat.rs` has separate code paths for ground and
flying creatures that should be unified using `find_nearest()`.

**Current behavior:**

- **Ground creatures:** Build a list of NavNodeIds near each target (using
  `find_nearest_node` or `find_melee_reachable_node` for flying targets),
  convert to VoxelCoords, call `find_nearest` to pick the closest, then
  `find_path` to path there. This involves a wasteful NavNodeId → VoxelCoord
  → NavNodeId round-trip through `find_nearest`.

- **Flying creatures:** Use squared Euclidean distance to pick the nearest
  target (no actual pathfinding), then call `fly_toward_target` to close
  distance. This is a heuristic approximation that ignores obstacles.

**Desired behavior:**

Unify both paths by using `find_nearest()` with a set of VoxelCoords that
are locations in melee range of the various target creatures. For each target,
compute the set of positions from which a melee attack could reach the target
(using melee range and footprint geometry — similar to what
`find_melee_reachable_node` already does for ground vs flying targets). Collect
all such positions into a single candidate list and call `find_nearest()`.

The result: if there is any reachable path to a location from which the
attacker could make a melee attack at any target, the attacker walks/flies
toward the closest such location. This works uniformly for ground and flying
attackers, eliminates the Euclidean approximation for flyers, and removes
the NavNodeId round-trip for ground creatures.

**Affected code:** `pursue_closest_target` in `sim/combat.rs` (~line 3192),
including the ground-specific `find_melee_reachable_node` helper and the
flying-specific Euclidean distance branch.

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

**Related:** F-herbalism, F-insect-husbandry, F-lesser-trees, F-seasons, F-wild-bushes, F-wild-grazing

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

#### F-grass-rendering — Advanced grass and terrain surface rendering
**Status:** Todo

Upgrade grass/dirt visual distinction beyond the basic vertex-color
approach from F-wild-grazing. The initial implementation uses a distinct
material for grassless dirt (brown vs green vertex color), which is
functional but bland. This feature explores richer options — shader-based
grass blades, per-chunk grass textures, smooth blending at grass/dirt
boundaries, seasonal variation, etc. Needs investigation into what works
well with the existing mesh pipeline (decimation, chunk boundaries,
smooth normals).

**Related:** F-wild-grazing

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

#### F-wild-bushes — Wild fruit bushes at ground level
**Status:** Todo

Ground-level fruit-bearing bushes and shrubs placed during worldgen.
Provides a food source accessible to non-climbing herbivores. Bush
species tie into the procedural fruit variety system — each bush type
produces a specific fruit species. Bushes occupy voxel space, may
affect pathfinding, and regrow fruit over time. Foundation for wild
fruit and animal foraging.

**Blocks:** F-wild-fruit
**Related:** F-forest-ecology

#### F-wild-foraging — Wild animal foraging for fruit
**Status:** Todo

Wild herbivorous animals autonomously seek out and consume wild fruit.
Animals search within a species-specific foraging radius for available
fruit sources (bushes, fallen fruit). Different species have different
fruit preferences. Split out from F-wild-grazing to separate fruit
foraging from grass grazing — grazing covers ground vegetation, this
covers fruit.

When implemented, monkey and squirrel should transition from grazing
(their interim food source from F-wild-grazing) to fruit foraging as
their primary food source. They are arboreal species that naturally
feed on fruit rather than grass. Their grazer flag should be removed
and replaced with forager behavior.

**Blocked by:** F-wild-fruit
**Related:** F-wild-grazing

#### F-wild-fruit — Wild fruit growing on bushes and ground-level plants
**Status:** Todo

Wild fruit that grows on ground-level bushes and can be found on the
forest floor. Unlike greenhouse-cultivated fruit, wild fruit spawns
naturally during worldgen and regrows seasonally. Wild-only fruit
species (from F-fruit-variety) appear here. Elves can forage wild
fruit manually; animals forage it autonomously (see F-wild-foraging).

**Blocked by:** F-fruit-variety, F-wild-bushes
**Blocks:** F-wild-foraging
**Related:** F-fruit-variety

#### F-wild-grazing — Wild animal herbivorous food cycle
**Status:** Done

Wild herbivorous animals graze on grass instead of starving. Fixes the
current problem where ground-only species (capybara, boar, etc.) can't
reach tree fruit and starve to death. Foundation for domesticated animal
feeding. Fruit foraging is handled separately by F-wild-foraging.

**Design decisions:**

**Grass representation — "track the exceptions."** By default, any
exposed dirt voxel is considered grassy. Instead of storing grass
presence (which would be nearly every terrain surface), we store a
per-chunk set of dirt voxel coordinates that are NOT grassy. Most
chunks will have an empty set. This avoids bloating the RLE voxel
storage or adding a parallel 2D grid. The data structure is a
`BTreeSet<VoxelCoord>` (or sorted `Vec<VoxelCoord>`) per chunk —
deterministic iteration, tiny footprint, usually empty.

**Coordinates include Y.** Grazeable surfaces aren't limited to
terrain-level dirt — future features (sky farms, elevated platforms)
could place dirt at any height. The grassless set stores full 3D
coords, not just (x, z).

**Grassless transitions.** Dirt becomes grassless when: (1) a creature
grazes on it, or (2) a voxel change freshly exposes a dirt surface
(digging, construction). The dirty_voxels drain already fires on voxel
changes, so exposed-dirt detection can piggyback on that pipeline.

**Regrowth.** A periodic sweep (global timer, every ~10k ticks)
iterates each chunk's grassless set. Each entry has a ~10% chance of
regrowing per sweep (removed from the set). Since most chunks have
empty sets, the sweep is cheap. Tuning values are intentionally fast
for visual feedback during development; will slow down for real
gameplay later. Regrowth chance and interval configurable via
GameConfig.

**Grazing behavior.** Herbivore-only — elves cannot graze. All
herbivore species graze initially (capybara, boar, deer, elephant,
monkey, squirrel). Monkey and squirrel are interim grazers — they
will transition to fruit foraging as their primary food source when
F-wild-foraging is implemented. Hungry herbivores get an autonomous
"Graze" task targeting a nearby grassy dirt surface. The creature walks to the target, grazes in a single
action (~3000 ticks), restores food, and the grazed dirt voxel (2m×2m)
enters the chunk's grassless set. Search walks Dijkstra outward from
the creature's position on the species nav graph; for each nav node,
check if adjacent surface dirt is grassy (not in the grassless set).
First hit wins — nearly always immediate since most ground is grassy.

**Food restoration.** Each graze action restores a per-species config
value of food — grazing is frequent, low-yield feeding. Different
species also have different food_max and food_decay_per_tick values to
reflect dramatically different metabolisms (an elephant needs far more
grazing than a squirrel). Specific per-species values TBD during
implementation.

**"Exposed dirt" definition.** A dirt voxel with air (or any non-solid
voxel) above it. Dirt under platforms, leaf canopy, or bridges is still
grazeable — grass grows in shade. All dirt surfaces start grassy at
worldgen.

**No partial grazing.** The single-action design avoids needing to
handle interruptions, partially-eaten patches, or multi-tick progress
tracking. A graze either completes or doesn't happen.

**Overgrazing.** If all nearby grass is depleted, herbivores just
wander and eventually die. No fallback to tree fruit — that behavior
belongs to F-wild-foraging. This creates natural population pressure.

**Rendering.** Grassless dirt gets a distinct material in mesh gen
(brown/earth vertex color instead of the default ground color). This
works with decimation since the material boundary prevents merging
across grass/grassless faces. Simple but functional — more interesting
visual treatments (shader grass, texture blending) deferred to later.

**Unblocked:** F-animal-husbandry, F-herding
**Related:** F-forest-ecology, F-grass-rendering, F-wild-foraging

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

#### B-ghost-chunks — Ghost chunks in distance remain visible after they should be hidden
**Status:** In Progress

Chunks in the distance sometimes remain visible ("ghost" chunks) after they should have had their visibility turned off. This has been attempted to be fixed before but is difficult due to the asynchronous nature of chunk mesh generation. A possible approach: generate the list of chunks that should be visible or shadowed fresh each frame, rather than trying to incrementally track visibility state.

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
**Status:** Done

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
- start_paused: bool (default false) — pause sim immediately on game start

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
**Status:** Done

Depth-based fog that fades distant geometry toward a sky/haze color. Can use Godot's built-in Environment fog or a simple shader-based depth fade. Hides LOD transitions (relevant for F-megachunk draw distance), gives depth cues, makes the forest feel large. Essentially free — per-fragment lerp based on depth.

**Related:** F-day-night-color, F-megachunk, F-mesh-lod

#### F-edge-outline — Edge highlighting shader (depth/normal discontinuity)
**Status:** Todo

Screen-space post-process shader that darkens edges at depth/normal discontinuities, giving the world a readable, slightly stylized look (similar to cel-shading outlines).

**How it works:** A full-screen pass runs a Sobel filter (3×3 convolution kernel) over the depth and normal buffers. Where depth changes sharply between neighboring pixels → silhouette edge (object against sky, platform edge against distant trunk). Where normals change sharply → crease edge (corner of a platform, ridge where two surfaces meet). Those pixels are darkened in the final image.

**Why it's a good fit:** Works entirely in screen space — no extra geometry, no mesh topology awareness. Disconnected triangles (our standard mesh output) are a non-issue since the filter only looks at per-pixel depth/normal values, not mesh adjacency. Coplanar adjacent triangles correctly produce no internal outlines; edges against different-depth backgrounds correctly produce outlines. Scales with screen resolution, not world complexity. One full-screen pass, minimal cost.

**Godot implementation path:** Full-screen ShaderMaterial on a ColorRect reading DEPTH_TEXTURE and NORMAL_ROUGHNESS_TEXTURE, or a CompositorEffect. Both are available in Godot 4 screen-space shaders.

**Tuning considerations:** Sensitivity thresholds need tuning — too sensitive produces noisy outlines on subtle normal variations (e.g., mesh decimation artifacts), too insensitive loses desired creases. Distance fog interaction matters: running the edge pass before fog lets outlines get fogged naturally (preferred); running after fog darkens already-fogged areas.

#### F-edge-scroll — Configurable edge scrolling (pan, rotate, or off)
**Status:** Done · **Phase:** 5

Moving the mouse to screen edges moves the camera. Three configurable
modes:
- Pan: edges scroll the camera horizontally (classic RTS, default)
- Rotate: edges rotate/tilt the camera
- Off: disabled

Edge scrolling auto-disables when the mouse is over a UI panel or when
the window loses focus, to prevent accidental camera movement.

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

#### F-fov-slider — FOV slider in settings
**Status:** Todo

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
**Status:** Done

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
**Status:** Done · **Phase:** 5

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

#### F-ssao — Screen-space ambient occlusion toggle
**Status:** In Progress

Experimental: expose Godot's built-in SSAO as a user-facing toggle in the settings panel. Adds a checkbox to the Visual section (like the existing fog toggle). Settings persisted in config.json, applied to the WorldEnvironment's Environment resource at runtime (same pattern as fog_controller.gd). Supplements any future baked per-vertex AO (F-voxel-ao) by catching medium-scale occlusion (room interiors, canopy undersides) and darkening dynamic objects (creatures, items) that have no baked AO. May be removed if the visual benefit doesn't justify the per-frame GPU cost or if it conflicts with the art style.

**WIP branch:** `feature/F-ssao` — toggle, config, controller, and tests are implemented. Initial testing found the effect is subtle and only visible on some creases, not all. Key findings so far: `ssao_light_affect` must be >0 for the effect to show in directly-lit areas (default 0.0 only affects ambient light, which is low in our scene). A small `ssao_radius` (~0.5 world units) works better than large values for catching chamfer-scale crevices. Next step: manually tweak SSAO parameters (`ssao_detail`, `ssao_power`, `ssao_horizon`, `ssao_intensity`) in the Godot editor's Environment inspector to find values that look good before committing final defaults.

**Related:** F-voxel-ao

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

Per-pixel ambient occlusion baked into chunk meshes at generation time. The shader multiplies AO into the final color to darken corners, crevices, undersides of branches, and interior spaces. High visual impact for minimal computational cost.

**Why not per-vertex AO:** The original plan assumed flat-shaded cubic geometry (the classic 0-1-2-3 corner AO algorithm). That no longer applies — F-visual-smooth has landed and meshes go through subdivision, chamfer, optional curvature smoothing, and QEM decimation. After decimation, triangles can be long, thin, and irregularly shaped, so per-vertex AO values would produce stretched interpolation artifacts across large triangles. The AO solution needs to be per-pixel, not per-vertex.

**Candidate approaches (no decision yet):**

- **3D volume texture AO.** Bake AO values into a 3D texture (one texel per voxel or sub-voxel), sample per-pixel in the fragment shader using world-space position (already available for triplanar noise). Godot 4 supports `ImageTexture3D` / `sampler3D` with hardware trilinear filtering. Concern: 1 texel per voxel (2m) is likely too coarse, but higher resolution grows cubically — 2x is 8x the data, 4x is 64x.
- **GPU voxel occupancy sampling.** Upload raw voxel occupancy as a 3D texture. The fragment shader counts occupied neighbors per-pixel at runtime — no CPU bake step, always current with voxel changes. Tradeoff is 26+ texture samples per fragment every frame.
- **SDF-based AO.** Bake a signed distance field into a 3D texture. Shader samples the SDF at a few points along the normal to estimate occlusion. Smooth results with ~5 texture reads per fragment. SDF is reusable for other effects (soft shadows, subsurface). More complex to bake (jump flooding algorithm). Same cubic resolution scaling concern as 3D volume texture.
- **Voxel-grid raymarching (CPU bake).** Cast short DDA rays from each vertex/texel into the voxel neighborhood during async mesh generation. Works with arbitrary positions. Could feed into any of the above storage formats.

The async mesh generation pipeline (rayon workers with ChunkNeighborhood snapshots) provides a natural place to bake AO — the voxel data is already available on the worker thread.

**Related:** F-mesh-par, F-ssao, F-visual-smooth

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
**Status:** Done

ActivityConfig::assembly_timeout_ticks is defined (default 300,000) but
never checked. An activity stuck in Assembling phase (e.g., participants
died or got lost en route) will wait indefinitely. Should check elapsed
time in the Assembling branch of the activation loop and cancel the
activity if the timeout has been exceeded, similar to how
check_activity_pause_timeout handles the Paused phase.

**Related:** F-group-activity

#### B-chamfer-nonmfld — Chamfer produces non-manifold edges for diagonally-adjacent voxels
**Status:** Done

The chamfer pass in `smooth_mesh.rs` produces non-manifold edges (edges shared by 3+ triangles) when two voxels are diagonally adjacent — sharing only an edge, not a face. This is a distinct issue from the QEM deformation bug (B-qem-deformation).

**How it happens:** When two voxels share only an edge (e.g., voxels at (x,y,z) and (x+1,y,z+1) with neither (x+1,y,z) nor (x,y,z+1) present), each voxel generates exposed faces on its sides. The chamfer subdivides these faces and deduplicates vertices by position. Along the shared edge, both voxels' face vertices merge, creating edges shared by triangles from two different faces on two different planes — a non-manifold configuration.

**Where it occurs in terrain:** Any heightmap where adjacent columns create a "checkerboard" height pattern triggers this. For example, a 2×2 area where h(0,0)=2, h(1,0)=1, h(0,1)=1, h(1,1)=2: the y=1 voxels at (0,0) and (1,1) are diagonally adjacent with no face-adjacent neighbor at the same height. This is common in natural-looking terrain with scattered single-step height variations.

**Evidence from testing:** In B-qem-deformation fuzz testing, 49/50 random heightmap seeds (height range 1–4) and 99/100 smooth heightmap seeds (max ±1 step) produced non-manifold chamfer output before a diagonal-gap-filling workaround was added to the test heightmap generator.

**Impact:** Non-manifold edges cause rendering artifacts (z-fighting, self-intersection, overlapping geometry) that could look like "bumps, dents, or misshapen surfaces." The QEM decimation pass has a non-manifold guard (`collapse_would_create_non_manifold`) that prevents creating NEW non-manifold edges, but cannot fix ones inherited from the chamfer input.

**Possible fix approaches:**
1. **Gap-filling in terrain gen:** Ensure no diagonal-only voxel adjacency exists by filling gaps (raise one neighbor column to eliminate the checkerboard pattern). The B-qem-deformation test suite uses this approach in `smooth_random_heightmap`.
2. **Chamfer-level fix:** Detect diagonal-only adjacency during face generation and either skip the conflicting faces or merge them into a single surface.
3. **Post-chamfer cleanup:** Add a non-manifold edge resolution pass before decimation.

**Chosen fix — vertex splitting (post-subdivision, pre-chamfer):**

After face subdivision (which creates 8 triangles per voxel face and deduplicates vertices by position) but before anchoring and chamfering, run a two-pass non-manifold resolution:

**Pass 1 — Split non-manifold voxel edge midpoints:** Build an edge→triangle adjacency map. Find edges shared by 3+ voxel faces (non-manifold edges). These occur along voxel edges where diagonally-adjacent voxels' faces were merged by position dedup. The non-manifold edges run through the *midpoint* vertices that were created by face subdivision — NOT the voxel corner vertices. For each non-manifold edge, partition its incident triangles into connected groups (connected through other, non-NM edges), then duplicate only the edge midpoint vertex for each additional group, rewriting that group's triangles to use the new vertex index. The corner vertices remain shared in this pass.

**Pass 2 — Split non-manifold voxel vertices:** After edge-midpoint splitting, scan for remaining non-manifold vertices — vertices where the incident triangles don't form a single connected fan (connected through shared edges). This catches cases not addressed by pass 1: (a) voxels sharing only a corner point with no shared edge, and (b) corner vertices that become non-manifold due to surrounding geometry (e.g., the bottom corner of a split diagonal edge that sits flush on a flat ground surface, creating two disconnected fans on either side). For each such vertex, partition incident triangles into connected fan components and duplicate the vertex per additional component.

No positional offset is needed for duplicated vertices — the pipeline operates on topology (vertex indices) downstream, so same-position vertices with different indices stay separate through chamfer, smoothing, and decimation.

**Related:** B-qem-deformation, F-mesh-lod

#### B-dead-enums — Remove dead GrownStairs/Bridge code and add explicit enum discriminants
**Status:** Done

GrownStairs, Bridge (VoxelType), and Stairs, Bridge (BuildType) exist in code but there is no way to produce them in-game. They are not planned for implementation in the foreseeable future. Remove all code referencing these variants — enum definitions, match arms, trait impls, tests, and any other references.

Additionally, add explicit integer discriminants to all serializable enums (at minimum VoxelType and BuildType) so that inserting or removing variants in the future does not silently change the serialized representation of existing variants.

#### B-dead-max-gen — Remove vestigial max_gen_per_frame field and ~40 test calls
**Status:** Done

#### B-leaf-diagonal — Leaf blobs sometimes only diagonally connected, looks bad
**Status:** Done

Leaf blobs generated during tree growth sometimes end up only diagonally connected to other geometry (other leaf blobs, branches, trunk). This looks bad after chamfering/smoothing because diagonal-only voxels produce visible gaps. Branches already have logic to ensure face-to-face (6-connected) adjacency with at least one other solid voxel. Leaf blob placement needs the same treatment: every leaf voxel must be face-adjacent to at least one other leaf or solid voxel.

#### B-quit-crash — Crash on quit from in-flight rayon mesh workers
**Status:** Done

When quitting the game, Godot may tear down while rayon worker threads are still executing chunk mesh generation tasks. The current `shutdown()` in `sim_bridge.rs` drops `MeshCache` (which drops the rayon `ThreadPool`), but rayon's `ThreadPool::drop` attempts to execute all remaining pending tasks before terminating — there is no built-in cancel API. This can cause crashes if Godot deallocates the process or its resources while workers are still alive.

**Observed:** Occasional crash on game quit.

**Root cause:** No cancellation mechanism for in-flight mesh generation tasks. Rayon issue #544 confirms there is no native "clear pending tasks" API.

**Proposed fix:**
1. Add an `Arc<AtomicBool>` cancellation flag to `MeshCache`, cloned into each spawned closure.
2. Workers check the flag at the top of the spawn closure (before `generate_chunk_mesh`) and bail early if set.
3. In `shutdown()`, set the cancel flag, then drain the mpsc channel until all `in_flight` tasks are accounted for (each bailed worker still sends a result or a cancellation sentinel). This ensures all workers have exited before the pool is dropped.
4. Consider whether the global rayon pool needs similar treatment — it's used for synchronous data-parallel ops (nav building, tree gen, world gen) so it's less likely to be in-flight during quit, but worth auditing.

**References:** rayon issues #544 (no cancel API), #688 (no sync shutdown), #776 (hang on drop).

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
**Status:** Done · **Phase:** 5

Replace event-driven creature activation with poll-based activation using indexed DB queries.

**Current problem:** Every code path in the activation cascade must explicitly call `event_queue.schedule()` or the creature goes permanently inert. Forgotten reactivations are a persistent bug class. Additionally, `cancel_creature_activations()` does an O(n) heap rebuild on action abort.

**Solution:** Remove `CreatureActivation` from the event queue entirely. The main `step()` loop queries for creatures that need activation using the existing `next_available_tick` field on the Creature table.

**Core mechanics:**
- A creature is "available for activation" when `vital_status == Alive` and `next_available_tick <= current_tick`.
- `next_available_tick` is always set to a concrete tick, never `None` in steady state. Set to `tick + 1` at spawn. When an action starts, set to `tick + duration`. When an action completes and the creature re-enters the decision cascade, set to `tick + 1` (replacing `schedule_reactivation`). This avoids same-tick activation order dependencies.
- Compound tabulosity index on `(vital_status, next_available_tick)` enables efficient range scan: all `(Alive, None..=Some(tick))` entries. `None` sorts before all `Some` values in Rust's `Option` ordering, so the scan covers both `None` (newly created creatures not yet activated) and `Some(t)` where `t <= tick` in a single contiguous index sweep. Dead creatures (tens of thousands in long games) are skipped at the index level, not post-filtered.
- Creatures processed in `CreatureId` order for deterministic tiebreaking within a tick.
- The main loop asks the index for `min(next_available_tick)` among living creatures to compute the next tick to advance to, preserving the "empty ticks are free" property.

**What this eliminates:**
- All `schedule_reactivation()` calls → replaced by setting `next_available_tick = tick + 1`.
- All `event_queue.schedule(CreatureActivation)` calls in `start_simple_action`, `ground_move_one_step`, `start_build_action`, etc. → action start already sets `next_available_tick`; no separate event needed.
- `cancel_creature_activations()` and the O(n) heap filter in `abort_current_action` → just clear/reset `next_available_tick` on the creature row.
- The "forgotten reactivation" bug class entirely — any alive creature with `next_available_tick <= tick` is found automatically.

**Fixes included:**
- **Ranged cooldown asymmetry:** Resolved by design — creature stays in `ActionKind::Shoot` during cooldown with `next_available_tick` set to cooldown end, preventing unnecessary walking. No explicit code change needed.

**Non-goals:** Other event types (`CreatureHeartbeat`, `TreeHeartbeat`, `LogisticsHeartbeat`, `ProjectileTick`, `GrassRegrowth`) remain in the event queue for now. See F-retire-events for extending poll-based activation to those systems.

**Unblocked:** F-retire-events
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

**Related:** F-activation-revamp, F-retire-events, F-sim-speed

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

**Unblocked:** F-choir-build, F-dinner-party, F-group-chat, F-group-dance
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

#### F-mesh-perf-2 — Mesh pipeline performance optimization, round 2
**Status:** Todo

Continuation of F-mesh-pipeline-perf. The initial round achieved ~1.8x speedup on the heaviest fixtures (worldgen_trunk: ~58ms to ~33ms at release profile) through six optimization attempts. This item continues the iterative agent-driven optimization process.

**Prior work:** See `docs/iterative_optimization.md` for the full guide on the optimization process, agent invocation patterns, and lessons learned. The round-1 diary is at `docs/optimization-diaries/mesh-pipeline-perf.md`. Start a new diary at `.tmp/mesh-perf-diary.md` for round 2 (copy the round-1 diary as a starting point, or start fresh with new baselines). Snapshot regression test at `elven_canopy_sim/tests/mesh_snapshots.rs`, criterion benchmarks at `elven_canopy_sim/benches/mesh_pipeline.rs`.

**Branch:** Start from `feature/F-mesh-pipeline-perf` (or main after merge). The harness infrastructure (snapshot tests, criterion benchmarks, exposed sub-stage functions) is already in place.

**Setup:** Run `cargo test -p elven_canopy_sim --test mesh_snapshots` once to generate fixtures in `.tmp/mesh_fixtures/`. Then `cargo bench -p elven_canopy_sim -- "default/chunk/(worldgen_trunk|worldgen_surface|fully_solid|flat_slab)"` for baseline numbers. Record baselines in `.tmp/mesh-perf-diary.md` before starting optimization agents.

**Required: new fixture scenarios.** Add two new world-generated fixtures to the snapshot test and benchmarks:
- **Sky chunk**: A chunk coordinate high above the canopy (e.g., cy=3 in a 64-tall world). The chunk itself should be mostly/entirely air, but the `ChunkNeighborhood` extraction should capture a full border region that may include treetop leaves. Tests the early-exit and sparse-chunk paths.
- **Underground chunk**: A chunk coordinate well below ground level (e.g., cy=0 below floor_y in a world with terrain). The chunk should be fully solid or nearly so. Tests the all-faces-culled path and column iteration overhead on dense chunks.
These exercise the "boring chunk" fast paths that real gameplay produces many of (most chunks in a large world are empty sky or solid underground).

**Optimization targets to try (non-exhaustive):**

1. **Chamfer inner loop**: `compute_chamfer_offset` in `smooth_mesh.rs` iterates all neighbors to find anchored ones and compute centroid. Pre-computing an anchored-neighbor index list during `apply_anchoring` would avoid repeated filtering in the chamfer loop.

2. **Non-manifold resolution**: `resolve_non_manifold` pass1/pass2 in `smooth_mesh.rs` does vertex splitting with Vec cloning, index remapping, and union-find. Look for redundant allocation or O(n^2) patterns in the splitting logic.

3. **QEM Quadric operations**: `Quadric::evaluate` and `Quadric::add` in `mesh_decimation.rs` operate on 10-element symmetric matrices. SIMD intrinsics or restructured memory layout (SoA vs AoS) could help in the priority queue hot loop.

4. **Coplanar region retri flood-fill**: The retri pass in `mesh_decimation.rs` does flood-fill to find coplanar same-material regions, then re-triangulates. The flood-fill visited tracking and region boundary extraction may have redundant work.

5. **Column span iteration in face gen**: `build_smooth_mesh` in `mesh_gen.rs` iterates `column_spans()` for every (x, z) column in the chunk+border. For chunks that are mostly air (sky) or mostly solid (underground), early-exit or skip-ahead on empty/full columns could help.

6. **SmoothMesh struct-of-arrays**: The per-triangle parallel arrays (triangle_tags, triangle_colors, triangle_voxel_pos, triangle_face_normals, triangle_face_midpoints) are separate Vecs. Merging into a single Vec of a per-triangle struct could improve cache locality during iteration.

7. **Inlining hints**: The face gen inner loop calls `get_or_create_vertex`, `add_subdivided_face`, and `add_edge` per face. If the compiler is not inlining these, `#[inline]` hints could eliminate call overhead.

8. **Reduce neighbor list allocation**: `SmoothVertex.neighbors` is a `Vec<u32>`. Since most vertices have 4-12 neighbors, a `SmallVec<[u32; 8]>` or `SmallVec<[u32; 12]>` could eliminate heap allocation for the common case.

**Process:** Follow the agent loop in `docs/iterative_optimization.md`. Key points: use the same branch (no worktrees), target the default pipeline config (smoothing OFF, decimation ON), benchmark with `default/chunk/...`, record every attempt in the diary.

**Related:** F-mesh-pipeline-perf

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

#### F-retire-events — Retire event queue: poll-based heartbeats and periodic systems
**Status:** Todo · **Phase:** 5

After F-activation-revamp removes `CreatureActivation` from the event queue, revisit the remaining event types (`CreatureHeartbeat`, `TreeHeartbeat`, `LogisticsHeartbeat`, `ProjectileTick`, `GrassRegrowth`) and retire the event queue entirely.

Two candidate approaches:
- **Polling:** Same pattern as F-activation-revamp — each system has an indexed tick field, main loop queries for ready entries.
- **Fixed-interval ticks:** Periodic systems fire at deterministic intervals (e.g., creature heartbeats at every tick divisible by N, logistics every M ticks). No per-entity scheduling or polling — the main loop checks `tick % interval == 0` and processes all relevant entities in ID order.

The fixed-interval approach may be simpler and cheaper for systems that already run on regular cadences. Design decision deferred until F-activation-revamp is complete and we have experience with the poll model in practice.

**Unblocked by:** F-activation-revamp
**Related:** F-event-loop

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
**Design:** `docs/drafts/tabulosity_design_reference.md` (old drafts: `docs/drafts/sim_db_v9.md`, `docs/drafts/tabulosity_advanced_indexes_v5.md`)
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

#### B-unsafe-db-calls — Replace _no_fk and modify_unchecked calls with safe database-level methods
**Status:** Done

All production code converted (0 `_no_fk`/`modify_unchecked` calls remain in non-test code). 464 calls remain in `tests.rs` (54k lines). The safe API conversion also uncovered and fixed real bugs: blueprint/task FK ordering in `designate_build` and `cancel_build`, and task insertion ordering for item reservations in logistics/activation/crafting/mod. **Next step:** split `tests.rs` (see F-split-sim-tests), then convert test code to safe API in manageable chunks.

**Unblocked by:** F-split-sim-tests

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
**Design:** `docs/drafts/tabulosity_design_reference.md` (old draft: `docs/drafts/tabulosity_advanced_indexes_v5.md`)

`BTreeSet<(F1, F2, ..., PK)>` compound indexes supporting prefix queries
(e.g., query by first field, or first two fields). Unified `#[index(...)]`
attribute with `IntoQuery` trait for ergonomic queries. Uses tracked bounds
(runtime min/max) instead of `Bounded` trait, enabling `String` PKs and
index fields. High complexity due to derive macro codegen for arbitrary
field tuples.

**Related:** F-sim-db-impl, F-tab-filter-idx

#### F-tab-filter-idx — Filtered/partial indexes
**Status:** Done
**Design:** `docs/drafts/tabulosity_design_reference.md` (old draft: `docs/drafts/tabulosity_advanced_indexes_v5.md`)

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
with a release profile. Add a `scripts/build.py relay` (or similar)
target that produces an optimized, stripped binary suitable for
deployment on a dedicated server. Include any necessary Cargo profile
tuning (LTO, codegen-units=1, strip=true) for minimal binary size and
maximum performance.

**Related:** F-multiplayer, F-relay-multi-game

### Testing Infrastructure

#### B-fragile-tests — Audit and harden tests against PRNG stream shifts and worldgen changes
**Status:** In Progress

Harden sim tests so they don't break when worldgen or PRNG changes.

**The problem:** Many tests use `test_sim(seed)` and inherit a full
worldgen result — tree shape, creature stats, nav graph, fruit
positions. They then implicitly depend on specific details of that
output: clear air at certain coordinates, specific civ relationships,
fruit on leaves, particular stat rolls. When anything upstream changes
(new PRNG calls, config tweaks, algorithm changes), tests break even
though the feature under test is fine. The seed number doesn't matter
— the fragility is that tests are coupled to worldgen output they
didn't ask for.

**General approach:** Each test should be examined for what it actually
needs from the world and given only that. Many tests don't need a tree
at all — they need a flat open world with clear air and solid ground,
like a fighting game training stage. Projectile tests need clear LOS
between two points. Melee tests need two creatures on adjacent
walkable tiles. Tests that do need a tree (construction, fruit, nav
graph connectivity) should create or find what they need explicitly
rather than relying on the specific shape a particular seed produces.
The fix is case-by-case.

**Historical incidents:**

- *F-attack-evasion (2026-03-24):* Adding evasion hit-checks (12 extra
  PRNG calls per attack) broke 28 combat tests. Fix: `zero_creature_stats`
  + `force_guaranteed_hits`.
- *quasi-normal-util (2026-03-24):* Changing the quasi-normal sampling
  range shifted creature stat generation, breaking 14 more combat tests.
  Same fix.
- *leaf density (earlier):* Fruit tests broke when tree growth params
  changed. Pinned `leaf_density` and `leaf_size` in `test_config` — a
  targeted fix that doesn't address the underlying coupling.
- *F-creature-sex (2026-03-30):* Adding `roll_creature_sex` at spawn
  (one extra PRNG call per creature) shifted the stream, breaking 4
  tests. Band-aid fix: seed changed from 42 to 200 for three tests,
  bootstrap interactions cranked to 50 for the fourth. All marked with
  HACK comments.

---

**Empirical validation (2026-03-30):**

Two perturbation experiments, each revealing a different dimension of
fragility with nearly disjoint failure sets.

*Experiment 1 — leaf_size 3→5:* 21 failures. Dominant mode: leaf blobs
at larger size fill previously-clear air, blocking projectile LOS at
hardcoded positions. Tests that use `force_position()` looked hardened
but the positions were only valid for one tree geometry.

- 13 projectile/LOS tests: `test_shoot_arrow_spawns_projectile`,
  `test_shoot_arrow_cooldown_prevents_second_shot`,
  `test_shoot_arrow_cooldown_expiry_allows_second_shot`,
  `test_shoot_arrow_leaf_does_not_block_los`,
  `shoot_arrow_hostile_in_path_does_not_block`,
  `flight_path_blocked_by_friendly_creature`,
  `arrow_chase_creates_autonomous_attack_move`,
  `arrow_chase_flying_creature_gets_chase_task`,
  `arrow_chase_preempts_autonomous_task`,
  `arrow_chase_second_hit_updates_destination`,
  `arrow_chase_second_hit_clears_target_creature`,
  `attack_target_spear_stops_at_extended_range`,
  `defensive_elf_with_task_interrupts_to_shoot_troll_at_10_voxels`
- 2 PRNG-shifted combat: `hostile_creature_pursues_and_attacks_elf`,
  `test_hostile_ai_shoots_when_armed`
- 1 PRNG-shifted hit check: `attack_target_continues_through_incapacitation_to_death`
- 2 diplomacy (civ relationships shifted): `diplomatic_relation_hostile_civs`,
  `is_non_hostile_different_civs_hostile`
- 2 fruit growth (leaf positions changed): `fruit_grows_during_heartbeat`,
  `fruit_heartbeat_tracks_species`
- 1 nav graph topology: `troll_pursues_elf_cross_graph_pathfinding`

*Experiment 2 — seed 42→99:* 10 failures (plus 1 expected checksum
test). Only 1 overlap with experiment 1.

- 4 hornet spawn at now-solid voxels: `aggressive_elf_vs_hornet_at_heights`,
  `ordered_elf_vs_hornet_at_heights`, `flying_creature_idle_wanders`,
  `flying_creature_directed_goto_mid_move_defers`
- 3 PRNG-shifted combat/pursuit: `hostile_ai_spear_attacks_at_extended_range`,
  `hostile_pursues_elf_within_detection_range`,
  `attack_target_continues_through_incapacitation_to_death`
- 1 projectile trajectory: `projectile_hits_solid_voxel_and_creates_ground_pile`
- 1 pursuit behavior: `defensive_creature_does_not_chase_far`
- 1 flying GoTo: `flying_creature_goto_reaches_destination`

~30 unique fragile tests from just 2 perturbations. The true number is
likely higher — these experiments only probe two dimensions of change.

---

**Static analysis findings (2026-03-30):**

5 independent reviewers (cautious, skeptical, veteran, statistical,
adversarial) audited all ~250 sim tests. These found a smaller set of
tests with assertion-level fragility that the empirical tests didn't
catch (different failure mode — not worldgen-coupled but structurally
flawed):

- `in_flight_arrow_hits_hostile_at_origin_neighbor` — compares HP
  against species base (100) without zeroing CON; creature's actual
  HP can exceed 100 from CON bonus
- `try_advance_skill_deterministic` — `assert_ne` between two seeds
  with ~4-5% collision probability (~20-30 reachable outcomes)
- `armor_degradation_non_penetrating_rare` — 100 trials at 5% rate,
  asserts count >= 1; P(zero) ≈ 0.6%
- `harvest_task_creates_ground_pile` and
  `harvest_fruit_carries_species_material` — index `fruit_positions[0]`
  without creating own fruit
- `stat_modified_hp_max_survives_serde_roundtrip` — borderline; asserts
  `hp_max > 100` on unzeroed troll (~2-3% failure probability)

**Blocks:** F-random-seeds

#### B-mesh-global-cfg — Mesh pipeline global atomics cause test flakiness risk
**Status:** Done

The mesh pipeline's behavior is controlled by global `AtomicBool`/`AtomicU32` statics in `mesh_gen.rs`: `SMOOTHING_ENABLED`, `DECIMATION_ENABLED`, `SMOOTH_NORMALS_ENABLED`, `QEM_ONLY`, `DECIMATION_MAX_ERROR`. Functions like `set_decimation_enabled(bool)` and `decimation_enabled()` read/write these atomics with `Ordering::Relaxed`. The pipeline functions (`generate_chunk_mesh`, `run_chamfer_smooth`, `run_decimation`) read these globals to decide which stages to run.

**The problem:** Unit tests in `mesh_gen.rs` and `chunk_neighborhood.rs` call `set_decimation_enabled(false)` (via the `one_chunk_world()` helper) or `set_decimation_enabled(true)` (in `sub_stages_match_generate_chunk_mesh`). Rust runs `#[test]` functions in parallel by default. If two tests run concurrently where one expects decimation enabled and the other expects it disabled, the global state is a race condition. The tests currently pass because most tests agree on `false` and the race window is tiny, but this is a latent flaky test — any new test that sets a different configuration could trigger nondeterministic failures.

Specific call sites that mutate global mesh config in tests:
- `mesh_gen::tests::one_chunk_world()` calls `set_decimation_enabled(false)` — used by ~20 tests
- `mesh_gen::tests::sub_stages_match_generate_chunk_mesh` calls `set_decimation_enabled(true)` and `set_smoothing_enabled(false)`
- `mesh_gen::tests::chunk_boundary_neighbor_check` calls `set_decimation_enabled(false)` directly
- `chunk_neighborhood::tests::neighborhood_mesh_matches_world_mesh` calls `set_decimation_enabled(false)`
- The integration test `mesh_snapshots.rs` also mutates these globals but runs in a separate process, so it's safe.

**Recommended fix: replace global atomics with a config struct parameter.**

Add a `MeshPipelineConfig` struct:
```rust
pub struct MeshPipelineConfig {
    pub smoothing_enabled: bool,
    pub smooth_normals_enabled: bool,
    pub decimation_enabled: bool,
    pub qem_only: bool,
    pub decimation_max_error: f32,
}
```
