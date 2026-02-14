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
        echo "Running sim tests..."
        cargo test -p elven_canopy_sim
        echo ""
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext
        echo "Done. Run: cd godot && godot"
        ;;
    run)
        echo "Building elven_canopy_gdext (debug)..."
        cargo build -p elven_canopy_gdext
        echo "Launching Elven Canopy..."
        godot --path "$REPO_ROOT/godot"
        ;;
    *)
        echo "Usage: scripts/build.sh [debug|release|test|run]" >&2
        exit 1
        ;;
esac
