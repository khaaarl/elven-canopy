# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Elven Canopy is a Dwarf Fortress-inspired simulation/management game set in a forest of enormous trees. The player is a **tree spirit** — the consciousness of an ancient tree — who forms a symbiotic relationship with a village of elves living on platforms, walkways, and structures grown from the tree's trunk and branches. Elves sing to the tree, and it grows in the desired shape, consuming mana. The tree provides food and shelter for the elves. Happy elves generate more mana, creating the game's central feedback loop.

**Key architectural decisions:**

- **Godot 4 + Rust via gdext.** Godot handles rendering, input, UI, and camera. All simulation logic lives in Rust.
- **Shared crates + game crates.** `elven_canopy_prng` provides a hand-rolled xoshiro256++ PRNG used by all crates (no external RNG dependencies). `elven_canopy_lang` provides shared Vaelith conlang types, vocabulary (JSON lexicon), and name generation — used by both `elven_canopy_sim` (elf names) and `elven_canopy_music` (lyrics). `elven_canopy_utils` provides shared utilities: deterministic fixed-point arithmetic (`Fixed64` scalar, `FixedVec3` 3D vector with 2^30 fractional bits) used by both `elven_canopy_sim` (projectile ballistics) and `elven_canopy_music` (composition scoring), plus parallel dedup algorithms. `elven_canopy_sim` is a pure Rust library (zero Godot dependencies) containing all simulation logic. `elven_canopy_gdext` is a thin wrapper that exposes the sim to Godot via GDExtension. `elven_canopy_music` is a standalone Palestrina-style polyphonic music generator with Vaelith (elvish) lyrics. The sim/gdext separation is enforced at the compiler level; the music crate is independent of both.
- **Deterministic simulation.** The sim is a pure function: `(state, commands) → (new_state, events)`. Hand-rolled xoshiro256++ PRNG (no external PRNG dependencies), no iterating `HashMap` (use `BTreeMap` for ordered iteration, `LookupMap` for point-query-only O(1) access), no system dependencies. Designed for future lockstep multiplayer, perfect replays, and verifiable performance optimizations.
- **Command-driven mutation.** All sim state changes go through `SimCommand`. In single-player, the GDScript glue translates UI actions into commands. In multiplayer, commands are broadcast and canonically ordered.
- **Event-driven ticks.** The sim uses a discrete event simulation with a priority queue, not fixed-timestep iteration. Empty ticks are free, enabling efficient fast-forward.
- **Voxel world, graph pathfinding.** The world is a 3D voxel grid (sim truth), but pathfinding uses a nav graph of nodes and edges matching the constrained topology (platforms, bridges, stairs, trunk surfaces).
- **Data-driven config.** All tunable parameters live in a `GameConfig` struct loaded from JSON. No magic numbers in the sim.

For full details, see `docs/design_doc.md`. Note that the design doc is an aspirational planning document — many features it describes (construction, structural integrity, fire, emotional systems, etc.) are not yet implemented. See `docs/tracker.md` for current feature status.

## Implementation Status

Phase 0–1 complete (foundations, tree, 12 species with procedural sprites). Phase 2 partial (construction loop, save/load, chunk mesh rendering, diagonal support struts, basic mana economy). Phase 3 complete (projectiles, melee, ranged, HP/death with incapacitation/bleed-out, hostile AI, flee, attack-move, RTS selection, military groups, friendly-fire avoidance, voxel exclusion, armor damage reduction with equipment degradation, debug enemy raids, troll HP regeneration). Phase 4 partial (cooking, manufacturing, mood, notifications, logistics, proximity tasks). Phase 6 complete (music crate, lang crate, elfcyclopedia). Phase 7 partial (fruit sprites/generation/cultivation, extraction, component recipes, item colors, equipment sprite overlays). Phase 5 partial (double-click group select, Ctrl+1–9 selection groups, shift+right-click command queuing with queue survival). Phase 8 partial (multi-session relay, elf melee weapons — spear/club with reach-based selection and degradation). Creature biology in progress (biological traits as sim data, trait-driven sprite generation). Creature stats done (8 stats with exponential scaling, wired to melee/speed/HP/arrows). Creature skills in progress (17 skill TraitKind variants, info panel Skills tab; advancement and effect wiring deferred). Tabulosity (sim DB) complete and integrated (38-table SimDb, hash-based indexes). Flying creatures in progress (3D flight A* on voxel grid with footprint clearance, giant hornet 1×1×1 and wyvern 2×2×2 with procedural sprites and aggressive AI, unified task-based activation pipeline — GoTo/AttackMove/AttackTarget/arrow-chase all work for flyers). Creature gravity done (unsupported creatures fall with configurable damage).

