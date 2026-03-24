# F-ai-test-harness: Puppet

**Tracker:** F-ai-test-harness

## Goal

Let Claude (or any external process) remotely observe and control a running game instance. Launch the game, query UI state, click things, press keys, read panels — all from the terminal via a Python CLI.

## Architecture

Three components:

1. **GDScript TCP server** — an autoload that binds via `TCPServer`, accepts connections as `StreamPeerTCP`, and processes requests in `_process()` with manual length-prefix buffering. Runs on the main thread, giving handlers full scene tree access. Activated by `PUPPET_SERVER=<port>` env var. **Must never be set in production builds.**

2. **RPC protocol** — JSON over TCP, length-delimited (4-byte big-endian length prefix + JSON payload, matching the relay protocol pattern). Request: `{"method": "read-panel-text", "args": ["CreatureInfoPanel"]}`. Response: `{"ok": true, "result": ...}` or `{"error": "..."}`.

3. **`scripts/puppet.py`** — standalone Python CLI (stdlib only, no virtualenv). Connects to the server, sends one or more RPC calls, prints human-readable output (tee to `.tmp/`). Made executable with shebang.

## CLI Usage

```bash
scripts/puppet.py launch                              # start game, default session "a"
scripts/puppet.py launch -g b                         # start a second game as session "b"
scripts/puppet.py game-state                           # talk to session "a" (default)
scripts/puppet.py -g b game-state                     # talk to session "b"
scripts/puppet.py -g a press-key B -- -g b press-key B  # same key to both
scripts/puppet.py read-panel-text CreatureInfoPanel
scripts/puppet.py click-at-world-pos 5,3,1
scripts/puppet.py find-text CreatureInfoPanel "Idle"
scripts/puppet.py eval .tmp/eval-snippet.gd
scripts/puppet.py list                                 # show running sessions
scripts/puppet.py quit                                 # quit session "a"
scripts/puppet.py -g b quit                           # quit session "b"
```

`-g <id>` (game ID) selects which session to talk to. Defaults to `a`. `launch` starts the game under `xvfb-run` with `PUPPET_SERVER=<port>`, picks a free port, writes session info (PID, port) to `.tmp/puppet-<id>.json`. Multiple games can run simultaneously — useful for multiplayer testing.

`launch` polls the TCP port until the server responds (with backoff), so it only returns once the game is ready for RPCs. `list` reads session files and checks PID liveness, cleaning up stale entries.

Multiple commands separated by `--`, executed sequentially in one TCP connection. Each segment can specify its own `-g` to target different sessions. Output is structured text, not raw JSON.

## Built-in RPC Methods

RPC method names mirror the helpers in `test_harness_integration.gd`. The helpers are currently instance methods on the GUT test class — they must be extracted into a standalone utility script (e.g., `godot/scripts/puppet_helpers.gd`) that both the puppet server autoload and the test file can use. The utility script takes a scene root reference on construction.

**Observe:**
- `game-state` — tick, elf count, mana, speed, list of visible panels
- `list-panels` — all named panels with visibility (aggregates `_is_panel_visible`)
- `is-panel-visible <name>` — boolean check for one panel (via `_is_panel_visible`)
- `read-panel-text <name>` — text of a single Label or RichTextLabel node by name (via `_read_panel_text`). For aggregating all text in a panel subtree, use `find-text` or `eval`.
- `find-text <panel> <substring>` — search panel descendants (Labels, Buttons, RichTextLabels) for text (via `_find_text_in_descendants`)
- `list-creatures` — species, name, position, task for each creature (new bridge method needed)
- `creature-info <id>` — detailed info for one creature (via bridge `get_creature_info_by_id`)
- `tree-info` — home tree position, mana, growth (via bridge `get_home_tree_info`)
- `list-structures` — list of built structures (via bridge `get_structures`)

**Act:**
- `click-at-world-pos <x,y,z>` — move camera, project to screen, dispatch click (via `_move_camera_to` + `_click_at_world_pos`)
- `press-button <substring>` — find button by text, press it (via `_find_button` + `_press_button`)
- `press-button-near <label_text> <button_text>` — find button adjacent to a label, searching the whole scene tree (via `_find_button_near_label`)
- `press-key <KEY>` — synthetic key press+release (via `_press_key`)
- `select-creature <uuid>` — select a creature by ID via `SelectionController` (new glue code needed; skip click projection)
- `step-ticks <n>` — advance sim by N ticks (via `_step_ticks`). Returns error if sim is not paused.
- `set-sim-speed <Paused|Normal|Fast>` — set sim speed (via bridge `set_sim_speed`)
- `move-camera-to <x,y,z>` — move camera pivot (via `_move_camera_to`)
- `quit` — shut down the game process cleanly

**Escape hatch:**
- `eval <filename>` — `puppet.py` reads the file and sends its contents in the RPC payload. The server writes it to `user://puppet_eval.gd`, loads and instantiates it, calls `run(scene_root)`, and returns the result. Resource cache is invalidated between evals.

## Implementation Notes

- TCP framing requires manual buffering in GDScript — `StreamPeerTCP` gives raw bytes, so the server must accumulate data until a full length-prefixed message is available. The wire format (4-byte big-endian length + JSON) matches the relay protocol for consistency, though the implementations are independent.
- Bridge methods return Godot variant types (`VarArray`, `VarDictionary`). RPC handlers must convert these to JSON-serializable dicts/arrays before sending responses.
- RPCs that fail (missing panel, no matching button, off-screen click) return `{"error": "..."}` with a descriptive message. `puppet.py` prints errors to stderr and exits non-zero.
- `puppet.py` is stdlib-only (`socket`, `json`, `argparse`, `subprocess`). No external deps.
- `puppet.py launch` handles everything: picks a free port, sets `PUPPET_SERVER=<port>`, launches the game under `xvfb-run` (same xvfb approach as `build.sh gdtest`), writes `.tmp/puppet-<id>.json` (port, PID), polls until ready. `-g <id>` names the session (default `a`). Multiple games coexist for multiplayer testing.
- The puppet autoload is registered in `project.godot`, ordered after `GameSession`. It reads `PUPPET_SERVER` on startup. If unset, the autoload is inert (no server, no overhead). If set, listens on that port.
- Max message size: 1 MB (far more than any realistic RPC; prevents runaway allocations).
- **Orphan guard:** When the puppet server is active, if no RPC is received for ~10 minutes (configurable via `PUPPET_TIMEOUT_SECS`), the game shuts itself down. Prevents abandoned headless processes.
- The built-in method set will grow organically as Claude uses the system and discovers what's missing.
