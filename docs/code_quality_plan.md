# Code Quality Plan

Phased rollout of linting, formatting, and static analysis for both Rust and
GDScript. Each tier builds on the previous one. Within a tier the Rust and
GDScript tracks are independent and can land in either order.

The plan deliberately avoids specifying which source files need changes — other
work is happening in parallel, so the concrete fixups will be done at
implementation time. The focus here is on **tooling and configuration**.

---

## Tier 1 — Formatting & Basic Linting

Goal: enforced, deterministic style and a baseline lint pass across the whole
codebase.

### Rust

1. **Add `rustfmt.toml`** at the repo root. Start with defaults; override
   only if there's a strong preference (e.g. `imports_granularity = "Crate"`).
2. **Run `cargo fmt --all`** once to reformat the entire workspace. This is a
   single mechanical commit with no behavioral changes.
3. **Add workspace lint configuration** to `Cargo.toml`:
   ```toml
   [workspace.lints.clippy]
   all = { level = "warn", priority = -1 }
   ```
   Each crate's `Cargo.toml` inherits with `[lints] workspace = true`.
4. **Run `cargo clippy --workspace`**, fix all warnings, and commit.
5. **Add a `check` mode to `build.sh`** that runs:
   ```
   cargo fmt --all --check
   cargo clippy --workspace -- -D warnings
   ```
6. **Add a `lint` job to CI** (`.github/workflows/build-and-package.yml` or a
   new lightweight workflow) that runs the same two commands. This job needs
   only Rust — no Godot — so it's fast and cheap.

### GDScript

1. **Add `gdtoolkit` to `python/requirements-dev.txt`** (new file):
   ```
   gdtoolkit>=4.3
   ```
   This sits alongside the existing `python/requirements.txt` (runtime music
   tools). The venv setup in `python/` already works; dev deps just use a
   different requirements file.
2. **Add a `.gdlintrc`** at the repo root with reasonable defaults. Tune
   `max-line-length` and disable any rules that conflict with the project's
   programmatic-UI-in-`_ready()` pattern.
3. **Run `gdformat` on all `.gd` files** once — single mechanical commit.
4. **Run `gdlint`**, fix warnings, commit.
5. **Extend the `check` mode in `build.sh`** (or the CI lint job) to also run:
   ```
   gdformat --check godot/scripts/*.gd
   gdlint godot/scripts/*.gd
   ```
6. **Update CLAUDE.md** with instructions for setting up and running the dev
   tools (see "CLAUDE.md Updates" below).

---

## Tier 2 — Stricter Lints & Static Analysis

Goal: catch deeper issues — unsafe code in the sim, missing types in GDScript,
broken references in Godot scenes.

### Rust

1. **Enable Clippy pedantic** at the workspace level:
   ```toml
   [workspace.lints.clippy]
   all = { level = "warn", priority = -1 }
   pedantic = { level = "warn", priority = -1 }
   # Selectively allow noisy pedantic lints:
   # module_name_repetitions = "allow"
   # must_use_candidate = "allow"
   # ... tune as needed
   ```
2. **Fix pedantic warnings** across the workspace.
3. **Deny `unsafe_code`** in the sim crate specifically — it has a determinism
   contract and should never need unsafe:
   ```toml
   # elven_canopy_sim/Cargo.toml
   [lints.rust]
   unsafe_code = "deny"
   ```
4. **Warn on `missing_docs`** for the sim crate's public API. The sim is the
   core library consumed by gdext; its public surface should be documented.

### GDScript

1. **Add Godot's built-in static check to CI:**
   ```
   godot --headless --check-only --path godot
   ```
   This runs the parser and type checker across all scripts — catches undefined
   references, type mismatches, and missing signals. The CI already installs
   Godot for exports, so this is free.
2. **Tighten `.gdlintrc`** — review which rules were disabled in Tier 1 and
   re-enable any that are now feasible.

---

## Tier 3 — Hardening & Conventions

Goal: prevent regressions and establish long-term conventions.

### Rust

1. **Promote all Clippy warnings to errors in CI** (`-D warnings` is already
   there from Tier 1, but ensure pedantic is also covered).
2. **Consider `cargo deny`** for dependency auditing — license checks,
   duplicate crate detection, advisory database scanning. Lower priority but
   valuable as the dependency tree grows.

### GDScript

1. **Adopt static typing conventions** — type all function signatures
   (`func foo(x: int) -> void`), gradually type local variables. gdlint
   doesn't enforce this, but it improves editor autocompletion and catches bugs
   at parse time.
2. **Tighten `gdlint` further** based on experience — e.g. max function
   length, max file length, naming conventions.

---

## CLAUDE.md Updates (part of Tier 1)

Add a section covering the dev tooling setup and how to run checks. Something
like:

```markdown
## Code Quality Tools

### Rust

`cargo fmt` and `cargo clippy` are enforced in CI. Run locally:

    scripts/build.sh check    # fmt --check + clippy

Workspace lint config lives in the root `Cargo.toml` under
`[workspace.lints]`. Each crate inherits via `[lints] workspace = true`.

### GDScript

GDScript linting uses [gdtoolkit](https://github.com/Scony/godot-gdscript-toolkit),
installed as a Python dev dependency. Setup:

    cd python && python -m venv .venv && source .venv/bin/activate
    pip install -r requirements-dev.txt

Run:

    gdformat --check godot/scripts/*.gd
    gdlint godot/scripts/*.gd

Both checks also run in CI and via `scripts/build.sh check`.
```

Note: the Python venv in `python/` serves double duty — `requirements.txt` for
the music generator's runtime deps, `requirements-dev.txt` for dev tooling
(gdtoolkit). Both can be installed into the same venv.

---

## Implementation Order

Each numbered item above is roughly one commit on a feature branch. The
recommended order within a tier:

1. Add config files and `requirements-dev.txt` (no code changes)
2. Mechanical formatting commits (`cargo fmt`, `gdformat`)
3. Lint fixes
4. CI / build.sh integration
5. CLAUDE.md updates

Tiers should be done sequentially (Tier 1 fully landed before starting
Tier 2). Within a tier, Rust and GDScript tracks can proceed in parallel.
