#!/usr/bin/env bash
# Build script for Elven Canopy.
#
# Compiles the Rust GDExtension library (debug or release) and ensures the
# godot/target symlink exists so Godot can find the compiled .so/.dll/.dylib.
#
# Usage:
#   scripts/build.sh          # debug build
#   scripts/build.sh release  # release build
#   scripts/build.sh test     # run sim tests then build
#   scripts/build.sh run      # debug build then launch the game
#   scripts/build.sh check    # run fmt, clippy, gdformat, gdlint checks
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

# --- Limit parallelism on low-RAM systems ------------------------------------
# Each rustc process can use 1-2 GB on the gdext crate. On systems with ≤4 GB
# of RAM, restrict to a single job to avoid OOM / heavy swapping.

CARGO_JOBS=""
TOTAL_RAM_KB=$(grep MemTotal /proc/meminfo 2>/dev/null | awk '{print $2}') || true
if [ -n "$TOTAL_RAM_KB" ] && [ "$TOTAL_RAM_KB" -le 4194304 ]; then
    CARGO_JOBS="-j 1"
    echo "Low RAM detected ($(( TOTAL_RAM_KB / 1024 )) MB) — building with -j 1"
fi

# --- Build --------------------------------------------------------------------

case "$MODE" in
    debug)
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext $CARGO_JOBS
        echo "Done. Run: cd godot && godot"
        ;;
    release)
        echo "Building elven_canopy_gdext (release)..."
        cargo build -p elven_canopy_gdext --release $CARGO_JOBS
        echo "Done. Run: cd godot && godot"
        ;;
    test)
        echo "Running sim tests..."
        cargo test -p elven_canopy_sim $CARGO_JOBS
        echo ""
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext $CARGO_JOBS
        echo "Done. Run: cd godot && godot"
        ;;
    run)
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext $CARGO_JOBS
        ensure_godot_imported
        echo "Launching Elven Canopy..."
        godot --path "$REPO_ROOT/godot"
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
        echo "Usage: scripts/build.sh [debug|release|test|run|check]" >&2
        exit 1
        ;;
esac
