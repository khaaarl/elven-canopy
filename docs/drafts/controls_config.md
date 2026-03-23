# F-controls-config: Centralized Controls Configuration

## Overview

Replace scattered `KEY_*` checks across ~15 GDScript files with a single
`ControlsConfig` autoload that owns all input bindings as data. Player
customizations persist to `user://controls.json` as overrides on top of
defaults. A full settings screen replaces `keybind_help.gd`.

## Design Decisions (from discussion)

- **Scope:** Everything is rebindable — keyboard shortcuts, camera controls
  (WASD/arrows via Godot input actions), mouse buttons, construction sub-mode
  keys, and menu bindings. This includes multiplayer menu hotkeys (H/J/B).
  Godot built-in actions like `ui_accept` in `new_game_menu.gd` are left as
  Godot-managed actions and are NOT in scope — they use standard platform
  conventions and do not need rebinding.
- **Toggles and sensitivity:** Invert-X, Invert-Y, invert scroll zoom, mouse
  orbit sensitivity, mouse zoom sensitivity.
- **Physical vs logical keycodes:** Camera movement actions (WASD, Q/E,
  arrows, R/F, PgUp/PgDn) use physical keycodes in `project.godot` so
  they work on non-QWERTY layouts (e.g., AZERTY's physical W key moves
  forward even though it produces Z). All other bindings use logical
  keycodes (the character the key produces). The data model distinguishes
  these with a `"physical": true` flag on movement bindings. When
  ControlsConfig updates the InputMap at runtime, it creates
  `InputEventKey` objects with `physical_keycode` set (not `keycode`) for
  physical bindings. Override JSON stores physical bindings with a
  `"physical": true` marker; the key string still uses the QWERTY label
  (e.g., `"W"`) since that is what `OS.get_keycode_string()` returns for
  the physical keycode value. The settings screen Phase C should display
  physical bindings by their physical key label, not the character they
  produce on the current layout.
- **Architecture:** Pure GDScript autoload (`ControlsConfig`). No Rust
  involvement — this is entirely a UI/input concern.
- **Persistence:** `user://controls.json` stores only overrides (delta from
  defaults). New bindings added in future versions automatically get defaults.
  If the file is missing or corrupt (invalid JSON, unknown version), the
  autoload logs a warning and falls back to all defaults — no crash, no
  partial state.
- **Modifier combos:** Data model supports them from day one (binding stores
  key + optional modifiers), but modifier-combo rebinding UI deferred to
  F-modifier-keybinds.
- **Modifier matching semantics (Phase A):** `is_action()` ignores modifier
  state when the binding's `modifiers` array is empty (matching current
  behavior — `KEY_1` fires regardless of Shift/Ctrl). When `modifiers` is
  non-empty (future, via F-modifier-keybinds), all listed modifiers must be
  held AND no unlisted modifiers may be held (exact match). This means adding
  a Ctrl+B binding later will not interfere with the unmodified B binding.
- **Conflict detection:** Deferred to F-binding-conflicts. However, Phase A
  includes a debug-build assertion at startup that checks for duplicate
  default key assignments within the same context scope — this catches the
  most obvious class of bugs (accidental duplicates when adding new features)
  with zero UI work.
- **"Press a key" rebind cancel:** 5-second timeout cancels the rebind (no
  ESC-as-cancel, so ESC itself is rebindable). A visible "Cancel" button is
  also shown during capture mode as a click target, solving both the
  discoverability and frustration concerns without requiring ESC.
- **Menu bindings:** Rebindable but shown in a low-priority category at the
  bottom of the settings screen.
- **Input type constraints:** Actions are typed as either keyboard-bindable
  or mouse-bindable. A keyboard action cannot be rebound to a mouse button
  and vice versa. This avoids complexity around event type mismatches (e.g.,
  rebinding "select" from left-click to a keyboard key would break
  click-position-dependent logic). If cross-type rebinding is ever wanted,
  it can be added as a future enhancement.
- **Construction preview panel buttons:** UI buttons in the construction
  preview panel (e.g., the "R" rotation button for ladders) are click-only
  UI elements, not keyboard shortcuts. They are out of scope for
  ControlsConfig. If keyboard shortcuts for these are added later, they
  should be added as new bindings at that time.

## Phasing