For detailed per-feature status, see `docs/implementation_status.md` and `docs/tracker.md`.

## Project Structure

Top-level crates: `elven_canopy_sim` (pure Rust sim), `elven_canopy_gdext` (GDExtension bridge), `elven_canopy_sprites` (procedural sprite generation, pure Rust RGBA8 buffers), `elven_canopy_music` (polyphonic music generator), `elven_canopy_lang` (Vaelith conlang), `elven_canopy_prng` (shared PRNG), `elven_canopy_utils` (shared utilities: fixed-point math, parallel dedup), `elven_canopy_protocol` (multiplayer wire protocol), `elven_canopy_relay` (multiplayer relay server + client), `multiplayer_tests` (integration tests for relay pipeline), `tabulosity`/`tabulosity_derive` (in-memory relational store). Godot project in `godot/` (scenes + scripts). Data files in `data/`. Python offline tools in `python/`. Docs in `docs/`. Build scripts in `scripts/`.

For the full annotated directory tree, see `docs/project_structure.md`.

## Building and Running

Use `scripts/build.sh` for all build operations. It ensures the `godot/target` symlink exists before compiling.

```bash
scripts/build.sh            # Debug build
scripts/build.sh release    # Release build
scripts/build.sh test       # Run all crate tests
scripts/build.sh quicktest  # Test only crates changed vs main
scripts/build.sh gdtest     # Run GDScript unit tests (GUT)
scripts/build.sh relay      # Optimized standalone relay binary (LTO, stripped)
scripts/build.sh run        # Debug build, then launch the game
scripts/build.sh run-branch NAME  # Fetch, checkout branch, sync to remote, build+run
```

Individual crate tests: `cargo test -p elven_canopy_sim`, `cargo test -p elven_canopy_lang`, `cargo test -p elven_canopy_music`, `cargo test -p tabulosity -p tabulosity_derive`. Tabulosity serde tests: `cargo test -p tabulosity --features serde --test serde`. Music CLI: `cargo run -p elven_canopy_music -- --help`.

**Targeted clean:** If stale build artifacts cause errors (e.g., after force-pushes or branch switches), clean only the project crates — not the entire target directory: `cargo clean -p elven_canopy_sim -p elven_canopy_gdext`. This preserves the cached `godot` crate build, which is very slow to recompile.

### Python Tools

The `python/` directory contains offline training tools for the music generator — they are **not** part of the game runtime. **Never use `source .venv/bin/activate`** — always invoke tools via their full venv path (e.g., `python/.venv/bin/python`).

## Toolchain Versions

- **Rust edition:** 2024
- **gdext crate:** `godot` 0.4.5 with feature `api-4-5`
- **Godot:** 4.6 (forward-compatible with the 4.5 API)

When upgrading the `godot` crate, check for a matching `api-4-x` feature flag. The API version must be ≤ the Godot runtime version.

## Code Quality Tools

`cargo fmt`, `cargo clippy`, `cargo test`, `gdformat`, and `gdlint` are all enforced in CI via `.github/workflows/ci.yml`. Run all checks locally with `scripts/build.sh check`. Workspace lint config lives in the root `Cargo.toml` under `[workspace.lints.clippy]`. GDScript uses gdtoolkit (`gdformat`/`gdlint`); `.gdlintrc` at repo root configures gdlint.

## Running Commands

The repo's `.claude/settings.json` sets `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1`, which resets the Bash tool's working directory to the project root before every command. This means you never need to worry about working directory drift — just write commands relative to the repo root.

