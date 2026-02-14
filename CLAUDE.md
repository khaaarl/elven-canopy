# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Elven Canopy is a Dwarf Fortress-inspired simulation/management game set in a forest of enormous trees. The player is a **tree spirit** — the consciousness of an ancient tree — who forms a symbiotic relationship with a village of elves living on platforms, walkways, and structures grown from the tree's trunk and branches. Elves sing to the tree, and it grows in the desired shape, consuming mana. The tree provides food and shelter for the elves. Happy elves generate more mana, creating the game's central feedback loop.

**Key architectural decisions:**

- **Godot 4 + Rust via gdext.** Godot handles rendering, input, UI, and camera. All simulation logic lives in Rust.
- **Two Rust crates.** `elven_canopy_sim` is a pure Rust library (zero Godot dependencies) containing all simulation logic. `elven_canopy_gdext` is a thin wrapper that exposes the sim to Godot via GDExtension. This separation is enforced at the compiler level.
- **Deterministic simulation.** The sim is a pure function: `(state, commands) → (new_state, events)`. Seeded ChaCha20 PRNG, no `HashMap` (use `BTreeMap`), no system dependencies. Designed for future lockstep multiplayer, perfect replays, and verifiable performance optimizations.
- **Command-driven mutation.** All sim state changes go through `SimCommand`. In single-player, the GDScript glue translates UI actions into commands. In multiplayer, commands are broadcast and canonically ordered.
- **Event-driven ticks.** The sim uses a discrete event simulation with a priority queue, not fixed-timestep iteration. Empty ticks are free, enabling efficient fast-forward.
- **Voxel world, graph pathfinding.** The world is a 3D voxel grid (sim truth), but pathfinding uses a nav graph of nodes and edges matching the constrained topology (platforms, bridges, stairs, trunk surfaces).
- **Data-driven config.** All tunable parameters live in a `GameConfig` struct loaded from JSON. No magic numbers in the sim.

For full details, see `elven_canopy_design_doc_v2.md`.

## Running Commands

The repo's `.claude/settings.json` sets `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1`, which resets the Bash tool's working directory to the project root before every command. This means you never need to worry about working directory drift — just write commands relative to the repo root.

## Scratch Files

Use `.tmp/` in the repo root (gitignored) for any temporary files — benchmark output, intermediate data, scratch scripts, etc. Always `mkdir -p .tmp` before writing. **Do NOT use `/tmp`** — it can trigger permission prompts and isn't project-scoped.

## Module Docstrings

Every code file should have a top-level comment that helps someone new to the codebase orient themselves. Cover:

- **What the file does** — its purpose and scope.
- **How it fits into the system** — which sibling files it delegates to or depends on, and what role it plays in the larger architecture. Use file extensions when referencing files (e.g., ``tempering.py``, not ``tempering``) so it's clear these are files, not abstract concepts.
- **Notable or surprising algorithms** — anything non-obvious that a reader might need context for (e.g., angular-sweep visibility, OBB collision via SAT).
- **Critical constraints** — if the file is subject to the determinism requirement, say so explicitly. A newcomer who doesn't know about the requirement can easily break it.

Keep it proportional to the file's complexity. A 50-line utility doesn't need a paragraph; an 800-line engine core does. Test files can be brief.

When making changes to a file, consider whether documentation elsewhere needs updating — module docstrings in sibling files that reference the changed module, the architecture overview in this file, etc. A renamed function or shifted responsibility can leave other files' docstrings silently wrong.

## Branching (CRITICAL — DO THIS FIRST)

**NEVER make code changes directly on `main` without explicit user permission.** Before writing ANY code, you MUST:

1. Create a feature branch: `git checkout -b feature/descriptive-branch-name`
2. Verify you are on the feature branch: `git branch --show-current`
3. ONLY THEN start making changes

**This is non-negotiable.** If you realize you are on `main` and have already made changes, STOP immediately and ask the user how to proceed — do NOT commit to `main`.

The only exception is editing `CLAUDE.md` itself, which can be done on `main` if explicitly requested. However, do NOT commit or push CLAUDE.md changes until the user explicitly says to — they may want to review or iterate first.

## Committing Code

ALWAYS ASK FOR PERMISSION BEFORE COMMITTING TO MAIN/MASTER, BUT COMMITTING TO FEATURE BRANCHES DOES NOT REQUIRE PERMISSION.

## Merging to Main

When the user asks to merge a feature branch to main, follow this procedure:

```bash
# 1. Create a temporary branch and squash all feature commits into one
#    (This way conflicts only need to be resolved once, not per-commit)
#    IMPORTANT: The REAL commit message goes HERE — step 4 is a fast-forward
#    merge which does NOT create a new commit, so any -m there is ignored.
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

# 4. Fast-forward merge into main (no new commit — just moves the pointer)
git checkout main
git merge --ff-only feature/my-branch-rebase

# 5. Push and clean up
git push
git branch -d feature/my-branch-rebase
git branch -D feature/my-branch
```

**Why squash first, then rebase?** Rebasing a multi-commit branch onto main can require resolving the same conflict repeatedly (once per commit). By squashing into one commit first, you only resolve conflicts once. The `git reset --soft ...` in step 1 is safe — it collapses our own feature commits back to the branch point, without touching main's state. The rebase in step 3 then does proper 3-way conflict detection against latest main.

**Handling rebase conflicts:** When `git rebase main` reports conflicts:
1. Run `git status` to see which files conflict
2. Read the conflicting files — look for `<<<<<<<`, `=======`, `>>>>>>>` markers
3. Resolve by editing to keep the correct version of each section
4. `git add <resolved-files> && git rebase --continue`
5. After rebase completes, verify the code still works (run tests)
6. **If conflicts required non-trivial edits** (e.g., integrating two features that touch the same code), ask the user for permission before completing the merge. Truly trivial conflicts (e.g., both sides added adjacent lines with no semantic interaction) can be resolved and merged without asking.

The squashed commit message should summarize the entire feature, not repeat individual commit messages. Always ask the user before pushing to main.

## Key Constraints

- **Determinism**: The simulator must produce identical results given the same seed. No hash-order dependence, no set iteration, no stdlib PRNG (use a portable PRNG like PCG/xoshiro implemented from scratch). This enables consistency in multiplayer and verification of optimizaitons.

## Simulator: Test-Driven Workflow (CRITICAL)

**Applies to:** Bug fixes and new features that affect simulator behavior.

1. **Write a failing unit test** that captures the bug or specifies the new behavior. Do NOT use `xfail`, `skip`, or any other marker — write a plain test that runs and fails.
   Confirm the new test **fails for the expected reason** — read the failure output and verify it fails because the behavior under test is wrong/missing, not because of a typo, import error, or unrelated issue.

2. **Write code** to make the test pass.
   Confirm the new test **passes** and no existing tests regress.

3. Repeat steps 1–2 as needed until the fix or feature is complete.
