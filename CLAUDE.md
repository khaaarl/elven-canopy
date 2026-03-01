# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Elven Canopy is a Dwarf Fortress-inspired simulation/management game set in a forest of enormous trees. The player is a **tree spirit** — the consciousness of an ancient tree — who forms a symbiotic relationship with a village of elves living on platforms, walkways, and structures grown from the tree's trunk and branches. Elves sing to the tree, and it grows in the desired shape, consuming mana. The tree provides food and shelter for the elves. Happy elves generate more mana, creating the game's central feedback loop.

**Key architectural decisions:**

- **Godot 4 + Rust via gdext.** Godot handles rendering, input, UI, and camera. All simulation logic lives in Rust.
- **Shared PRNG crate + game crates.** `elven_canopy_prng` provides a hand-rolled xoshiro256++ PRNG used by all crates (no external RNG dependencies). `elven_canopy_sim` is a pure Rust library (zero Godot dependencies) containing all simulation logic. `elven_canopy_gdext` is a thin wrapper that exposes the sim to Godot via GDExtension. `elven_canopy_music` is a standalone Palestrina-style polyphonic music generator with Vaelith (elvish) lyrics. The sim/gdext separation is enforced at the compiler level; the music crate is independent of both.
- **Deterministic simulation.** The sim is a pure function: `(state, commands) → (new_state, events)`. Hand-rolled xoshiro256++ PRNG (no external PRNG dependencies), no `HashMap` (use `BTreeMap`), no system dependencies. Designed for future lockstep multiplayer, perfect replays, and verifiable performance optimizations.
- **Command-driven mutation.** All sim state changes go through `SimCommand`. In single-player, the GDScript glue translates UI actions into commands. In multiplayer, commands are broadcast and canonically ordered.
- **Event-driven ticks.** The sim uses a discrete event simulation with a priority queue, not fixed-timestep iteration. Empty ticks are free, enabling efficient fast-forward.
- **Voxel world, graph pathfinding.** The world is a 3D voxel grid (sim truth), but pathfinding uses a nav graph of nodes and edges matching the constrained topology (platforms, bridges, stairs, trunk surfaces).
- **Data-driven config.** All tunable parameters live in a `GameConfig` struct loaded from JSON. No magic numbers in the sim.

For full details, see `docs/design_doc.md`. Note that the design doc is an aspirational planning document — many features it describes (construction, structural integrity, fire, emotional systems, etc.) are not yet implemented. See `docs/tracker.md` for current feature status.

## Implementation Status

Loose overview of where things stand. See `docs/tracker.md` for the full project tracker with per-feature status, blocking relationships, and design doc cross-references. **Keep this section roughly in sync with the tracker** — it's a quick orientation aid, not a detailed status report.

