#!/usr/bin/env bash
# Build script for Elven Canopy.
#
# Compiles the Rust GDExtension library (debug or release) and ensures the
# godot/target symlink exists so Godot can find the compiled .so/.dll/.dylib.
#
# Usage:
#   scripts/build.sh            # debug build
#   scripts/build.sh release    # release build
#   scripts/build.sh test       # run all crate tests
#   scripts/build.sh quicktest  # test only crates changed vs main
#   scripts/build.sh run        # debug build then launch the game
#   scripts/build.sh check      # run fmt, clippy, gdformat, gdlint checks
#   scripts/build.sh coverage  # generate HTML code coverage report (requires cargo-llvm-cov)
#   scripts/build.sh run-branch NAME  # fetch, checkout branch, sync to remote, build+run
#                                       NAME can be exact or partial (tries feature/ and bug/ prefixes)
#
# Run from the repo root.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

MODE="${1:-debug}"

# --- Find the Godot binary ----------------------------------------------------
# Snap installs as godot-4, some systems use godot4, others just godot.

find_godot() {
    for CMD in godot-4 godot4 godot; do
        if command -v "$CMD" &>/dev/null; then
            echo "$CMD"
            return
        fi
    done
    echo ""
}

GODOT="$(find_godot)"

# --- Ensure the godot/target symlink exists -----------------------------------

LINK="godot/target"
if [ ! -L "$LINK" ]; then
    echo "Creating symlink $LINK -> ../target"
    ln -sf ../target "$LINK"
elif [ "$(readlink "$LINK")" != "../target" ]; then
    echo "Fixing symlink $LINK -> ../target"
    rm "$LINK"
    ln -sf ../target "$LINK"
fi

# --- Import Godot project on first run ----------------------------------------
# The .godot/ directory (gitignored) contains the import cache and extension
# registry. On a fresh clone it doesn't exist, so Godot can't find the
# GDExtension classes. Running --import --headless creates it. The editor
# may crash after importing (known Godot bug in headless mode), but the
# side effect — creating .godot/ — is all we need, so we suppress errors.

ensure_godot_imported() {
    if [ ! -d "$REPO_ROOT/godot/.godot" ]; then
        echo "First run: importing Godot project..."
        "$GODOT" --path "$REPO_ROOT/godot" --headless --import --quit &>/dev/null || true
    fi
}

# --- GDScript parse check -----------------------------------------------------
# Launches Godot to the main menu (which eagerly loads all scripts via
# _preload_all_scripts in main_menu.gd), then immediately quits. Uses
# xvfb-run to provide a virtual display so GDExtension loads properly.
# Grep the output for SCRIPT ERROR to detect parse failures.

godot_script_check() {
    if [ -z "$GODOT" ]; then
        echo "Error: Godot not found (tried godot-4, godot4, godot)" >&2
        exit 1
    fi
    echo "Building elven_canopy_gdext for GDScript check..."
    cargo build -p elven_canopy_gdext
    ensure_godot_imported
    echo "Checking GDScript parse validity..."
    OUTPUT="$(xvfb-run "$GODOT" --path "$REPO_ROOT/godot" --quit 2>&1)" || true
    if printf '%s' "$OUTPUT" | grep -q "SCRIPT ERROR"; then
        echo "$OUTPUT" >&2
        echo "" >&2
        echo "GDScript parse check failed!" >&2
        exit 1
    fi
    echo "GDScript parse check passed."
}

# --- Build --------------------------------------------------------------------