### Phase A: Centralization (F-controls-config-A)

Create the `ControlsConfig` autoload and migrate all existing input handling.

**ControlsConfig autoload (`controls_config.gd`):**
- Dictionary of action names -> binding definitions
- Each binding: `{ key: KEY_*, modifiers: [], mouse_button: null, context: "...", hidden: false }`
- Lookup methods:
  - `get_key(action_name)` — returns the KEY_* constant for the action.
  - `is_action(event, action_name)` — event-based check for `_input()` and
    `_unhandled_input()` callbacks. Checks both primary and alt key. Handles
    both `InputEventKey` and `InputEventMouseButton` based on the binding's
    type. **Matches on both press and release events** — it is a pure
    key/button identity check that does NOT filter on `event.pressed`.
    Callers already distinguish press vs release themselves (e.g.,
    `construction_controller.gd` checks `not mb.pressed` for drag-end).
    Works identically in `_input()` and `_unhandled_input()` — callers
    choose which callback to use based on their precedence needs (e.g.,
    `save_dialog.gd` uses `_input()` intentionally to fire before the
    ItemList's type-to-search).
    **Echo filtering:** `is_action()` does NOT filter echo events
    (`event.echo`). Callers that want only the initial keypress (not
    held-key repeats) must check `not event.echo` themselves, as they do
    today. No current handler intentionally processes echo events, but
    filtering in `is_action()` would be a hidden behavior change — callers
    keep their existing echo checks during migration.
  - `is_pressed(action_name) -> bool` — polling-based check for `_process()`
    continuous input. For InputMap-backed actions (those with
    `"input_action": true` — WASD, arrows, focal, tilt), delegates to
    `Input.is_action_pressed(action_name)`, which respects the InputMap and
    handles physical keycodes and alt keys automatically. For non-InputMap
    actions (zoom_in, zoom_out), wraps
    `Input.is_key_pressed(get_key(action_name))` (and checks alt_key too).
    Used by `orbital_camera.gd` for both camera movement and continuous
    zoom.
  - `get_display_name(action_name)` — returns human-readable string for UI
    (e.g., "Space", "Left Click", "Q / Left Arrow" for alt-key bindings,
    "Ctrl+S" for modifier combos). Uses `OS.get_keycode_string()` for keys;
    manual mapping for mouse buttons.
  - `get_label_suffix(action_name)` — returns `"[B]"` style string for
    embedding in button labels. Toolbar and panel buttons call this to show
    current bindings instead of hardcoding key names.
- Binding categories (display order matches this list; "Menus" always last):
  1. Camera Movement (WASD, Q/E rotation, arrow tilt, R/F/PgUp/PgDn focal)
  2. Camera Zoom/Orbit (scroll wheel, +/-, middle mouse drag)
  3. Speed Controls (Space, 1, 2, 3)
  4. Construction Mode (B toggle, P/G/L/C sub-modes, Enter confirm, Right Click cancel)
  5. Panels (T tasks, U units, I tree info, ? help, F12 debug)
  6. Selection (Left Click select, ESC deselect)
  7. General (ESC — the universal dismiss/cancel/close action)
  8. Menus (N/L/M/Q main menu, Q/S escape menu, H/J/B multiplayer menu)
- Godot input action bindings (WASD, arrows, etc.) updated at runtime via
  `InputMap` API when overrides are loaded.
- Hidden bindings: debug shortcuts (F12, debug-panel spawn keys) have
  `"hidden": true` and do not appear in the settings screen UI. They are
  still centrally managed and rebindable via `controls.json` for power users.
- **Structures panel:** The "Structures" toolbar button has no keyboard
  shortcut — it is click-only. No binding entry exists in ControlsConfig for
  it. If a keyboard shortcut is added later, it should go through
  ControlsConfig as a new binding.

**ESC as a single action with shared semantics:**

ESC is defined as one action (`"ui_cancel"`) in the bindings dictionary.
Multiple handlers all check `is_action(event, "ui_cancel")` and rely on the
existing `_unhandled_input` precedence chain (node tree order + calling
`set_input_as_handled()`) to determine which handler fires. This means:

- Rebinding ESC changes the dismiss/cancel key everywhere at once — which is
  the correct behavior (you would not want panels to close on ESC while
  construction cancels on Backspace).
- The precedence chain is unchanged: placement_controller -> construction_controller
  -> keybind_help -> task_panel / units_panel / structure_list_panel /
  tree_info_panel -> selection_controller -> escape_menu.
- `escape_menu.gd` currently uses `event.is_action_pressed("ui_cancel")` (a
  Godot built-in action). During migration, `escape_menu.gd` switches to
  `controls_config.is_action(event, "ui_cancel")` like all other handlers.
  ControlsConfig also updates the Godot `InputMap` for `ui_cancel` when
  overrides are loaded, so any Godot-internal uses of the action stay in
  sync.

**Migration:**
- Every `_unhandled_input` and `_input` handler that checks `KEY_*` or
  `MOUSE_BUTTON_*` switches to `controls_config.is_action(event, "action_name")`.
- `orbital_camera.gd` `_process` polling switches to
  `controls_config.is_pressed("zoom_in")` /
  `controls_config.is_pressed("zoom_out")`.
- Toolbar button labels (e.g., `"Build [B]"`) switch to using
  `controls_config.get_label_suffix()` so they reflect current bindings.
  Same for construction sub-mode buttons (`"Platform [P]"` etc.) and panel
  close buttons (`"Close [ESC]"`).
- Help overlay (`keybind_help.gd`) keeps its hardcoded content in Phase A
  (changing it to auto-generate would alter visible behavior during a
  pure-refactoring phase). Auto-generation happens in Phase C when the
  settings screen replaces it entirely.
- **Phase A audit checklist:** Before Phase A is considered complete, do a
  comprehensive grep for `KEY_`, `MOUSE_BUTTON_`, `Input.is_key_pressed`,
  `Input.is_action_pressed`, and `is_action_pressed` (the event form)
  across all GDScript files. Every hit must either be migrated to
  ControlsConfig or explicitly documented as an exception (e.g., Godot
  built-in `ui_accept`, `save_dialog.gd`'s LineEdit `text_submitted`
  signal).

### Phase B: Persistence (F-controls-config-B)

Merged scope note: Phase B covers persistence (load/save) AND the non-keybind
settings (invert, sensitivity). The settings screen UI is Phase C. Phase B is
testable via manual `controls.json` editing and via the non-keybind settings
which `orbital_camera.gd` reads each frame — changing a value in the JSON and
restarting produces observable behavior changes (inverted camera, different
sensitivity).

**Persistence:**
- On startup, load `user://controls.json` if it exists. Merge overrides on
  top of defaults. If the file is missing, corrupt, or has an unknown version,
  log a warning and use all defaults.
- Save triggered explicitly from settings screen (not auto-save on every
  change — user confirms).
- Schema: `{ "version": 1, "bindings": { "action_name": { "key": "KEY_B" } }, "settings": { "invert_x": false, ... } }`
- Key names in JSON use `OS.get_keycode_string()` format (e.g., `"Space"`,
  `"Escape"`, `"F12"`, `"B"`). On load, converted back via
  `OS.find_keycode_from_string()`. Unknown key strings are logged as warnings
  and ignored (binding falls back to default). Mouse buttons use string names
  like `"LeftClick"`, `"RightClick"`, `"MiddleClick"`, `"ScrollUp"`,
  `"ScrollDown"`.

**Non-keybind settings:**
- Invert-X (horizontal orbit direction)
- Invert-Y (vertical orbit/tilt direction)
- Invert scroll zoom (scroll-up = zoom out)
- Mouse orbit sensitivity (float, default 0.005)
- Mouse zoom sensitivity (float, affects scroll wheel zoom speed)
- Key zoom speed (float, default 30.0)

**Settings plumbed into orbital_camera.gd:**
- Camera reads invert/sensitivity values from `ControlsConfig` each frame.
- Changing a setting takes effect immediately (no restart needed).

### Phase C: Settings Screen (F-controls-config-C)

**Full settings screen UI:**
- Replaces `keybind_help.gd` and the "? Help" toolbar button.
- New "Controls" button in toolbar (replaces "? Help"). The `?` key binding
  (`toggle_help` action) is preserved and opens the new settings screen, so
  players with muscle memory for `?` are not broken.
- Layout: categorized list of all bindings (excluding `hidden: true`), each
  row showing action name + current binding + "Rebind" button. Categories
  are collapsible sections, displayed in the order defined in the category
  list above. All categories start expanded; collapse state does not persist
  across reopens (always reopen fully expanded).
- "Press a key" capture: clicking Rebind enters a fully modal capture mode
  that consumes ALL input (keyboard and mouse). Next keypress or mouse button
  sets the new binding. 5-second timeout cancels. Visual countdown indicator.
  A visible "Cancel" button is also shown as a click target. During capture
  mode, if the pressed key is already bound to another action, a warning
  label appears: "Already bound to [Action Name]". The binding is still
  set (full conflict prevention is F-binding-conflicts), but the player is
  informed.
- Alt-key bindings: settings screen shows both primary and alt key for
  actions that have them. Both are independently rebindable.
- Reset-to-default per individual binding (small reset icon per row).
- "Reset All to Defaults" button at the bottom, with a confirmation dialog
  ("Are you sure? This will reset all bindings to defaults.").
- Non-keybind settings (invert toggles, sensitivity sliders) shown in their
  own section.
- Menu bindings category shown last (low priority, niche).

**Cleanup:**
- Delete `keybind_help.gd`.
- Remove "? Help" button, add "Controls" button in its place.

## Binding Data Model

```gdscript
# Category display order (settings screen renders in this order)
var category_order = [
    "Camera Movement",
    "Camera Zoom/Orbit",
    "Speed Controls",
    "Construction",
    "Panels",
    "Selection",
    "General",
    "Menus",
]

# Each binding definition
var bindings = {
    # Camera Movement (Godot InputMap actions — physical keycodes for non-QWERTY support)
    # "input_action": true means this is backed by a Godot InputMap action.
    # "physical": true means the InputEventKey uses physical_keycode, not keycode.
    # is_pressed() delegates to Input.is_action_pressed() for these.
    "move_forward": { "key": KEY_W, "category": "Camera Movement", "label": "Move Forward",
                      "context": "gameplay", "input_action": true, "physical": true },
    "move_back": { "key": KEY_S, "category": "Camera Movement", "label": "Move Back",
                   "context": "gameplay", "input_action": true, "physical": true },
    "move_left": { "key": KEY_A, "category": "Camera Movement", "label": "Move Left",
                   "context": "gameplay", "input_action": true, "physical": true },
    "move_right": { "key": KEY_D, "category": "Camera Movement", "label": "Move Right",
                    "context": "gameplay", "input_action": true, "physical": true },
    "rotate_left": { "key": KEY_Q, "category": "Camera Movement", "label": "Rotate Left",
                     "alt_key": KEY_LEFT, "context": "gameplay", "input_action": true,
                     "physical": true },
    "rotate_right": { "key": KEY_E, "category": "Camera Movement", "label": "Rotate Right",
                      "alt_key": KEY_RIGHT, "context": "gameplay", "input_action": true,
                      "physical": true },
    "tilt_up": { "key": KEY_UP, "category": "Camera Movement", "label": "Tilt Up",
                 "context": "gameplay", "input_action": true, "physical": true },
    "tilt_down": { "key": KEY_DOWN, "category": "Camera Movement", "label": "Tilt Down",
                   "context": "gameplay", "input_action": true, "physical": true },
    "focal_up": { "key": KEY_PAGEUP, "category": "Camera Movement", "label": "Focal Height Up",
                  "alt_key": KEY_R, "context": "gameplay", "input_action": true,
                  "physical": true },
    "focal_down": { "key": KEY_PAGEDOWN, "category": "Camera Movement",
                    "label": "Focal Height Down", "alt_key": KEY_F, "context": "gameplay",
                    "input_action": true, "physical": true },

    # Discrete shortcuts
    "toggle_pause": { "key": KEY_SPACE, "category": "Speed Controls", "label": "Pause/Resume",
                      "context": "gameplay" },
    "speed_normal": { "key": KEY_1, "category": "Speed Controls", "label": "Normal Speed",
                      "context": "gameplay" },
    "speed_fast": { "key": KEY_2, "category": "Speed Controls", "label": "Fast Speed",
                    "context": "gameplay" },
    "speed_vfast": { "key": KEY_3, "category": "Speed Controls", "label": "Very Fast Speed",
                     "context": "gameplay" },
    "build_mode": { "key": KEY_B, "category": "Construction", "label": "Toggle Build Mode",
                    "context": "gameplay" },

    # Construction sub-modes (only active inside construction mode)
    "build_platform": { "key": KEY_P, "category": "Construction", "label": "Platform Mode",
                        "context": "construction" },
    "build_building": { "key": KEY_G, "category": "Construction", "label": "Building Mode",
                        "context": "construction" },
    "build_ladder": { "key": KEY_L, "category": "Construction", "label": "Ladder Mode",
                      "context": "construction" },
    "build_carve": { "key": KEY_C, "category": "Construction", "label": "Carve Mode",
                     "context": "construction" },
    "confirm_placement": { "key": KEY_ENTER, "alt_key": KEY_KP_ENTER,
                           "category": "Construction", "label": "Confirm Placement",
                           "context": "construction" },

    # Mouse bindings
    "select": { "mouse_button": MOUSE_BUTTON_LEFT, "category": "Selection",
                "label": "Select", "context": "gameplay" },
    "cancel_mouse": { "mouse_button": MOUSE_BUTTON_RIGHT, "category": "Construction",
                      "label": "Cancel (mouse)", "context": "gameplay" },
    # NOTE: Orbit is a drag interaction — press sets _rotating flag, motion while
    # _rotating does rotation, release clears _rotating. Rebinding orbit to a
    # different mouse button requires orbital_camera.gd to use is_action() for
    # both the press AND release events (not just the initial press), and the
    # motion guard must check the _rotating flag (not a hardcoded button index).
    # The Phase A migration handles this by replacing all MOUSE_BUTTON_MIDDLE
    # checks in orbital_camera.gd with ControlsConfig lookups.
    "camera_orbit": { "mouse_button": MOUSE_BUTTON_MIDDLE, "category": "Camera Zoom/Orbit",
                      "label": "Orbit Camera (drag)", "context": "gameplay" },
    "zoom_in_scroll": { "mouse_button": MOUSE_BUTTON_WHEEL_UP,
                        "category": "Camera Zoom/Orbit", "label": "Zoom In (scroll)",
                        "context": "gameplay" },
    "zoom_out_scroll": { "mouse_button": MOUSE_BUTTON_WHEEL_DOWN,
                         "category": "Camera Zoom/Orbit", "label": "Zoom Out (scroll)",
                         "context": "gameplay" },

    # Keyboard zoom (continuous — used via is_pressed(), not is_action())
    "zoom_in": { "key": KEY_EQUAL, "alt_key": KEY_KP_ADD,
                 "category": "Camera Zoom/Orbit", "label": "Zoom In (key)",
                 "context": "gameplay" },
    "zoom_out": { "key": KEY_MINUS, "alt_key": KEY_KP_SUBTRACT,
                  "category": "Camera Zoom/Orbit", "label": "Zoom Out (key)",
                  "context": "gameplay" },

    # General
    "ui_cancel": { "key": KEY_ESCAPE, "category": "General",
                   "label": "Dismiss / Cancel / Close", "context": "global" },

    # Menus
    "menu_host": { "key": KEY_H, "category": "Menus", "label": "Host Game",
                   "context": "multiplayer_menu" },
    "menu_join": { "key": KEY_J, "category": "Menus", "label": "Join Game",
                   "context": "multiplayer_menu" },
    "menu_back": { "key": KEY_B, "category": "Menus", "label": "Back",
                   "context": "multiplayer_menu" },

    # Main menu bindings
    "menu_new_game": { "key": KEY_N, "category": "Menus", "label": "New Game",
                       "context": "main_menu" },
    "menu_load_game": { "key": KEY_L, "category": "Menus", "label": "Load Game",
                        "context": "main_menu" },
    "menu_multiplayer": { "key": KEY_M, "category": "Menus", "label": "Multiplayer",
                          "context": "main_menu" },
    "menu_quit_main": { "key": KEY_Q, "category": "Menus", "label": "Quit",
                        "context": "main_menu" },

    # Pause menu bindings (Q and S — same keys as main menu Q but different context)
    "menu_quit_pause": { "key": KEY_Q, "category": "Menus", "label": "Quit to Menu",
                         "context": "escape_menu" },
    "menu_save": { "key": KEY_S, "category": "Menus", "label": "Save Game",
                   "context": "escape_menu" },

    # Load dialog bindings (modal overlay — uses _input(), distinct context from main_menu)
    "load_confirm": { "key": KEY_ENTER, "alt_key": KEY_KP_ENTER, "category": "Menus",
                      "label": "Load Selected Save", "context": "load_dialog" },
    "load_select": { "key": KEY_L, "category": "Menus", "label": "Load (shortcut)",
                     "context": "load_dialog" },

    # Panels
    "toggle_tasks": { "key": KEY_T, "category": "Panels", "label": "Toggle Tasks Panel",
                      "context": "gameplay" },
    "toggle_units": { "key": KEY_U, "category": "Panels", "label": "Toggle Units Panel",
                      "context": "gameplay" },
    "toggle_tree_info": { "key": KEY_I, "category": "Panels", "label": "Toggle Tree Info",
                          "context": "gameplay" },
    "toggle_help": { "key": KEY_QUESTION, "category": "Panels",
                     "label": "Help / Controls", "context": "gameplay" },

    # Debug (hidden — not shown in settings screen)
    "toggle_debug": { "key": KEY_F12, "category": "Panels", "label": "Toggle Debug Panel",
                      "context": "gameplay", "hidden": true },
}

# Non-keybind settings
var settings = {
    "invert_x": false,
    "invert_y": false,
    "invert_scroll_zoom": false,
    "mouse_orbit_sensitivity": 0.005,
    "mouse_zoom_sensitivity": 1.0,  # multiplier on scroll zoom step
    "key_zoom_speed": 30.0,
}
```

**Alt-key semantics:** The `alt_key` field provides a secondary binding for
the same action. `is_action()` and `is_pressed()` check both keys.
`get_display_name()` returns both (e.g., `"Q / Left Arrow"`). In the override
JSON, alt_key is stored as a separate field: `{ "key": "Q", "alt_key": "Left" }`.
Both primary and alt key are independently rebindable in the Phase C settings
screen. If a third binding is ever needed for a single action, `alt_key` can
be extended to an `alt_keys` array, but two covers all current cases.

**Context field:** The `context` field indicates when a binding is active.
Current values: `"global"` (always active), `"gameplay"` (in-game),
`"construction"` (only inside construction mode), `"multiplayer_menu"` (only
on the multiplayer menu screen), `"main_menu"`, `"escape_menu"`,
`"load_dialog"` (modal load-game overlay). Context is not enforced by
ControlsConfig itself — handlers still do their own visibility/state checks.
The field exists so F-binding-conflicts can use it for scope-aware conflict
detection: two bindings on the same key in different contexts are not a
conflict.

**Context overlap rules (for debug assertion and future conflict detection):**

Contexts form a hierarchy — some can be simultaneously active, others are
mutually exclusive. The debug assertion uses these rules to determine whether
two same-key bindings are a real conflict:

- **`global`** overlaps with everything (always active).
- **`gameplay`** overlaps with `construction` (construction is a sub-state of
  gameplay — the toolbar, camera, and ESC chain are all still active).
- **`gameplay`** does NOT overlap with `main_menu`, `escape_menu`,
  `multiplayer_menu`, or `load_dialog` (these are separate screens/modals).
- **`construction`** does NOT overlap with any menu context.
- **`main_menu`**, **`escape_menu`**, **`multiplayer_menu`**, and
  **`load_dialog`** are all mutually exclusive — only one menu screen is
  active at a time. (Note: `load_dialog` is a modal overlay that fires
  before `main_menu` via `_input()`, so even though both are "visible,"
  the dialog consumes input first. They have distinct contexts to avoid
  false conflict reports for shared keys like L.)
- **`escape_menu`** does NOT overlap with `gameplay` — when the escape menu
  is open, gameplay input is blocked.

Two bindings on the same key are a conflict only if their contexts overlap.
The debug assertion checks this at startup for all default bindings. Example
non-conflicts: B in `gameplay` (build_mode) vs B in `multiplayer_menu`
(menu_back); Q in `main_menu` (quit) vs Q in `escape_menu` (quit to menu);
L in `main_menu` (load game) vs L in `load_dialog` (load shortcut).

## File Changes

**New files:**
- `godot/scripts/controls_config.gd` — autoload singleton
- `godot/scripts/controls_screen.gd` — settings screen (Phase C)

**Modified files (Phase A migration):**
- `godot/scripts/action_toolbar.gd` — query ControlsConfig instead of KEY_*;
  use `get_label_suffix()` for button labels
- `godot/scripts/orbital_camera.gd` — use `is_pressed()` for zoom keys,
  read sensitivity/invert settings
- `godot/scripts/construction_controller.gd` — query ControlsConfig;
  use `get_label_suffix()` for sub-mode button labels
- `godot/scripts/selection_controller.gd` — query ControlsConfig
- `godot/scripts/placement_controller.gd` — query ControlsConfig
- `godot/scripts/escape_menu.gd` — switch from `ui_cancel` to ControlsConfig;
  query ControlsConfig for Q/S hotkeys
- `godot/scripts/main_menu.gd` — query ControlsConfig
- `godot/scripts/multiplayer_menu.gd` — query ControlsConfig for H/J/B
- `godot/scripts/save_dialog.gd` — query ControlsConfig for ESC handling only
  (note: uses `_input()`, not `_unhandled_input()`, intentionally — see
  `_input` vs `_unhandled_input` note below). The Enter-to-submit behavior
  is driven by the LineEdit's `text_submitted` signal (Godot built-in widget
  behavior), NOT a KEY_ENTER check — it is not migratable to ControlsConfig
  and should not be touched during migration.
