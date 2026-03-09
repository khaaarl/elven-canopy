# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Elven Canopy is a Dwarf Fortress-inspired simulation/management game set in a forest of enormous trees. The player is a **tree spirit** — the consciousness of an ancient tree — who forms a symbiotic relationship with a village of elves living on platforms, walkways, and structures grown from the tree's trunk and branches. Elves sing to the tree, and it grows in the desired shape, consuming mana. The tree provides food and shelter for the elves. Happy elves generate more mana, creating the game's central feedback loop.

**Key architectural decisions:**

- **Godot 4 + Rust via gdext.** Godot handles rendering, input, UI, and camera. All simulation logic lives in Rust.
- **Shared PRNG crate + game crates.** `elven_canopy_prng` provides a hand-rolled xoshiro256++ PRNG used by all crates (no external RNG dependencies). `elven_canopy_lang` provides shared Vaelith conlang types, vocabulary (JSON lexicon), and name generation — used by both `elven_canopy_sim` (elf names) and `elven_canopy_music` (lyrics). `elven_canopy_sim` is a pure Rust library (zero Godot dependencies) containing all simulation logic. `elven_canopy_gdext` is a thin wrapper that exposes the sim to Godot via GDExtension. `elven_canopy_music` is a standalone Palestrina-style polyphonic music generator with Vaelith (elvish) lyrics. The sim/gdext separation is enforced at the compiler level; the music crate is independent of both.
- **Deterministic simulation.** The sim is a pure function: `(state, commands) → (new_state, events)`. Hand-rolled xoshiro256++ PRNG (no external PRNG dependencies), no `HashMap` (use `BTreeMap`), no system dependencies. Designed for future lockstep multiplayer, perfect replays, and verifiable performance optimizations.
- **Command-driven mutation.** All sim state changes go through `SimCommand`. In single-player, the GDScript glue translates UI actions into commands. In multiplayer, commands are broadcast and canonically ordered.
- **Event-driven ticks.** The sim uses a discrete event simulation with a priority queue, not fixed-timestep iteration. Empty ticks are free, enabling efficient fast-forward.
- **Voxel world, graph pathfinding.** The world is a 3D voxel grid (sim truth), but pathfinding uses a nav graph of nodes and edges matching the constrained topology (platforms, bridges, stairs, trunk surfaces).
- **Data-driven config.** All tunable parameters live in a `GameConfig` struct loaded from JSON. No magic numbers in the sim.

For full details, see `docs/design_doc.md`. Note that the design doc is an aspirational planning document — many features it describes (construction, structural integrity, fire, emotional systems, etc.) are not yet implemented. See `docs/tracker.md` for current feature status.

## Implementation Status

Loose overview of where things stand. See `docs/tracker.md` for the full project tracker with per-feature status, blocking relationships, and design doc cross-references. **Keep this section roughly in sync with the tracker** — it's a quick orientation aid, not a detailed status report.

