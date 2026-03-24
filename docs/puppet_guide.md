# Puppet: Remote Game Control for AI Testing

**Keep this document up to date.** When you add RPCs, discover gotchas, or change behavior, update this guide so the next Claude has accurate information.

## What It Is

Puppet lets you launch a headless Elven Canopy instance and control it over TCP — press buttons, read UI text, query game state, step the simulation. Three components:

1. **`godot/scripts/puppet_server.gd`** — TCP server autoload, activated by `PUPPET_SERVER=<port>` env var (inert otherwise)
2. **`godot/scripts/puppet_helpers.gd`** — shared UI interaction helpers (also used by GUT integration tests)
3. **`scripts/puppet.py`** — Python CLI (stdlib only) that manages sessions and sends RPCs

## Quick Start

```bash
# Launch a headless game (starts at main menu)
python3 scripts/puppet.py launch

# Or launch with a visible window (for watching on a non-headless machine)
python3 scripts/puppet.py launch --visible

# Navigate into a game
python3 scripts/puppet.py press-button "New Game"
python3 scripts/puppet.py press-button "Start Game"

# Wait ~30-60s for scene load (tree generation is slow in debug builds)
# Poll with ping until responsive:
python3 scripts/puppet.py ping

# Now interact
python3 scripts/puppet.py game-state
python3 scripts/puppet.py press-key U              # open units panel
python3 scripts/puppet.py collect-text UnitsPanel   # read all text from it
python3 scripts/puppet.py press-key ESCAPE          # close panel

# ALWAYS clean up when done
python3 scripts/puppet.py kill
```

## Critical: Always Kill Your Sessions

**You do not have general kill permission.** If you leave headless Godot processes running, they will consume memory until the system OOMs or the user has to clean up manually. Always `kill` when done — even if your task errored out partway through.

```bash
python3 scripts/puppet.py kill         # kill session "a" (default)
python3 scripts/puppet.py kill --all   # kill ALL sessions
python3 scripts/puppet.py list         # verify nothing is running
```

The kill command tries graceful RPC quit first, then SIGTERM, then SIGKILL on the process group.

## Session Management

Sessions are tracked in `.tmp/puppet-<id>.json` files (PID, port, start time).

```bash
python3 scripts/puppet.py launch              # default session "a"
python3 scripts/puppet.py launch -g b         # second session "b"
python3 scripts/puppet.py -g b game-state     # talk to session "b"
python3 scripts/puppet.py list                # show all sessions
python3 scripts/puppet.py kill --all          # clean up everything
```

## Available RPC Methods

### Observe (read-only)

| Method | Args | Returns |
|--------|------|---------|
| `ping` | — | `"pong"` |
| `game-state` | — | tick, elf_count, mana, speed, visible_panels |
| `list-panels` | — | all UI panels with visibility status |
| `is-panel-visible` | `panel_name` | boolean |
| `read-panel-text` | `node_name` | text of first matching Label/RichTextLabel |
| `find-text` | `panel_name` `substring` | boolean — is substring present? |
| `collect-text` | `panel_name` | array of {node_name, node_type, text} for all Labels/Buttons |
| `tree-info` | — | home tree stats (mana, health, growth, voxels) |
| `list-structures` | — | array of built structures |

### Act (mutate state or UI)

| Method | Args | Returns |
|--------|------|---------|
| `press-key` | `key_name` | OK — sends synthetic key press+release |
| `press-button` | `button_text` | OK — finds button by text substring, presses it |
| `press-button-near` | `label_text` `button_text` | OK — finds button adjacent to a label |
| `click-at-world-pos` | `x,y,z` | OK — moves camera, projects to screen, clicks |
| `step-ticks` | `count` | {ticks_stepped, current_tick} — sim must be paused |
| `set-sim-speed` | `Paused\|Normal\|Fast\|VeryFast` | OK |
| `move-camera-to` | `x,y,z` | OK — repositions camera pivot |
| `quit` | — | OK — shuts down game process |

### Key Names for press-key

Single letters (`A`–`Z`, case-insensitive), digits (`0`–`9`), and named keys: `ESCAPE`/`ESC`, `ENTER`/`RETURN`, `SPACE`, `TAB`, `BACKSPACE`, `DELETE`/`DEL`, `HOME`, `END`, `UP`/`DOWN`/`LEFT`/`RIGHT`, `F1`–`F12`.

## Gotchas and Tips

### Scene Load Blocking

After pressing "Start Game", the game takes **30-120 seconds** to load in debug builds (tree generation, nav graph, mesh build). On constrained cloud systems with other processes competing for CPU, expect the upper end. During this time the TCP server is completely unresponsive because `_process()` doesn't run while the scene is loading on the main thread.

**How to handle this:** Poll with `ping` until you get `pong`. Set a generous timeout (2+ minutes). Do not assume the process is hung just because it doesn't respond for 60 seconds — scene generation is genuinely that slow in debug mode. The `puppet.py launch` command only waits for the TCP server at the main menu, not for the game scene to finish loading after you press "Start Game."

If the process dies during load, check `.tmp/puppet-<id>.log` for errors.

### game-state Requires a Game Scene

`game-state`, `tree-info`, `step-ticks`, and `list-structures` all need the SimBridge, which only exists after a game is loaded. They return `"bridge not available"` at the main menu. UI methods (`press-button`, `collect-text`, etc.) work on any scene including menus.

### read-panel-text vs collect-text

`read-panel-text` finds a single node by name and returns its text. If multiple nodes share the same name (e.g., every creature row has a `NameLabel`), it only returns the first. Use `collect-text` to scrape all text from a panel subtree.

### Orphan Guard

If no RPC is received for 300 seconds (5 minutes, set by `PUPPET_TIMEOUT_SECS` in puppet.py's launch), the game auto-quits. This is a safety net — don't rely on it. Always `kill` explicitly.

### preload Pattern

`puppet_helpers.gd` does **not** use `class_name` (removed to avoid Godot UID generation issues). All references go through `preload("res://scripts/puppet_helpers.gd")`. If you add new files that reference the helpers, use the same pattern.

## Wire Protocol

4-byte big-endian length prefix + UTF-8 JSON payload. Max 1 MB.

- Request: `{"method": "game-state", "args": ["optional", "params"]}`
- Response: `{"ok": true, "result": ...}` or `{"error": "description"}`

This matches the relay protocol's framing pattern (see `elven_canopy_protocol/src/framing.rs`), though the implementations are independent.

## Not Yet Implemented

These are described in `docs/drafts/F-ai-test-harness.md` but not built yet:

- `list-creatures` — needs a new bridge method
- `creature-info <id>` — needs bridge glue
- `select-creature <uuid>` — needs SelectionController glue
- `eval <filename>` — escape hatch for arbitrary GDScript
- `--` command chaining in puppet.py (multiple commands in one invocation)

## File Map

| File | Purpose |
|------|---------|
| `scripts/puppet.py` | Python CLI — launch, kill, list, RPC subcommands |
| `godot/scripts/puppet_server.gd` | TCP server autoload (inert without env var) |
| `godot/scripts/puppet_helpers.gd` | Shared UI helpers (also used by GUT integration tests) |
| `godot/test/test_puppet.gd` | Unit tests for helpers and server internals |
| `godot/test/test_harness_integration.gd` | Integration tests using the same helpers |
| `docs/drafts/F-ai-test-harness.md` | Design doc (aspirational — not all implemented) |
| `.tmp/puppet-*.json` | Session state files (gitignored) |