- `godot/scripts/load_dialog.gd` — query ControlsConfig (note: uses `_input()`,
  not `_unhandled_input()`, intentionally)
- `godot/scripts/tree_info_panel.gd` — query ControlsConfig for ESC
- `godot/scripts/task_panel.gd` — query ControlsConfig for ESC
- `godot/scripts/structure_list_panel.gd` — query ControlsConfig for ESC
- `godot/scripts/units_panel.gd` — query ControlsConfig for ESC
- `godot/scripts/keybind_help.gd` — unchanged in Phase A (keeps hardcoded
  strings), deleted in Phase C
- `godot/project.godot` — register ControlsConfig autoload

**`_input()` vs `_unhandled_input()` note:** `save_dialog.gd` and
`load_dialog.gd` use `_input()` (not `_unhandled_input()`) because they need
to fire before the ItemList's type-to-search consumes letter keys.
`controls_config.is_action()` works identically in both callbacks — it is a
pure event-matching function with no side effects. The choice of callback is
the caller's responsibility.

**Deleted files (Phase C):**
- `godot/scripts/keybind_help.gd`

## Migration Risks

**Dual ownership of InputMap actions:** Camera movement (WASD, arrows,
PgUp/PgDn, R/F) currently uses Godot InputMap actions defined in
`project.godot`. ControlsConfig updates these at runtime via `InputMap` API
when overrides are loaded. If ControlsConfig fails to load (corrupt JSON),
the InputMap retains the `project.godot` defaults — this is intentional
graceful degradation, not a bug.