case "$MODE" in
    debug)
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext
        echo "Done. Run: cd godot && godot"
        ;;
    release)
        echo "Building elven_canopy_gdext (release)..."
        cargo build -p elven_canopy_gdext --release
        echo "Done. Run: cd godot && godot"
        ;;
    test)
        # Tabulosity tests run in a separate invocation to avoid Cargo feature
        # unification: elven_canopy_sim depends on tabulosity with features=["serde"],
        # which would activate the serde feature for tabulosity's own test targets,
        # breaking test structs that don't derive Serialize/Deserialize.
        echo "Running tabulosity tests..."
        cargo test -p tabulosity -p tabulosity_derive -- --test-threads=16
        echo ""
        echo "Running tabulosity serde tests..."
        cargo test -p tabulosity --features serde --test serde -- --test-threads=16
        echo ""
        ALL_TEST_PACKAGES="-p elven_canopy_prng -p elven_canopy_lang -p elven_canopy_sim -p elven_canopy_protocol -p elven_canopy_relay -p elven_canopy_music -p multiplayer_tests"
        echo "Running all other crate tests..."
        cargo test $ALL_TEST_PACKAGES -- --test-threads=16
        echo ""
        godot_script_check
        echo ""
        echo "All tests passed."
        ;;
    quicktest)
        # Test only crates with changes: committed (vs main), staged, or unstaged.
        BRANCH_CHANGES="$(git diff --name-only main...HEAD 2>/dev/null || true)"
        STAGED_CHANGES="$(git diff --name-only --cached 2>/dev/null || true)"
        UNSTAGED_CHANGES="$(git diff --name-only 2>/dev/null || true)"
        CHANGED_FILES="$(printf '%s\n%s\n%s' "$BRANCH_CHANGES" "$STAGED_CHANGES" "$UNSTAGED_CHANGES" | sort -u)"
        # Tabulosity tests run separately to avoid Cargo feature unification
        # (see the 'test' target comment for details).
        TAB_PACKAGES=""
        OTHER_PACKAGES=""
        for CRATE_DIR in elven_canopy_prng elven_canopy_lang elven_canopy_sim elven_canopy_protocol elven_canopy_relay elven_canopy_music tabulosity tabulosity_derive; do
            if printf '%s' "$CHANGED_FILES" | grep -q "^${CRATE_DIR}/"; then
                if [ "$CRATE_DIR" = "tabulosity" ] || [ "$CRATE_DIR" = "tabulosity_derive" ]; then
                    TAB_PACKAGES="$TAB_PACKAGES -p $CRATE_DIR"
                else
                    OTHER_PACKAGES="$OTHER_PACKAGES -p $CRATE_DIR"
                fi
            fi
        done
        HAS_RUST_CHANGES=""
        if printf '%s' "$CHANGED_FILES" | grep -q '\.rs$\|Cargo\.toml$'; then
            HAS_RUST_CHANGES="1"
        fi
        if [ -n "$TAB_PACKAGES" ]; then
            echo "Running tabulosity tests:$TAB_PACKAGES"
            cargo test $TAB_PACKAGES -- --test-threads=16
            echo ""
            echo "Running tabulosity serde tests..."
            cargo test -p tabulosity --features serde --test serde -- --test-threads=16
            echo ""
        fi
        if [ -n "$OTHER_PACKAGES" ]; then
            # Include multiplayer_tests alongside changed crates for cross-crate coverage.
            OTHER_PACKAGES="$OTHER_PACKAGES -p multiplayer_tests"
            echo "Running tests for:$OTHER_PACKAGES"
            cargo test $OTHER_PACKAGES -- --test-threads=16
            echo ""
        elif [ -n "$HAS_RUST_CHANGES" ]; then
            # Only Cargo.toml or non-crate .rs files changed — still run multiplayer tests.
            echo "Running multiplayer tests..."
            cargo test -p multiplayer_tests -- --test-threads=16
            echo ""
        else
            echo "No Rust changes detected, skipping Rust tests."
        fi
        if printf '%s' "$CHANGED_FILES" | grep -q '\.gd$'; then
            godot_script_check
            echo ""
        fi
        echo "All tests passed."
        ;;
    run)
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext
        ensure_godot_imported
        echo "Launching Elven Canopy..."
        RUST_BACKTRACE=1 "$GODOT" --path "$REPO_ROOT/godot"
        ;;
    run-branch)
        BRANCH_NAME="${2:-}"
        if [ -z "$BRANCH_NAME" ]; then
            echo "Usage: scripts/build.sh run-branch <branch-name>" >&2
            echo "  branch-name can be exact (feature/F-foo) or partial (F-foo)" >&2
            exit 1
        fi

        echo "Fetching latest from origin..."
        git fetch --prune

        # Resolve branch name: try exact, then feature/, then bug/ prefix
        RESOLVED=""
        ALL_BRANCHES="$(git branch -a --format='%(refname:short)')"
        for CANDIDATE in "$BRANCH_NAME" "feature/$BRANCH_NAME" "bug/$BRANCH_NAME"; do
            if printf '%s\n' "$ALL_BRANCHES" | grep -qxF "$CANDIDATE"; then
                RESOLVED="$CANDIDATE"
                break
            fi
            # Also check origin/ remotes (for branches not yet checked out locally)
            if printf '%s\n' "$ALL_BRANCHES" | grep -qxF "origin/$CANDIDATE"; then
                RESOLVED="$CANDIDATE"
                break
            fi
        done

        if [ -z "$RESOLVED" ]; then
            echo "Error: no branch found matching '$BRANCH_NAME'" >&2
            echo "Tried: $BRANCH_NAME, feature/$BRANCH_NAME, bug/$BRANCH_NAME" >&2
            echo "" >&2
            echo "Available branches:" >&2
            git branch -a --format='%(refname:short)' | grep -E "^(feature|bug)/" | sort >&2 || true
            exit 1
        fi

        CURRENT_BRANCH="$(git branch --show-current)"
        if [ "$CURRENT_BRANCH" != "$RESOLVED" ]; then
            echo "Switching to $RESOLVED..."
            git checkout "$RESOLVED"
        else
            echo "Already on $RESOLVED."
        fi

        if [ "$RESOLVED" = "main" ]; then
            # On main: simple pull (fail on conflicts rather than force)
            git pull
        else
            # Update local main ref without checking it out
            git fetch origin main:main

            # Update to match remote if needed (handles rebases/force-pushes)
            LOCAL="$(git rev-parse HEAD)"
            REMOTE="$(git rev-parse "origin/$RESOLVED")"
            if [ "$LOCAL" != "$REMOTE" ]; then
                echo "Updating to $(echo "$REMOTE" | head -c 8)..."
                git reset --hard "origin/$RESOLVED"
            else
                echo "Already up to date."
            fi
        fi

        echo ""
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext
        ensure_godot_imported
        echo "Launching Elven Canopy..."
        RUST_BACKTRACE=1 "$GODOT" --path "$REPO_ROOT/godot"
        ;;
    coverage)
        if ! command -v cargo-llvm-cov &>/dev/null; then
            echo "cargo-llvm-cov not found. Install with: cargo install cargo-llvm-cov" >&2
            exit 1
        fi
        # Tabulosity runs separately to avoid Cargo feature unification
        # (elven_canopy_sim activates tabulosity's serde feature).
        echo "Running tabulosity coverage..."
        cargo llvm-cov --no-report -p tabulosity -p tabulosity_derive -- --test-threads=16
        echo ""
        echo "Running other crate coverage..."
        cargo llvm-cov --no-report --workspace --exclude elven_canopy_gdext --exclude tabulosity --exclude tabulosity_derive -- --test-threads=16
        echo ""
        echo "Generating HTML report..."
        cargo llvm-cov report --html --output-dir target/llvm-cov
        echo ""
        echo "Generating LCOV file..."
        cargo llvm-cov report --lcov --output-path target/llvm-cov/lcov.info
        echo ""
        echo "Coverage report: target/llvm-cov/html/index.html"
        echo "LCOV file:       target/llvm-cov/lcov.info"
        ;;
    check)
        echo "Checking Rust formatting..."
        cargo fmt --all --check
        echo ""
        echo "Running Clippy..."
        cargo clippy --workspace -- -D warnings
        echo ""
        # Ensure gdformat/gdlint are available, set up venv if not
        GDFORMAT="$REPO_ROOT/python/.venv/bin/gdformat"
        GDLINT="$REPO_ROOT/python/.venv/bin/gdlint"
        if [ ! -x "$GDFORMAT" ] || [ ! -x "$GDLINT" ]; then
            echo "GDScript tools not found — setting up Python venv..."
            python3 -m venv "$REPO_ROOT/python/.venv"
            "$REPO_ROOT/python/.venv/bin/pip" install -r "$REPO_ROOT/python/requirements-dev.txt"
        fi
        echo "Checking GDScript formatting..."
        "$GDFORMAT" --check --line-length 100 godot/scripts/*.gd
        echo ""
        echo "Running GDScript linter..."
        "$GDLINT" godot/scripts/*.gd
        echo ""
        echo "All checks passed."
        ;;
    *)
        echo "Usage: scripts/build.sh [debug|release|test|quicktest|run|run-branch|check|coverage]" >&2
        exit 1
        ;;
esac