**Keep Bash commands simple.** Do not use `source`, command substitution (`$(...)` or backticks), heredocs (`<<EOF`), shell variables, or other shell tricks. These trigger unnecessary permission prompts. Also avoid putting flag names inside quotes (e.g., `git show --stat "--format="` can trigger a "quoted flag names" permission check) — keep flags as bare arguments. Use the dedicated Read/Write/Edit tools for file operations. For `git commit`, always use the `.tmp/commit-msg.txt` + `git commit -F` approach described in the "Committing Code" section.

**Preserve slow command output.** Commands that compile, test, format, lint, or are otherwise slow must capture output via `tee` to `.tmp/` (e.g., `scripts/build.sh check 2>&1 | tee .tmp/check.txt | grep error`). Grep or read the file afterward instead of re-running the command.

## Scratch Files

Use `.tmp/` in the repo root (gitignored) for any temporary files — benchmark output, intermediate data, scratch scripts, etc. It always exists. **Do NOT use `/tmp`** — it can trigger permission prompts and isn't project-scoped.

## Module Docstrings

Every code file should have a top-level comment that helps someone new to the codebase orient themselves. Cover: what the file does, how it fits into the system (reference sibling files with extensions), notable algorithms, and critical constraints (e.g., determinism). Keep it proportional to the file's complexity.

When making changes to a file, consider whether documentation elsewhere needs updating — module docstrings in sibling files that reference the changed module, the architecture overview in this file, etc.

## Codebase Patterns and Gotchas

For the full list of codebase patterns, conventions, and gotchas, see `docs/codebase_patterns.md`. The most critical items are duplicated here:

**Data file loading (CRITICAL):**
- **Never use runtime file I/O (`std::fs`, `FileAccess`) to load static data files.** Always use `include_str!` or `include_bytes!` to embed at compile time. Runtime paths break in exported Godot builds.

**Keyboard shortcut assignment (CRITICAL):**
- Before assigning ANY new keyboard shortcut, **thoroughly audit all existing bindings** across every GDScript file. Search for `KEY_` in `godot/scripts/`. Many keys are already in use.
- **Always ask the user** before assigning a shortcut — never pick one unilaterally.

**Godot ScrollContainer sizing (CRITICAL):**
- Before writing ANY code involving `ScrollContainer`, **read `docs/godot_scroll_sizing.md` in full.** Your built-in understanding of scroll container sizing is wrong.

**Voxel coordinate system:** Y is up. Flat array indexing: `x + z * size_x + y * size_x * size_z`. Terrain floor at `config.floor_y` (default 50), creatures walk at `floor_y + 1`. Renderers offset by +0.5.

**"Pull main":** When asked to pull/update/rebase on main, first update the local ref: `git fetch origin main:main` (if not on main) or `git pull` (if on main). A stale local main causes wrong diffs.

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

**Remote testing (CRITICAL):** The user tests on a different machine. Any time you tell the user to build, run, or test something, you MUST commit and push first. Code that isn't pushed doesn't exist from the user's perspective.

**Pre-commit checks (CRITICAL):** Before every commit that includes code changes (Rust or GDScript), run `scripts/build.sh check` and fix any issues. Do NOT commit code that fails formatting or linting. For commits that change Rust code, also run `scripts/build.sh quicktest` and ensure all tests pass. For commits that change GDScript code, also run `scripts/build.sh gdtest` and ensure all GDScript unit tests pass. Non-code changes (e.g., docs, config, CLAUDE.md) can skip these steps.

**Commit message procedure:** Always write the commit message to `.tmp/commit-msg.txt` using the Write tool, then commit with `-F`:

```bash
git commit -F .tmp/commit-msg.txt
rm .tmp/commit-msg.txt
```

This applies to all commits — single-line and multi-line alike. Do NOT use `-m` flags, command substitution, heredocs, or shell variables to build commit messages.

## The Once-Over

When a feature branch's work is done, use `/once-over` for a final quality review. It spawns four parallel review agents (code quality, test coverage, corner cases, spec adherence). Agent instructions live in `docs/once-over/`.

## Merging to Main

When the user asks to merge a feature branch to main, use the `/merge-to-main` slash command. It follows a squash-rebase-ff workflow that keeps main's history clean. The entire procedure is delegated to a subagent to keep the main context clean. See `.claude/commands/merge-to-main.md` for the full procedure.

## Conversation Flow (CRITICAL)