**ESC precedence chain:** ESC is the highest-risk migration because 10+
handlers share it. The migration MUST NOT change node tree order or introduce
new consumers. Since every handler already calls `set_input_as_handled()`,
replacing `key.keycode == KEY_ESCAPE` with `controls_config.is_action(event,
"ui_cancel")` is a 1:1 substitution that preserves precedence.

## CRITICAL: Binding Conflict Policy

**Future Claude instances: READ THIS CAREFULLY.**

The `ControlsConfig` autoload is the SINGLE SOURCE OF TRUTH for all input
bindings. When adding new features that need keyboard or mouse bindings:

1. **Check ControlsConfig first.** Every bound key is listed there. Do NOT
   grep for KEY_* in individual files — they all delegate to ControlsConfig.
2. **Do NOT silently add a new binding that conflicts with an existing one.**
   Even if the contexts seem different (e.g., "this key is only used during
   construction"), ASK THE HUMAN before assigning any key that appears anywhere
   in the bindings dictionary.
3. **Add new bindings to ControlsConfig**, not as hardcoded KEY_* checks in
   the handler file. Every new binding must have a category, label, context,
   and default.
4. **Update the settings screen categories** if the new binding doesn't fit
   existing categories.

Violating these rules creates exactly the kind of scattered, conflicting
input handling this feature was designed to eliminate.