- **Phase 0 (Foundations):** Complete.
- **Phase 1 (A Tree and an Elf):** Complete. Ten species implemented: Elf, Capybara, Boar, Deer, Elephant, Goblin, Monkey, Orc, Squirrel, Troll (all with procedural sprites, data-driven behavior). Goblin/Orc/Troll are hostile-faction placeholders — spawnable via debug UI, wander and climb, no food decay or mood system yet. Dynamic pursuit infrastructure: tasks can track a moving target creature with automatic repathfinding (prerequisite for combat).
- **Phase 2 (Construction and Persistence):** Partial — construction loop works (designate/build/cancel with incremental nav updates), save/load works, Rust chunk-based mesh generation with face culling replaces GDScript MultiMesh rendering. Mouse-driven click-drag placement UI with height-slice grid overlay implemented. Hover tooltips for creatures, structures, ground piles, and fruit. Persistent status bar (bottom-left) showing elf population, idle count, active tasks, and sim speed. No mana economy, no visual smoothing.
- **Phase 6 (Culture and Language):** Music crate complete with Phase 1 waveform synthesizer (`synth.rs`) and runtime generation API (`generate.rs`). Integrated into game via gdext: construction designation triggers background composition, GDScript `construction_music.gd` plays PCM through `AudioStreamGenerator`. Shared lang crate (`elven_canopy_lang`) provides Vaelith types, lexicon, and name generation. Embedded encyclopedia HTTP server (localhost, species bestiary) with in-game book button to open in browser.
- **Phase 4 (Economy and Ecology):** Kitchen cooking, workshop manufacturing (bow/arrow/bowstring recipes), elf personal item acquisition, creature thoughts, and basic mood scoring implemented. Notification system with sim-side persistence (SimDb table), multiplayer-aware command pipeline, toast UI, and moping notifications. Creature actions formalized as typed, duration-bearing operations (`ActionKind` enum, `MoveAction` table for interpolation). Unified `interrupt_task()` entry point for all task interruption and cleanup (nav invalidation, mope preemption, pursuit abandonment); rest not started.
- **Tabulosity (sim DB):** Typed in-memory relational store complete — derive macros for `Bounded`, `Table`, `Database` with FK validation and serde support (feature-gated). Includes compound indexes (`#[index(...)]`) with prefix queries, filtered/partial indexes, unified `IntoQuery` API, tracked runtime bounds, `on_delete cascade`/`nullify` FK semantics with cycle detection, auto-increment primary keys (`#[primary_key(auto_increment)]`), unique index enforcement (`#[indexed(unique)]`), `modify_unchecked` closure-based in-place mutation with debug-build safety checks, `QueryOpts` for ordering (asc/desc) and offset (skip N) on all query methods, `modify_each_by_*` query-driven batch mutation, and schema versioning (`#[schema_version(N)]`) with missing-tables-default-to-empty on deserialization. **Integrated into `elven_canopy_sim`:** `SimDb` (26 tables) replaces all BTreeMap entity storage — creatures, tasks (with decomposed extension tables), blueprints, structures, inventories, item stacks (with subcomponents and enchantments), ground piles, thoughts, notifications, furniture, music compositions, logistics wants, civilizations, and civ relationships.
- **Phase 3 (Combat):** Projectile ballistics math module complete (integer-only sub-voxel coordinates, symplectic Euler trajectory stepping, iterative aim solver, speed²-based damage formula). Creature spatial index complete (BTreeMap voxel→creatures, maintained at all position mutation points, multi-voxel footprint support). HP/death system complete: per-species hp_max, VitalStatus (Alive/Dead), DamageCreature/HealCreature/DebugKillCreature commands, death handler (task interruption, inventory drop, spatial index deregistration, notification). Dead creatures remain in DB (vital_status=Dead) for future states (ghost, undead). Entity/SimDb/tick integration for projectiles not yet started.
- **Phases 5, 7–8:** Not started.