**Default to talking, not doing.** You are far too proactive by default. When in doubt, respond with text and wait for an explicit instruction to act. This is one of the most important rules in this file.

**Questions:** When the user asks a question, ONLY answer the question. Do not continue with previous work, do not "move on." Stop and wait for the user to explicitly tell you to proceed.

**Design and planning discussions:** When the user is discussing design, brainstorming, planning, or giving feedback on a sketch — respond with text. Do NOT start editing files, writing code, or updating the tracker. Phrases like "let's do X", "we should add Y", "I'm envisioning Z" in a design conversation are the user thinking out loud, not giving you an edit instruction. Stay in the conversation until the user explicitly asks you to implement, write, edit, or create something. Even then, confirm scope before starting if the request is ambiguous.

**When to act:** Only start editing files or running commands when the user gives a clear, unambiguous instruction to do so — e.g., "implement this", "write that test", "update the tracker", "make a branch and do X". If you're not sure whether the user wants you to act or keep discussing, ask.

**Never assume your own code is correct.** When the user requests verification (once-over, review, additional testing), run it without pushback, even if you just ran a similar check. Multiple rounds of review catch different issues. Do not say "we just did that" or "the branch is clean" as a reason to skip requested verification. The user's judgment about how much checking is needed overrides your confidence in the code.

## Planning

**Never use EnterPlanMode/ExitPlanMode unless explicitly requested.** When a task is large enough to warrant significant planning, write the plan to `.tmp/plan-<name>.md` so it survives context compaction and restarts, summarize it in conversation, and wait for approval before implementing.

**TDD audit:** After writing a plan, audit it for TDD compliance before presenting it. Every implementation step that changes behavior must be preceded by a failing test in the plan. For simulator work (`elven_canopy_sim`), this is mandatory — do not present a plan that batches tests at the end. For other crates, TDD ordering is strongly recommended but not blocking.

## Key Constraints

- **No silent deferrals.** If you notice a bug or issue, even if out of current scope of work or a pre-existing issue, you MUST alert the user LOUDLY. DO NOT merge to main unless you have fixed it and/or added a tracker bug. NO MERCY FOR BUGS.

- **Determinism (sim crate)**: `elven_canopy_sim` must produce identical results given the same seed. No hash-order dependence, no set iteration, no floating-point arithmetic (precision varies across platforms/compilers), no stdlib PRNG. All crates share a hand-rolled xoshiro256++ PRNG from `elven_canopy_prng` (with SplitMix64 seeding) — no external PRNG crate dependencies. This enables consistency in multiplayer and verification of optimizations. **Scope:** The strict determinism constraint (identical results across platforms/compilers) applies to `elven_canopy_sim`. The music crate uses the same PRNG for seed-based reproducibility but doesn't participate in lockstep multiplayer or replay verification.

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

## Test Robustness (CRITICAL)

**No flaky tests.** Every test must pass deterministically, every time.

1. **Trust failures.** A failing test is a real bug — investigate, don't re-run.
2. **Resilient assertions.** Don't assert exact pseudorandom worldgen values — assert structural properties (counts, bounds, ordering). Don't use wall-clock timing. Set config values explicitly rather than relying on defaults. Build minimal test worlds, not shared fixtures.

## GDScript: Unit Testing with GUT

GDScript unit tests use the [GUT](https://github.com/bitwes/Gut) (Godot Unit Test) framework. Tests live in `godot/test/test_*.gd` and extend `GutTest`. Run them with `scripts/build.sh gdtest`.

**GDScript work requires unit tests.** When adding or modifying GDScript logic, write tests for any behavior that can be tested without a running game — coordinate math, UI state machines, formatting helpers, selection logic, input mode transitions, data transformations. If testable logic is embedded in scene-dependent code, extract it into a utility class (e.g., `geometry_utils.gd`) so it can be tested in isolation. The bar is the same as for Rust: if you're adding behavior, prove it works with a test.

**Test file naming:** `godot/test/test_<module>.gd` — mirrors the source file in `godot/scripts/`.

**Pre-commit:** For commits that change `.gd` files, `scripts/build.sh check` already covers formatting and linting. Also run `scripts/build.sh gdtest` to verify GDScript tests pass (analogous to `quicktest` for Rust).

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
