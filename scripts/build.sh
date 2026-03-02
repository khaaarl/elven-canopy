#!/usr/bin/env bash
# Build script for Elven Canopy.
#
# Compiles the Rust GDExtension library (debug or release) and ensures the
# godot/target symlink exists so Godot can find the compiled .so/.dll/.dylib.
#
# Usage:
#   scripts/build.sh            # debug build
#   scripts/build.sh release    # release build
#   scripts/build.sh test       # run all crate tests + gdext compile check
#   scripts/build.sh quicktest  # test only crates changed vs main
#   scripts/build.sh run        # debug build then launch the game
#   scripts/build.sh check      # run fmt, clippy, gdformat, gdlint checks
#
# Run from the repo root.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

MODE="${1:-debug}"

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
        godot --path "$REPO_ROOT/godot" --headless --import --quit &>/dev/null || true
    fi
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
        ALL_TEST_PACKAGES="-p elven_canopy_prng -p elven_canopy_lang -p elven_canopy_sim -p elven_canopy_protocol -p elven_canopy_relay -p elven_canopy_music -p multiplayer_tests"
        echo "Compile-checking elven_canopy_gdext..."
        cargo build -p elven_canopy_gdext
        echo ""
        echo "Running all crate tests..."
        cargo test $ALL_TEST_PACKAGES -- --test-threads=16
        echo ""
        echo "All tests passed."
        ;;
    quicktest)
        # Test only crates with source changes relative to main.
        CHANGED_FILES="$(git diff --name-only main...HEAD 2>/dev/null || true)"
        TEST_PACKAGES=""
        for CRATE_DIR in elven_canopy_prng elven_canopy_lang elven_canopy_sim elven_canopy_protocol elven_canopy_relay elven_canopy_music; do
            if printf '%s' "$CHANGED_FILES" | grep -q "^${CRATE_DIR}/"; then
                TEST_PACKAGES="$TEST_PACKAGES -p $CRATE_DIR"
            fi
        done
        # Always include multiplayer_tests (cross-crate correctness).
        TEST_PACKAGES="$TEST_PACKAGES -p multiplayer_tests"
        echo "Compile-checking elven_canopy_gdext..."
        cargo build -p elven_canopy_gdext
        echo ""
        echo "Running tests for:$TEST_PACKAGES"
        cargo test $TEST_PACKAGES -- --test-threads=16
        echo ""
        echo "All tests passed."
        ;;
    run)
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext
        ensure_godot_imported
        echo "Launching Elven Canopy..."
        RUST_BACKTRACE=1 godot --path "$REPO_ROOT/godot"
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
        echo "Usage: scripts/build.sh [debug|release|test|quicktest|run|check]" >&2
        exit 1
        ;;
esac