- **Phase 0 (Foundations):** Complete.
- **Phase 1 (A Tree and an Elf):** Complete.
- **Phase 2 (Construction and Persistence):** Partial — construction loop works (designate/build/cancel with incremental nav updates), save/load works, but no blueprint mode UI, no mana economy, no visual smoothing.
- **Phase 6 (Culture and Language):** Music crate complete as standalone generator, not yet integrated into game runtime.
- **Phases 3–5, 7–8:** Not started.

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
│       ├── sim.rs              # SimState, tick loop, command processing
│       ├── nav.rs              # NavGraph, NavNode, NavEdge, graph construction
│       ├── pathfinding.rs      # A* search over NavGraph
│       ├── tree_gen.rs         # Procedural tree generation (trunk + branches)
│       └── world.rs            # Dense 3D voxel grid
├── elven_canopy_prng/          # Shared xoshiro256++ PRNG (used by sim, music, lang)
│   ├── src/
│   │   └── lib.rs              # GameRng: xoshiro256++ with SplitMix64 seeding
│   └── Cargo.toml
├── elven_canopy_gdext/         # GDExtension bridge (depends on sim + godot crate)
│   └── src/
│       ├── lib.rs              # ExtensionLibrary entry point
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
│   │   └── lilypond.rs         # LilyPond sheet music output
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
│       ├── orbital_camera.gd   # Camera controls (orbit + follow mode)
│       ├── elf_renderer.gd     # Billboard chibi elf sprites (pool pattern)
│       ├── capybara_renderer.gd # Billboard chibi capybara sprites
│       ├── tree_renderer.gd    # Tree voxel mesh rendering (MultiMesh)
│       ├── sprite_factory.gd   # Procedural chibi sprite generation from seed
│       ├── action_toolbar.gd   # Top toolbar (gameplay) + toggleable debug panel
│       ├── placement_controller.gd  # Click-to-place for spawns and tasks
│       ├── selection_controller.gd  # Click-to-select creatures
│       └── creature_info_panel.gd   # Right-side creature info + follow button
├── data/                       # Trained Markov models for music generator
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
│   └── build.sh                # Build, test, and run script
└── default_config.json         # Default GameConfig values
```

## Building and Running

Use `scripts/build.sh` for all build operations. It ensures the `godot/target` symlink exists before compiling.

```bash
scripts/build.sh          # Debug build
scripts/build.sh release  # Release build
scripts/build.sh test     # Run sim tests, then debug build
scripts/build.sh run      # Debug build, then launch the game
```

To run sim tests alone: `cargo test -p elven_canopy_sim`

To run music crate tests: `cargo test -p elven_canopy_music`

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
scripts/build.sh check    # fmt --check + clippy + gdformat --check + gdlint
scripts/build.sh test     # run sim tests, then debug build
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

**Keep Bash commands simple.** Do not use `source`, command substitution (`$(...)` or backticks), heredocs (`<<EOF`), shell variables, or other shell tricks. These trigger unnecessary permission prompts. Also avoid putting flag names inside quotes (e.g., `git show --stat "--format="` can trigger a "quoted flag names" permission check) — keep flags as bare arguments. Use the dedicated Read/Write/Edit tools for file operations. For `git commit`, pass the message directly with `-m "..."` using a simple quoted string.

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

**Tick rate and sim decoupling:**
- The sim runs at **1000 ticks per simulated second** (`tick_duration_ms = 1`). All tick-denominated config values (heartbeat intervals, food decay rates, species speed params) are calibrated for this rate.
- The sim is decoupled from the frame rate. `main.gd` uses a time-based accumulator to compute how many ticks to advance per frame, capped at 5000 to prevent spiral-of-death.
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

**SimBridge side effects:**
- `spawn_elf()`, `spawn_capybara()`, and `create_goto_task()` in `sim_bridge.rs` automatically step the sim by 1 tick after applying the command. This is convenient for UI but means these are not pure command-enqueue operations.

**Sprite rendering and movement interpolation:**
- Elf sprites are offset +0.48 in Y, capybara sprites +0.32, to visually center them above their nav node position. Selection ray-to-sprite distance uses these same offsets.
- Sprites use a pool pattern: created on demand, never destroyed, only hidden when count decreases.
- Creature positions are smoothly interpolated between nav nodes. Each `Creature` stores `move_from`/`move_to`/`move_start_tick`/`move_end_tick` (rendering metadata, never read by sim logic). `main.gd` computes a fractional `render_tick = current_tick + accumulator_fraction` each frame and distributes it to renderers and the selection controller. `SimBridge.get_elf_positions(render_tick)` and `get_capybara_positions(render_tick)` call `Creature::interpolated_position()` to lerp between nav nodes.

**Input precedence:**
- ESC handling flows: placement_controller (cancel placement) → selection_controller (deselect) → pause_menu (open/close menu). Each handler calls `set_input_as_handled()` to prevent downstream handlers from firing.

**Codegen tuning:**
- `Cargo.toml` sets `codegen-units = 256` for both dev and release. This is intentional: `godot-core` generates massive binding code, and lower codegen-units prevent RAM from exceeding 4 GB during compilation.

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

**Pre-commit checks (CRITICAL):** Before every commit that includes code changes (Rust or GDScript), run `scripts/build.sh check` and fix any issues. Do NOT commit code that fails formatting or linting. For commits that change Rust sim or music crate code, also run `scripts/build.sh test` and ensure all tests pass. Non-code changes (e.g., docs, config, CLAUDE.md) can skip these steps.

## Merging to Main

When the user asks to merge a feature branch to main, follow this procedure:

```bash
# 1. Create a temporary LOCAL branch and squash all feature commits into one
#    (This way conflicts only need to be resolved once, not per-commit)
#    IMPORTANT: The REAL commit message goes HERE — step 5 is a fast-forward
#    merge which does NOT create a new commit, so any -m there is ignored.
#    NOTE: The -rebase branch is local only — do NOT push it to origin.
git checkout -b feature/my-branch-rebase feature/my-branch
git merge-base main feature/my-branch-rebase  # Learn the common ancestor!
git reset --soft THAT-COMMON-ANCESTOR
git commit -m "Your descriptive commit message here"

# 2. Pull latest main
git checkout main && git pull

# 3. Rebase the single squashed commit onto main (conflict detection here)
git checkout feature/my-branch-rebase
git rebase main
# If conflicts arise, resolve them carefully, then: git add <files> && git rebase --continue

# 4. Update tracker: mark completed features as done in docs/tracker.md
#    (move summary lines to Done, update detailed status), then amend the
#    squashed commit to include the tracker changes.
git add docs/tracker.md
git commit --amend --no-edit

# 5. Fast-forward merge into main (no new commit — just moves the pointer)
git checkout main
git merge --ff-only feature/my-branch-rebase

