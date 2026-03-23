# Codebase Patterns and Gotchas

Things that are non-obvious or surprising about the Elven Canopy codebase. Read this before diving into unfamiliar parts of the code.

## Data File Loading (CRITICAL)

**Never use runtime file I/O (`std::fs`, `FileAccess`) to load static data files** (JSON configs, lexicons, etc.). Always use `include_str!` or `include_bytes!` to embed them at compile time. Runtime paths break in exported Godot builds because `res://` points into the PCK bundle and relative paths outside it don't exist. See `elven_canopy_lang/src/lib.rs` and `elven_canopy_gdext/src/elfcyclopedia_server.rs` for examples of the correct pattern.

## Tick Rate and Sim Decoupling

- The sim runs at **1000 ticks per simulated second** (`tick_duration_ms = 1`). All tick-denominated config values (heartbeat intervals, food decay rates, species speed params) are calibrated for this rate.
- The sim is decoupled from the frame rate. `main.gd` calls `bridge.frame_update(delta)` each frame. In single-player, a `LocalRelay` on the Rust side handles tick pacing with a time-based accumulator, capped at 5000 ticks per frame to prevent spiral-of-death.
- Movement speed is per-species: `walk_ticks_per_voxel` (ticks per 1.0 units of euclidean distance on flat ground) and `climb_ticks_per_voxel` (ticks per 1.0 units on TrunkClimb/GroundToTrunk edges). Nav graph edges store euclidean distance, not time-cost — speed config is not needed for graph construction.

## Voxel Coordinate System

- Y is up. The world is (x, z) horizontal, y vertical.
- Flat array indexing: `x + z * size_x + y * size_x * size_z`. Y is the outermost axis, not the middle one.
- Terrain floor is at `floor_y` (solid `Dirt` voxels). Creatures walk on air voxels above the topmost dirt. Nav nodes start at the air layer above terrain.
- Voxel coordinates are integer corners. Renderers offset by +0.5 to center meshes/sprites on the voxel.

## Navigation Graph

- Built from the voxel world at startup, not updated incrementally. If the world changes, the nav graph must be rebuilt.
- Uses 26-connectivity (not 6) to avoid disconnecting thin geometry like radius-1 branches. Duplicate edges are avoided by only checking 13 "positive-half" neighbor offsets per node.
- A nav node exists for every air voxel that has at least one face-adjacent solid voxel (i.e., the creature is standing on or clinging to a surface).

## Tree Generation

- Trunk is just the first branch — all segments (trunk, branches, roots) use the same growth algorithm with different parameters.
- Every tree voxel must be face-connected (6-connectivity) to at least one other tree voxel. `bridge_cross_sections()` fills gaps when growth steps diagonally.
- Voxel type priority: Trunk > Branch > Root > Leaf > Air. Higher types are never overwritten by lower ones.

## GDScript UI

- All UI is built programmatically in `_ready()` methods, not in `.tscn` scene files. The scene files are mostly empty shells.
- `game_session.gd` is a Godot autoload singleton that persists seed, tree config, and player username across scene transitions (main menu → new game → game). The player name is loaded from `user://player.cfg` on startup.

## SimBridge Command Flow

- All commands (spawn, goto, build, carve, etc.) are applied immediately to the sim at the current tick in single-player. In multiplayer, commands are sent to the relay and applied when they come back in a Turn. `AdvanceTo` / `frame_update()` drives tick advancement and scheduled event processing but carries no commands.
- Build/carve validation is done upfront by the `validate_*_preview()` query methods that GDScript calls before confirming placement. The designation commands themselves are fire-and-forget.

## Sprite Rendering and Movement Interpolation

- Elf sprites are offset +0.48 in Y, capybara sprites +0.32, to visually center them above their nav node position. Selection ray-to-sprite distance uses these same offsets.
- Sprites use a pool pattern: created on demand, never destroyed, only hidden when count decreases.
- Creature positions are smoothly interpolated between nav nodes. Movement interpolation data lives in the `MoveAction` table (`move_from`/`move_to`/`move_start_tick`/`move_end_tick`), separate from the `Creature` struct. Each `Creature` has `action_kind` and `next_available_tick` fields tracking its current action. `bridge.frame_update(delta)` returns a fractional `render_tick` each frame; `main.gd` distributes it to renderers and the selection controller. `SimBridge.get_creature_positions(species, render_tick)` calls `Creature::interpolated_position()` to lerp between nav nodes using the associated `MoveAction` row.

## Input Precedence

- ESC handling flows: placement_controller (cancel placement) → construction_controller (cancel construction) → selection_controller (deselect) → pause_menu (open/close menu). Each handler calls `set_input_as_handled()` to prevent downstream handlers from firing.

## Keyboard Shortcut Assignment (CRITICAL)

- Before assigning ANY new keyboard shortcut, **thoroughly audit all existing bindings** across every GDScript file. Search for `KEY_` in `godot/scripts/` to find all current bindings. Many keys are already in use (Space, F1–F3, F12, B, T, U, M, I, Y, F, ?, P/G/L/C (construction), 1–9/Ctrl+1–9/Shift+1–9 (selection groups), ESC, Enter, Home, PgUp/PgDn, arrow keys, +/=).
- **Always ask the user** before assigning a shortcut — never pick one unilaterally.

## Dev Profile Tuning

- `Cargo.toml` sets `opt-level = 0` for the dev profile (fastest compile times). For machines that run the game with UI, override to `opt-level = 1` via `.cargo/config.toml` (gitignored) for ~4x faster sim execution at a small compile-time cost. The test profile inherits from dev.

## Code Quality Tools

All checks can be run together via `scripts/build.sh check`, but individual tools are useful for debugging CI failures:

### Rust

Workspace lint config lives in the root `Cargo.toml` under `[workspace.lints.clippy]`. Each crate inherits via `[lints] workspace = true`. Formatting config is in `rustfmt.toml` (currently all defaults).

```bash
cargo fmt --all --check       # check formatting
cargo clippy --workspace -- -D warnings   # lint
cargo fmt --all               # auto-format
```

### GDScript

GDScript files are checked with **gdformat** (formatter) and **gdlint** (linter) from the [gdtoolkit](https://github.com/Scony/godot-gdscript-toolkit) package. `scripts/build.sh check` auto-creates the venv and installs gdtoolkit if missing.

```bash
python/.venv/bin/gdformat --check --line-length 100 godot/scripts/*.gd
python/.venv/bin/gdlint godot/scripts/*.gd
python/.venv/bin/gdformat --line-length 100 godot/scripts/*.gd   # auto-format
```

`.gdlintrc` at the repo root configures gdlint. Currently disables `function-variable-name` (short names like `W`/`H` are intentional in pixel-drawing code).

## Python Tools

The `python/` directory contains offline training tools for the music generator — they are **not** part of the game runtime. **Never use `source .venv/bin/activate`** — always invoke tools via their full venv path (e.g., `python/.venv/bin/python`).

```bash
cd python && python3 -m venv .venv && .venv/bin/pip install -r requirements.txt   # One-time setup
cd python && .venv/bin/python corpus_analysis.py   # Train Markov models from Palestrina corpus → data/
cd python && .venv/bin/python rate_midi.py          # Pairwise MIDI comparison for preference model training
```