## Project Structure

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
│       ├── event.rs            # EventQueue (priority queue), SimEvent
│       ├── fruit.rs            # Procedural fruit species: types, generation, coverage, Vaelith naming
│       ├── session.rs          # GameSession — message-driven session management
│       ├── local_relay.rs      # LocalRelay — accumulator-based tick pacer (SP)
│       ├── sim.rs              # SimState, tick loop, command processing
│       ├── db.rs               # SimDb — tabulosity relational store (26 tables, all entities)
│       ├── mesh_gen.rs          # Chunk-based voxel mesh generation with face culling
│       ├── texture_gen.rs      # Procedural face textures (3D Perlin noise atlases)
│       ├── nav.rs              # NavGraph, NavNode, NavEdge, graph construction
│       ├── pathfinding.rs      # A* search over NavGraph
│       ├── projectile.rs       # Integer-only ballistic trajectories, aim solver, damage
│       ├── tree_gen.rs         # Procedural tree generation (trunk + branches)
│       ├── world.rs            # Dense 3D voxel grid
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
│       ├── encyclopedia_server.rs # Embedded localhost HTTP species bestiary
│       └── sim_bridge.rs       # SimBridge node exposed to Godot
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
│       ├── tree_renderer.gd    # Tree voxel mesh rendering (MultiMesh)
│       ├── sprite_factory.gd   # Procedural chibi sprite generation from seed
│       ├── action_toolbar.gd   # Top toolbar (speed controls, gameplay) + toggleable debug panel
│       ├── construction_controller.gd # Click-drag construction placement (5-state FSM)
│       ├── height_grid_renderer.gd    # Wireframe height-slice grid overlay
│       ├── placement_controller.gd  # Click-to-place for spawns and tasks
│       ├── selection_controller.gd  # Click-to-select creatures
│       ├── tooltip_controller.gd    # Hover tooltips for world objects
│       ├── notification_display.gd  # Toast-style event notifications
│       ├── status_bar.gd           # Persistent bottom-left status bar (population, idle, tasks, speed)
│       ├── keybind_help.gd         # Keyboard shortcuts help overlay
│       ├── creature_info_panel.gd   # Right-side creature info + follow button
│       └── construction_music.gd    # Construction music playback (PCM via AudioStreamGenerator)
├── data/                       # Shared data files (lexicon, Markov models, encyclopedia)
│   ├── vaelith_lexicon.json    # Vaelith vocabulary (41 entries with syllables + tones)
│   ├── species_encyclopedia.json # Species bestiary data (name, description, traits)
│   ├── markov_models.json      # Interval transition tables from Palestrina corpus
│   └── motif_library.json      # Ranked interval n-gram motifs
├── python/                     # Offline tools (not part of game runtime)
│   ├── corpus_analysis.py      # Trains Markov models from Renaissance polyphony
│   ├── rate_midi.py            # Pairwise MIDI comparison for preference training
│   └── requirements.txt        # music21, numpy, mido, python-rtmidi
├── docs/
│   ├── design_doc.md           # Full design specification (all phases)
│   ├── tracker.md              # Project tracker (features, bugs, status)
│   ├── music_generator.md      # Music generator user guide + CLI reference
│   ├── organic_tree_vision.md  # Tree generation design notes
│   └── drafts/                 # Working design documents
├── scripts/
│   ├── build.sh                # Build, test, and run script
│   └── tracker.py              # CLI tool for docs/tracker.md queries and mutations
└── default_config.json         # Default GameConfig values
```

## Building and Running

Use `scripts/build.sh` for all build operations. It ensures the `godot/target` symlink exists before compiling.

```bash
scripts/build.sh            # Debug build
scripts/build.sh release    # Release build
scripts/build.sh test       # Run all crate tests
scripts/build.sh quicktest  # Test only crates changed vs main + multiplayer
scripts/build.sh run        # Debug build, then launch the game
scripts/build.sh run-branch NAME  # Pull main, checkout branch, pull, build+run
```

To run sim tests alone: `cargo test -p elven_canopy_sim`

To run lang crate tests: `cargo test -p elven_canopy_lang`

To run music crate tests: `cargo test -p elven_canopy_music`

To run tabulosity tests: `cargo test -p tabulosity -p tabulosity_derive`

To run tabulosity serde tests (separate invocation): `cargo test -p tabulosity --features serde --test serde`

To generate music from the CLI: `cargo run -p elven_canopy_music -- --help` (see `docs/music_generator.md` for full usage).

### Python Tools

The `python/` directory contains offline training tools for the music generator — they are **not** part of the game runtime. **Never use `source .venv/bin/activate`** — always invoke tools via their full venv path (e.g., `python/.venv/bin/python`).

```bash
cd python && python3 -m venv .venv && .venv/bin/pip install -r requirements.txt   # One-time setup
cd python && .venv/bin/python corpus_analysis.py   # Train Markov models from Palestrina corpus → data/
cd python && .venv/bin/python rate_midi.py          # Pairwise MIDI comparison for preference model training
```

## Toolchain Versions

- **Rust edition:** 2024
- **gdext crate:** `godot` 0.4.5 with feature `api-4-5`
- **Godot:** 4.6 (forward-compatible with the 4.5 API)

When upgrading the `godot` crate, check for a matching `api-4-x` feature flag. The API version must be ≤ the Godot runtime version.

## Code Quality Tools

`cargo fmt`, `cargo clippy`, `cargo test`, `gdformat`, and `gdlint` are all enforced in CI via `.github/workflows/ci.yml`. Run all checks locally with:

```bash
scripts/build.sh check      # fmt --check + clippy + gdformat --check + gdlint
scripts/build.sh test       # run all crate tests + gdext compile check
scripts/build.sh quicktest  # test only crates changed vs main + multiplayer
```

### Rust

Workspace lint config lives in the root `Cargo.toml` under `[workspace.lints.clippy]`. Each crate inherits via `[lints] workspace = true`. Formatting config is in `rustfmt.toml` (currently all defaults).

Run individually:

```bash
cargo fmt --all --check       # check formatting
cargo clippy --workspace -- -D warnings   # lint
cargo fmt --all               # auto-format
```

### GDScript

GDScript files are checked with **gdformat** (formatter) and **gdlint** (linter) from the [gdtoolkit](https://github.com/Scony/godot-gdscript-toolkit) package.

**Setup (one-time):** `scripts/build.sh check` auto-creates the venv and installs gdtoolkit if missing.

Run individually:

```bash
python/.venv/bin/gdformat --check --line-length 100 godot/scripts/*.gd
python/.venv/bin/gdlint godot/scripts/*.gd
python/.venv/bin/gdformat --line-length 100 godot/scripts/*.gd   # auto-format
```

**Configuration:** `.gdlintrc` at the repo root configures gdlint. Currently disables `function-variable-name` (short names like `W`/`H` are intentional in pixel-drawing code).

## Running Commands

The repo's `.claude/settings.json` sets `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1`, which resets the Bash tool's working directory to the project root before every command. This means you never need to worry about working directory drift — just write commands relative to the repo root.

**Keep Bash commands simple.** Do not use `source`, command substitution (`$(...)` or backticks), heredocs (`<<EOF`), shell variables, or other shell tricks. These trigger unnecessary permission prompts. Also avoid putting flag names inside quotes (e.g., `git show --stat "--format="` can trigger a "quoted flag names" permission check) — keep flags as bare arguments. Use the dedicated Read/Write/Edit tools for file operations. For `git commit`, always use the `.tmp/commit-msg.txt` + `git commit -F` approach described in the "Committing Code" section.

## Scratch Files

Use `.tmp/` in the repo root (gitignored) for any temporary files — benchmark output, intermediate data, scratch scripts, etc. Before writing, run `ls .tmp` to check if the directory exists; if it doesn't, run `mkdir .tmp`. **Do NOT use `/tmp`** — it can trigger permission prompts and isn't project-scoped.

## Module Docstrings

Every code file should have a top-level comment that helps someone new to the codebase orient themselves. Cover:

- **What the file does** — its purpose and scope.
- **How it fits into the system** — which sibling files it delegates to or depends on, and what role it plays in the larger architecture. Use file extensions when referencing files (e.g., ``tempering.py``, not ``tempering``) so it's clear these are files, not abstract concepts.
- **Notable or surprising algorithms** — anything non-obvious that a reader might need context for (e.g., angular-sweep visibility, OBB collision via SAT).
- **Critical constraints** — if the file is subject to the determinism requirement, say so explicitly. A newcomer who doesn't know about the requirement can easily break it.

Keep it proportional to the file's complexity. A 50-line utility doesn't need a paragraph; an 800-line engine core may need several paragraphs to explain its algorithms and how it fits into the rest of the project. Test files can be brief.

When making changes to a file, consider whether documentation elsewhere needs updating — module docstrings in sibling files that reference the changed module, the architecture overview in this file, etc. A renamed function or shifted responsibility can leave other files' docstrings silently wrong.

## Codebase Patterns and Gotchas

Things that are non-obvious or surprising about this codebase:

**Data file loading (CRITICAL):**
- **Never use runtime file I/O (`std::fs`, `FileAccess`) to load static data files** (JSON configs, lexicons, etc.). Always use `include_str!` or `include_bytes!` to embed them at compile time. Runtime paths break in exported Godot builds because `res://` points into the PCK bundle and relative paths outside it don't exist. See `elven_canopy_lang/src/lib.rs` and `elven_canopy_gdext/src/encyclopedia_server.rs` for examples of the correct pattern.

**Tick rate and sim decoupling:**
- The sim runs at **1000 ticks per simulated second** (`tick_duration_ms = 1`). All tick-denominated config values (heartbeat intervals, food decay rates, species speed params) are calibrated for this rate.
- The sim is decoupled from the frame rate. `main.gd` calls `bridge.frame_update(delta)` each frame. In single-player, a `LocalRelay` on the Rust side handles tick pacing with a time-based accumulator, capped at 5000 ticks per frame to prevent spiral-of-death.
- Movement speed is per-species: `walk_ticks_per_voxel` (ticks per 1.0 units of euclidean distance on flat ground) and `climb_ticks_per_voxel` (ticks per 1.0 units on TrunkClimb/GroundToTrunk edges). Nav graph edges store euclidean distance, not time-cost — speed config is not needed for graph construction.

**Voxel coordinate system:**
- Y is up. The world is (x, z) horizontal, y vertical.
- Flat array indexing: `x + z * size_x + y * size_x * size_z`. Y is the outermost axis, not the middle one.
- Forest floor is at y=0 (solid `ForestFloor` voxels). Creatures walk on air voxels at y=1 (above the floor). Nav nodes start at y=1.
- Voxel coordinates are integer corners. Renderers offset by +0.5 to center meshes/sprites on the voxel.

**Navigation graph:**
- Built from the voxel world at startup, not updated incrementally. If the world changes, the nav graph must be rebuilt.
- Uses 26-connectivity (not 6) to avoid disconnecting thin geometry like radius-1 branches. Duplicate edges are avoided by only checking 13 "positive-half" neighbor offsets per node.
- A nav node exists for every air voxel that has at least one face-adjacent solid voxel (i.e., the creature is standing on or clinging to a surface).

**Tree generation:**
- Trunk is just the first branch — all segments (trunk, branches, roots) use the same growth algorithm with different parameters.
- Every tree voxel must be face-connected (6-connectivity) to at least one other tree voxel. `bridge_cross_sections()` fills gaps when growth steps diagonally.
- Voxel type priority: Trunk > Branch > Root > Leaf > Air. Higher types are never overwritten by lower ones.

**GDScript UI:**
- All UI is built programmatically in `_ready()` methods, not in `.tscn` scene files. The scene files are mostly empty shells.
- `game_session.gd` is a Godot autoload singleton that persists seed and tree config across scene transitions (main menu → new game → game).

**SimBridge command flow:**
- All commands (spawn, goto, build, carve, etc.) are buffered and execute on the next `frame_update()` (~16ms at 60fps), with identical behavior in SP and MP. No command auto-steps the sim.
- Build/carve validation is done upfront by the `validate_*_preview()` query methods that GDScript calls before confirming placement. The designation commands themselves are fire-and-forget.

**Sprite rendering and movement interpolation:**
- Elf sprites are offset +0.48 in Y, capybara sprites +0.32, to visually center them above their nav node position. Selection ray-to-sprite distance uses these same offsets.
- Sprites use a pool pattern: created on demand, never destroyed, only hidden when count decreases.
- Creature positions are smoothly interpolated between nav nodes. Movement interpolation data lives in the `MoveAction` table (`move_from`/`move_to`/`move_start_tick`/`move_end_tick`), separate from the `Creature` struct. Each `Creature` has `action_kind` and `next_available_tick` fields tracking its current action. `bridge.frame_update(delta)` returns a fractional `render_tick` each frame; `main.gd` distributes it to renderers and the selection controller. `SimBridge.get_creature_positions(species, render_tick)` calls `Creature::interpolated_position()` to lerp between nav nodes using the associated `MoveAction` row.

**Input precedence:**
- ESC handling flows: placement_controller (cancel placement) → construction_controller (cancel construction) → selection_controller (deselect) → pause_menu (open/close menu). Each handler calls `set_input_as_handled()` to prevent downstream handlers from firing.

**Keyboard shortcut assignment (CRITICAL):**
- Before assigning ANY new keyboard shortcut, **thoroughly audit all existing bindings** across every GDScript file. Search for `KEY_` in `godot/scripts/` to find all current bindings. Many keys are already in use (Space, 1-3, B, T, U, I, F12, ?, ESC, Enter, arrow keys, +/=).
- **Always ask the user** before assigning a shortcut — never pick one unilaterally.

**Dev profile tuning:**
- `Cargo.toml` sets `opt-level = 0` for the dev profile (fastest compile times). For machines that run the game with UI, override to `opt-level = 1` via `.cargo/config.toml` (gitignored) for ~4x faster sim execution at a small compile-time cost. The test profile inherits from dev.

## Branching (CRITICAL — DO THIS FIRST)

**NEVER make ANY edits to files on `main` unless the user explicitly asks you to.** This includes "just reading and tweaking" — if you're about to use Edit or Write on any file, you must be on a feature branch. Before writing ANY code, you MUST:

1. Create a feature branch: `git checkout -b feature/F-tracker-id` (or `bug/B-tracker-id` for bugs). If the work has a tracker ID, use it as the branch name — e.g., `feature/F-tree-overlap`. If there's no tracker ID yet (exploratory work, docs-only changes), use a descriptive name like `feature/descriptive-branch-name`.
2. Push the branch to origin: `git push -u origin feature/F-tracker-id`
3. Verify you are on the feature branch: `git branch --show-current`
4. ONLY THEN start making changes

**This is non-negotiable.** If you realize you are on `main` and have already made changes, STOP immediately and ask the user how to proceed — do NOT commit to `main`.

The only exception is editing `CLAUDE.md` itself, which can be done on `main` if explicitly requested. However, do NOT commit or push CLAUDE.md changes until the user explicitly says to — they may want to review or iterate first.

## Committing Code

ALWAYS ASK FOR PERMISSION BEFORE COMMITTING TO MAIN/MASTER, BUT COMMITTING TO FEATURE BRANCHES DOES NOT REQUIRE PERMISSION. When committing to a feature branch, always push to origin immediately after committing (`git push`).

**Pre-commit checks (CRITICAL):** Before every commit that includes code changes (Rust or GDScript), run `scripts/build.sh check` and fix any issues. Do NOT commit code that fails formatting or linting. For commits that change Rust code, also run `scripts/build.sh quicktest` and ensure all tests pass. Non-code changes (e.g., docs, config, CLAUDE.md) can skip these steps.

**Commit message procedure:** Always write the commit message to `.tmp/commit-msg.txt` using the Write tool, then commit with `-F`:

```bash
git commit -F .tmp/commit-msg.txt
rm .tmp/commit-msg.txt
```

This applies to all commits — single-line and multi-line alike. Do NOT use `-m` flags, command substitution, heredocs, or shell variables to build commit messages.

## The Once-Over

When a feature branch's work is done, the user will likely ask for a "once-over" — a final quality review before merging. Use the `/once-over` slash command, which delegates the review to a subagent to keep the main context clean. See `.claude/commands/once-over.md` for the full checklist.

## Merging to Main

When the user asks to merge a feature branch to main, use the `/merge-to-main` slash command. It follows a squash-rebase-ff workflow that keeps main's history clean. The entire procedure is delegated to a subagent to keep the main context clean. See `.claude/commands/merge-to-main.md` for the full procedure.

## Conversation Flow (CRITICAL)

**Default to talking, not doing.** You are far too proactive by default. When in doubt, respond with text and wait for an explicit instruction to act. This is one of the most important rules in this file.

**Questions:** When the user asks a question, ONLY answer the question. Do not continue with previous work, do not "move on." Stop and wait for the user to explicitly tell you to proceed.

**Design and planning discussions:** When the user is discussing design, brainstorming, planning, or giving feedback on a sketch — respond with text. Do NOT start editing files, writing code, or updating the tracker. Phrases like "let's do X", "we should add Y", "I'm envisioning Z" in a design conversation are the user thinking out loud, not giving you an edit instruction. Stay in the conversation until the user explicitly asks you to implement, write, edit, or create something. Even then, confirm scope before starting if the request is ambiguous.

**When to act:** Only start editing files or running commands when the user gives a clear, unambiguous instruction to do so — e.g., "implement this", "write that test", "update the tracker", "make a branch and do X". If you're not sure whether the user wants you to act or keep discussing, ask.

## Key Constraints

- **Determinism (sim crate)**: `elven_canopy_sim` must produce identical results given the same seed. No hash-order dependence, no set iteration, no stdlib PRNG. All crates share a hand-rolled xoshiro256++ PRNG from `elven_canopy_prng` (with SplitMix64 seeding) — no external PRNG crate dependencies. This enables consistency in multiplayer and verification of optimizations. **Scope:** The strict determinism constraint (identical results across platforms/compilers) applies to `elven_canopy_sim`. The music crate uses the same PRNG for seed-based reproducibility but doesn't participate in lockstep multiplayer or replay verification.

## Simulator: Test-Driven Workflow (CRITICAL)

**Applies to:** Bug fixes and new features that affect simulator behavior.

1. **Write a failing unit test** that captures the bug or specifies the new behavior. Do NOT use `xfail`, `skip`, or any other marker — write a plain test that runs and fails.
   Confirm the new test **fails for the expected reason** — read the failure output and verify it fails because the behavior under test is wrong/missing, not because of a typo, import error, or unrelated issue.

2. **Write code** to make the test pass.
   Confirm the new test **passes** and no existing tests regress.

3. Repeat steps 1–2 as needed until the fix or feature is complete.

4. **Audit test coverage before considering the feature complete.** For every behavior described in the feature spec or design, there must be a corresponding test. Systematically check:
    - Every distinct code path the feature introduces (not just the happy path — the "elf walks home" path is different from the "elf is already home" path)
    - Interactions with existing systems: if the feature can be interrupted by X, or can't interrupt Y, test both.
    - Guard clauses and rejection cases (already in this state, blocked by higher-priority task, etc.)
    - Serde roundtrip for any new enum variant, config field, or persisted type — if sibling variants have a test, the new one needs one too
    - Do not count on shared infrastructure being "tested elsewhere" as a reason to skip testing a specific feature's use of that infrastructure. The test proves *this feature's* integration works, not that the infrastructure works in general.

When tests fail unexpectedly, diagnose the root cause. Do not bypass, skip, or work around failing checks (validators, lints, assertions). Never increase retry counts, disable validation, or add #[ignore] to make a test pass. Do not ever take the "easy" route; do the right thing. If the user has not requested that you operate on your own, you may ask the user for guidance after thoroughly examining the problem.

## Project Tracker (`docs/tracker.md`)

The tracker is the single source of truth for feature/bug status. **Use `scripts/tracker.py`** for all tracker operations — it handles both sections, ordering, and relationship symmetry automatically. Run `list` at the start of any work session to understand what's in progress, what's next, and what's blocked.

**Query commands** (read-only, stdout — use these instead of reading the full file):
```bash
python3 scripts/tracker.py list [--status todo|progress|done|all]  # default: progress + todo
python3 scripts/tracker.py show <ID> [<ID> ...]                    # full detail entries
python3 scripts/tracker.py search <pattern> [-i]                   # regex search
```

**Mutation commands** (edit in place, auto-run `fix` at end):
```bash
python3 scripts/tracker.py change-state <ID> todo|progress|done
python3 scripts/tracker.py add <ID> <title> --group <GROUP> [--phase N] [--refs §N] [--status todo|progress|done]
python3 scripts/tracker.py edit-title <ID> <title>
python3 scripts/tracker.py edit-description <ID> <FILE>               # read description from file
python3 scripts/tracker.py block <ID> --by <ID>
python3 scripts/tracker.py unblock <ID> --by <ID>
python3 scripts/tracker.py relate <ID1> <ID2>
python3 scripts/tracker.py unrelate <ID1> <ID2>
python3 scripts/tracker.py fix                                     # sort, symmetrize, prune
```

All mutation commands support `--dry-run` to preview changes as a unified diff.

**Other guidelines:**
- When a draft design doc is created, link it from the tracker item (`**Draft:** path`).
- If work reveals a new bug or sub-task, add it as a new tracker item rather than leaving it as a TODO comment in code.