# 6. Push and clean up
git push
git branch -d feature/my-branch-rebase
git branch -D feature/my-branch
git push origin --delete feature/my-branch
```

**Why squash first, then rebase?** Rebasing a multi-commit branch onto main can require resolving the same conflict repeatedly (once per commit). By squashing into one commit first, you only resolve conflicts once. The `git reset --soft ...` in step 1 is safe — it collapses our own feature commits back to the branch point, without touching main's state. The rebase in step 3 then does proper 3-way conflict detection against latest main.

**Handling rebase conflicts:** When `git rebase main` reports conflicts:
1. Run `git status` to see which files conflict
2. Read the conflicting files — look for `<<<<<<<`, `=======`, `>>>>>>>` markers
3. Resolve by editing to keep the correct version of each section
4. `git add <resolved-files> && git rebase --continue`
5. After rebase completes, verify the code still works (run tests)
6. **If conflicts required non-trivial edits** (e.g., integrating two features that touch the same code), ask the user for permission before completing the merge. Truly trivial conflicts (e.g., both sides added adjacent lines with no semantic interaction) can be resolved and merged without asking.

**Tracker update (step 4):** After the rebase succeeds and before merging to main, update `docs/tracker.md` to reflect completed work — move summary lines from In Progress/Todo to Done, update `**Status:**` in detailed entries. Amending the squashed commit ensures the tracker update and the code land atomically.

The squashed commit message should summarize the entire feature, not repeat individual commit messages. Always ask the user before pushing to main.

## Conversation Flow

**When the user asks a question, ONLY answer the question.** Do not continue with previous work, do not "move on." Stop and wait for the user to explicitly tell you to proceed.

## Key Constraints

- **Determinism (sim crate)**: `elven_canopy_sim` must produce identical results given the same seed. No hash-order dependence, no set iteration, no stdlib PRNG. All crates share a hand-rolled xoshiro256++ PRNG from `elven_canopy_prng` (with SplitMix64 seeding) — no external PRNG crate dependencies. This enables consistency in multiplayer and verification of optimizations. **Scope:** The strict determinism constraint (identical results across platforms/compilers) applies to `elven_canopy_sim`. The music crate uses the same PRNG for seed-based reproducibility but doesn't participate in lockstep multiplayer or replay verification.

## Simulator: Test-Driven Workflow (CRITICAL)

**Applies to:** Bug fixes and new features that affect simulator behavior.

1. **Write a failing unit test** that captures the bug or specifies the new behavior. Do NOT use `xfail`, `skip`, or any other marker — write a plain test that runs and fails.
   Confirm the new test **fails for the expected reason** — read the failure output and verify it fails because the behavior under test is wrong/missing, not because of a typo, import error, or unrelated issue.

2. **Write code** to make the test pass.
   Confirm the new test **passes** and no existing tests regress.

3. Repeat steps 1–2 as needed until the fix or feature is complete.

## Project Tracker (`docs/tracker.md`)

The tracker is the single source of truth for feature/bug status. **Read it at the start of any work session** to understand what's in progress, what's next, and what's blocked.

The tracker has two sections that must stay in sync:
1. **Summary** — one line per item inside a fenced code block, grouped by status (In Progress → Todo → Done). Format: `[status] F-id-name` padded to 23 chars, then a short title.
2. **Detailed Items** — full descriptions grouped by topic area, with design doc refs, draft doc links, and blocking relationships.

**When starting work on a tracked feature:**
1. In the summary: change `[ ]` to `[~]` and **move the line** from the Todo section into the In Progress section, maintaining alphabetical order by ID.
2. In the detailed entry: change `**Status:** Todo` to `**Status:** In Progress`.

**When completing a tracked feature:**
1. In the summary: change `[~]` to `[x]` and **move the line** from In Progress to Done, maintaining alphabetical order by ID.
2. In the detailed entry: change `**Status:** In Progress` to `**Status:** Done`.

**When adding a new feature or bug:**
1. Pick a unique `F-kebab-name` or `B-kebab-name` ID (max 20 chars). Check existing IDs to avoid collisions.
2. Add a summary line in the correct status section, **in alphabetical order by ID**, padded to the 23-char column.
3. Add a detailed entry in the appropriate topic group, **in alphabetical order by ID**, with status, phase, design doc refs, and any blocking relationships.

**Alphabetical ordering is important** — it reduces merge conflicts when multiple work streams modify the tracker in parallel. Items within each summary section (In Progress, Todo, Done) and within each detailed topic group are sorted by ID.

**Other updates:**
- When a draft design doc is created, link it from the tracker item (`**Draft:** path`).
- Blocking: use `**Blocked by:**` and `**Blocks:**` in the detailed entry. Remove resolved blockers.
- If work reveals a new bug or sub-task, add it as a new tracker item rather than leaving it as a TODO comment in code.
