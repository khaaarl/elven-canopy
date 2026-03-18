# Project Structure

Annotated directory tree for the Elven Canopy codebase. See also `CLAUDE.md` for key constraints and workflows, and `docs/design_doc.md` for full design specification.

```
elven-canopy/
├── Cargo.toml                  # Workspace root (resolver = "2")
├── elven_canopy_sim/           # Pure Rust simulation library (no Godot deps)
│   └── src/
│       ├── lib.rs              # Crate root, module declarations, re-exports prng crate
│       ├── types.rs            # VoxelCoord, SimUuid, entity IDs, Species enum
│       ├── command.rs          # SimCommand, SimAction
│       ├── config.rs           # GameConfig (loaded from JSON)
│       ├── species.rs          # SpeciesData — data-driven creature behavior
│       ├── stats.rs            # Creature stat multiplier table (2^20 fixed-point exponential)
│       ├── event.rs            # EventQueue (priority queue), SimEvent
│       ├── fruit.rs            # Procedural fruit species: types, generation, coverage, Vaelith naming
│       ├── session.rs          # GameSession — message-driven session management
│       ├── local_relay.rs      # LocalRelay — accumulator-based tick pacer (SP)
│       ├── lookup_map.rs      # LookupMap — non-iterable HashMap wrapper (deterministic)
│       ├── sim/                # SimState and all simulation logic (directory module)
│       │   ├── mod.rs          #   Struct definition, constructors, tick loop, event dispatch, serialization
│       │   ├── activation.rs   #   Creature activation chain, task selection, claiming
│       │   ├── combat.rs       #   Melee, ranged, projectiles, flee, hostile AI, diplomacy
│       │   ├── construction.rs #   Build/carve designation, materialization, furnishing, raycast
│       │   ├── crafting.rs     #   Recipe execution, active recipe management, cooking
│       │   ├── creature.rs     #   Spawning, surface placement, pile gravity, task cleanup
│       │   ├── greenhouse.rs   #   Fruit spawning, harvest monitoring
│       │   ├── inventory_mgmt.rs #  Item stack ops, reservations, equipment, durability
│       │   ├── logistics.rs    #   Hauling, harvesting, pickup/dropoff, logistics heartbeat
│       │   ├── movement.rs     #   GoTo commands, unit spreading, step execution, wandering
│       │   ├── needs.rs        #   Eating, sleeping, moping, personal item acquisition
│       │   ├── raid.rs         #   Raid triggering (hostile civ raiding parties)
│       │   └── task_helpers.rs #   Task extension table accessors, insert_task
│       ├── db.rs               # SimDb — tabulosity relational store (36 tables, all entities)
│       ├── mesh_gen.rs         # Chunk-based voxel mesh generation with face culling
│       ├── texture_gen.rs      # Prime-period tiling textures (tileable Perlin noise, bark + ground)
│       ├── nav.rs              # NavGraph, NavNode, NavEdge, graph construction
│       ├── pathfinding.rs      # A* search over NavGraph
│       ├── projectile.rs       # Integer-only ballistic trajectories, aim solver
│       ├── tree_gen.rs         # Procedural tree generation (trunk + branches)
│       ├── world.rs            # RLE column-based voxel grid
│       └── worldgen.rs         # Worldgen framework — generator sequencing, worldgen PRNG
├── elven_canopy_lang/          # Shared Vaelith conlang (types, lexicon, name gen)
│   └── src/
│       ├── lib.rs              # Lexicon loader (JSON → typed struct), re-exports
│       ├── types.rs            # Tone, VowelClass, Syllable, LexEntry, Word, PartOfSpeech
│       ├── phonotactics.rs     # Suffix tables (aspect, case) with vowel harmony
│       └── names.rs            # Deterministic elvish name generator
├── elven_canopy_prng/          # Shared xoshiro256++ PRNG (used by sim, music, lang)
│   ├── src/
│   │   └── lib.rs              # GameRng: xoshiro256++ with SplitMix64 seeding
│   └── Cargo.toml
├── tabulosity/                 # Typed in-memory relational store (derive macros)
│   ├── src/
│   │   ├── lib.rs              # Re-exports, module declarations
│   │   ├── error.rs            # Error enum (5 variants), DeserializeError
│   │   └── table.rs            # Bounded, FkCheck, TableMeta, AutoIncrementable, IntoQuery, QueryOpts
│   ├── tests/
│   │   ├── auto_increment.rs   # Auto-increment PK generation and serde roundtrip
│   │   ├── basic_table.rs      # CRUD operations on derived tables
│   │   ├── bounded.rs          # derive(Bounded) on newtypes
│   │   ├── database.rs         # FK validation, restrict/cascade/nullify on-delete
│   │   ├── indexed_table.rs    # Secondary indexes, range queries
│   │   ├── modify_unchecked.rs # Closure-based mutation (single/range/all) + debug assertions
│   │   ├── query_opts.rs       # QueryOpts ordering/offset + modify_each_by_*
│   │   ├── serde.rs            # Serde roundtrip (feature-gated)
│   │   └── unique_index.rs     # Unique index enforcement on insert/update
│   └── Cargo.toml
├── tabulosity_derive/          # Proc macros: derive(Bounded), derive(Table), derive(Database)
│   └── src/
│       ├── lib.rs              # Proc macro entry points
│       ├── bounded.rs          # derive(Bounded) for newtypes
│       ├── database.rs         # derive(Database) — FK validation, cascade/nullify, modify_unchecked delegation
│       ├── parse.rs            # Shared attribute parsing (#[primary_key], #[indexed])
│       └── table.rs            # derive(Table) — companion struct, indexes, serde, modify_unchecked, modify_each_by_*
├── elven_canopy_gdext/         # GDExtension bridge (depends on sim + godot crate)
│   └── src/
│       ├── lib.rs              # ExtensionLibrary entry point
│       ├── mesh_cache.rs       # Chunk mesh cache with dirty tracking
│       ├── elfcyclopedia_server.rs # Embedded localhost HTTP species bestiary
│       ├── sprite_bridge.rs    # SpriteGenerator — converts pixel buffers to Godot textures
│       └── sim_bridge.rs       # SimBridge node exposed to Godot
├── elven_canopy_sprites/       # Procedural sprite generation (pure Rust, RGBA8 buffers)
│   └── src/
│       ├── lib.rs              # Crate root, public API re-exports
│       ├── color.rs            # RGBA color type with darken/lighten helpers
│       ├── drawing.rs          # PixelBuffer with drawing primitives (circle, ellipse, rect, line)
│       ├── fruit.rs            # 16x16 fruit sprites (6 shapes + glow effect)
│       └── species/            # Dispatcher + per-species modules (10 species, 32x32 to 96x80)
│           ├── elf.rs          # Elf base sprite + CreatureDrawInfo compositing
│           └── elf_equipment.rs # Equipment overlay drawing (9 equippable item kinds)
├── elven_canopy_music/         # Palestrina-style polyphonic music generator
│   ├── src/
│   │   ├── lib.rs              # Crate root, module declarations
│   │   ├── main.rs             # CLI: single/batch/mode-scan generation
│   │   ├── grid.rs             # Core SATB score grid (eighth-note resolution)
│   │   ├── mode.rs             # Church mode scales (dorian through ionian)
│   │   ├── markov.rs           # Melodic/harmonic Markov models + motif library
│   │   ├── structure.rs        # High-level form planning, imitation points
│   │   ├── draft.rs            # Initial note placement with voice-leading
│   │   ├── scoring.rs          # 10-layer counterpoint quality scoring
│   │   ├── sa.rs               # Simulated annealing with adaptive cooling
│   │   ├── vaelith.rs          # Vaelith conlang grammar engine (elvish lyrics)
│   │   ├── text_mapping.rs     # Syllable-to-grid mapping, tonal contours
│   │   ├── midi.rs             # MIDI file output with embedded lyrics
│   │   ├── lilypond.rs         # LilyPond sheet music output
│   │   ├── generate.rs         # High-level runtime API (full pipeline in one call)
│   │   └── synth.rs            # Phase 1 waveform synthesizer (Grid → mono PCM)
│   └── Cargo.toml
├── godot/                      # Godot 4 project
│   ├── project.godot           # Project config + input map + autoloads
│   ├── elven_canopy.gdextension
│   ├── target -> ../target     # Symlink so Godot can find the compiled .so
│   ├── shaders/
│   │   └── bark_ground.gdshader # Prime-period tiling shader for bark/ground surfaces
│   ├── scenes/
│   │   ├── main.tscn           # Game scene (3D world, camera, renderers)
│   │   ├── main_menu.tscn      # Main menu (New Game / Load / Quit)
│   │   └── new_game.tscn       # New game config (seed, tree presets)
│   └── scripts/
│       ├── main.gd             # Game scene controller, wires all subsystems
│       ├── game_session.gd     # Autoload: persists seed/config across scenes
│       ├── main_menu.gd        # Main menu UI
│       ├── new_game_menu.gd    # New game screen with tree parameter sliders
│       ├── pause_menu.gd       # In-game pause overlay (ESC)
│       ├── save_dialog.gd      # Modal save-game dialog (name input)
│       ├── load_dialog.gd      # Modal load-game dialog (file list)
│       ├── orbital_camera.gd   # Camera controls (orbit, follow, vertical snap)
│       ├── elf_renderer.gd     # Billboard chibi elf sprites (pool pattern)
│       ├── capybara_renderer.gd # Billboard chibi capybara sprites
│       ├── tree_renderer.gd    # Tree voxel chunk mesh rendering (tiling shader materials)
│       ├── action_toolbar.gd   # Top toolbar (speed controls, gameplay) + toggleable debug panel
│       ├── construction_controller.gd # Click-drag construction placement (5-state FSM)
│       ├── height_grid_renderer.gd    # Wireframe height-slice grid overlay
│       ├── placement_controller.gd  # Click-to-place for spawns and tasks
│       ├── selection_controller.gd  # Click/box/double-click select creatures
│       ├── selection_utils.gd      # Pure click/box modifier helpers for selection
│       ├── tooltip_controller.gd    # Hover tooltips for world objects
│       ├── notification_display.gd  # Toast-style event notifications
│       ├── status_bar.gd           # Persistent bottom-left status bar (population, idle, tasks, speed)
│       ├── keybind_help.gd         # Keyboard shortcuts help overlay
│       ├── creature_info_panel.gd   # Right-side creature info + follow button
│       ├── group_info_panel.gd     # Right-side multi-creature selection panel
│       ├── selection_highlight.gd    # Faction-colored selection ring rendering (pool pattern)
│       ├── hp_bar.gd               # Overhead HP/MP bar textures and sprite helpers
│       ├── mana_vfx.gd             # Floating blue swirl VFX for mana-depleted work actions
│       ├── projectile_renderer.gd   # In-flight projectile rendering (oriented CylinderMesh pool)
│       ├── minimap.gd              # Zoomable top-down minimap (terrain, creatures, camera frustum)
│       ├── construction_music.gd    # Construction music playback (PCM via AudioStreamGenerator)
│       ├── item_utils.gd           # Item display utilities (condition_label for durability)
│       ├── view_toolbar.gd         # Right-edge vertical toolbar for view mode toggles
│       ├── view_toggle_button.gd   # Custom-drawn square toggle button for the view toolbar
│       └── view_toggle_icons.gd    # Procedural icon drawing (house and tree) for view toggles
├── data/                       # Shared data files (lexicon, Markov models, elfcyclopedia)
│   ├── vaelith_lexicon.json    # Vaelith vocabulary (41 entries with syllables + tones)
│   ├── species_elfcyclopedia.json # Species bestiary data (name, description, traits)
│   ├── markov_models.json      # Interval transition tables from Palestrina corpus
│   └── motif_library.json      # Ranked interval n-gram motifs
├── python/                     # Offline tools (not part of game runtime)
│   ├── corpus_analysis.py      # Trains Markov models from Renaissance polyphony
│   ├── rate_midi.py            # Pairwise MIDI comparison for preference training
│   └── requirements.txt        # music21, numpy, mido, python-rtmidi
├── docs/
│   ├── design_doc.md           # Full design specification (all phases)
│   ├── tracker.md              # Project tracker (features, bugs, status)
│   ├── project_structure.md    # Annotated directory tree (this file)
│   ├── implementation_status.md # Detailed per-phase feature status
│   ├── codebase_patterns.md    # Codebase conventions, patterns, and gotchas
│   ├── music_generator.md      # Music generator user guide + CLI reference
│   ├── tabulosity.md           # Tabulosity (sim DB) documentation
│   ├── items.md                # Item system documentation
│   ├── organic_tree_vision.md  # Tree generation design notes
│   ├── code_quality_plan.md    # Code quality improvement plan
│   ├── construction_roadmap.md # Construction system roadmap
│   └── drafts/                 # Working design documents
├── scripts/
│   ├── build.sh                # Build, test, and run script
│   └── tracker.py              # CLI tool for docs/tracker.md queries and mutations
└── default_config.json         # Default GameConfig values
```
