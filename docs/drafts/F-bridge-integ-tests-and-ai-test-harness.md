# AI Test Harness and Bridge Integration Tests

**Tracker:** F-bridge-integ-tests, F-ai-test-harness
**Status:** Draft

## Overview

This document covers two tracker items that are really one system. The AI
test harness (F-ai-test-harness) is the infrastructure: a toolkit for
programmatically booting, observing, and interacting with the full game.
The bridge integration tests (F-bridge-integ-tests) are the first tests
written using that infrastructure.

The harness has two layers — low-level bridge calls for fixture generation,
time control, and sim-truth assertions, and high-level UI interaction for
testing the actual player experience. Tests mix both layers freely.

## Motivating incidents

1. A segfault caused by an `Array<GString>` vs `VarArray` type mismatch in
   a bridge function. The Rust code compiled, the GDScript code parsed, but
   the runtime FFI call crashed. Only a test that actually calls the bridge
   function from GDScript in a running Godot process can catch this.

2. A crafting UI that displayed inventory items without material info. The
   Rust sim was correct, the bridge serialized correctly, but the GDScript
   panel read the wrong dict key. Unit tests on either side couldn't catch
   this because the bug lived in the full vertical slice.

## Core principles

1. **Real game scene.** Tests instantiate the actual `main.tscn` as a child
   node, not a stripped-down test scene or bare SimBridge node. The full
   scene tree — camera, UI panels, selection controller, renderers — is
   live. (See "Scene loading strategy" for why we instantiate rather than
   change scenes.)

2. **Load via UI like a human.** Tests don't call bridge functions to set up
   state. They trigger a save load via the same GameSession path the main
   menu uses, and the game starts exactly as a player would experience it.
   The save file is the test fixture.

3. **Test saves are generated, not checked in.** Each test generates its
   own save file programmatically before the UI-driven portion begins. This
   is done via direct bridge calls: `init_sim(seed)`, spawn creatures, step
   ticks, `save_game_json()`. The generated JSON is written to a temp
   location. This avoids save format drift — if the format changes, tests
   regenerate rather than needing manual save file updates.

4. **UI-driven actions.** Tests interact with the game primarily through
   the UI: moving the camera, clicking on creatures, pressing keyboard
   shortcuts, reading panel text, checking button states. This catches the
   full vertical slice: sim state -> bridge serialization -> GDScript
   rendering -> UI display -> input handling.

5. **Bridge calls for setup, time control, and deep assertions.** Bridge
   functions are used for:
   - Generating test saves (before loading)
   - Time control: `step_exactly(n)` to advance ticks deterministically
   - Deep assertions on sim truth that aren't visible in UI (e.g.,
     verifying internal creature state)
   - The game starts **paused on load** via F-config-file's
     `start_paused_on_load` setting (see "Start-paused behavior" below)

6. **Hybrid approach with strong UI preference.** When testing something,
   prefer UI interaction over bridge calls. Use bridge calls when the UI
   path would be impractical (e.g., spawning 50 goblins for a combat test
   save, or checking that a creature's internal HP changed by exactly 1).

7. **Poll, don't guess.** After dispatching UI interactions (clicks, key
   presses), poll for the expected result using `_wait_for` rather than
   awaiting a fixed number of frames. After stepping sim ticks, poll for
   completion using `_step_until` rather than hardcoding tick counts. This
   makes tests robust without needing baseline calibration.

8. **Named nodes.** Key UI panels created dynamically in `main.gd`'s
   `_setup_common()` get explicit `.name` assignments (e.g.,
   `_panel.name = "CreatureInfoPanel"`). This gives tests a stable contract
   for node discovery and improves debuggability in Godot's remote debugger
   regardless of testing.

## Two-layer toolkit

### Low-level: bridge calls

Init sim, step ticks, spawn creatures, query state, save/load. Used for
fixture generation, time control, and sim-truth assertions.

Key bridge functions for test infrastructure:
- `init_sim(seed)` — create a deterministic sim
- `step_to_tick(n)` — advance time to an absolute tick (exists today)
- `step_exactly(n)` — advance by exactly N ticks (new, see below)
- `spawn_creature(species, x, y, z)` — populate the world
- `save_game_json()` / `load_game_json(json)` — save/load
- `get_creature_info_by_id(id, tick)` — deep state inspection
- `get_home_tree_info()` — world state queries
- `current_tick()`, `elf_count()`, etc. — basic state reads

### High-level: UI interaction

Move camera, click at screen coordinates, read panel text, check button
states, press keyboard shortcuts. Used for testing the actual player
experience.

Key techniques:
- **Camera positioning:** Set `CameraPivot.position` directly, or call
  the orbital camera's exported properties to set zoom/rotation.
- **World-to-screen projection:** Use `Camera3D.unproject_position()` to
  convert a known world position to screen coordinates for clicking.
  Note: requires a valid camera transform, which needs empirical
  verification under headless xvfb (see "Risks" section).
- **Synthetic input:** Create `InputEventMouseButton` or `InputEventKey`
  and dispatch via `Input.parse_input_event()` for full pipeline testing,
  or call `_unhandled_input()` directly on specific nodes.
- **Panel text reading:** Access `Label.text`, `RichTextLabel.get_parsed_text()`,
  `Button.text` on UI nodes.
- **Visibility checks:** Read `Control.visible` on panels and buttons.
- **Node access:** Use `find_child()` with named nodes (names assigned as
  part of this feature's prerequisites — see "Scene tree reference").

### Example workflow

A test that verifies creature selection works end-to-end:

1. **Bridge:** Generate save — `init_sim(42)`, `step_to_tick(50)`, get an
   elf's UUID and position via `get_creature_uuid()` and
   `get_creature_info()`, call `save_game_json()`, write to temp file.
2. **UI:** Load the save via `_load_game_scene()`, which instantiates
   `main.tscn` as a child and polls until SimBridge is initialized.
3. **UI:** Position camera — set `CameraPivot.position` to the elf's known
   world position, set zoom close enough to see it.
4. **UI:** Click on elf — use `Camera3D.unproject_position()` to get screen
   coords of the elf's world position (with species Y offset from
   `Main.SPECIES_Y_OFFSETS`), create and dispatch a click event at those
   coords.
5. **UI:** Assert info panel visible — poll with `_wait_for` until the
   creature info panel ("CreatureInfoPanel") is visible and its species
   label reads "Elf".
6. **Bridge:** Verify selection in sim — call
   `get_creature_info_by_id(uuid, tick)` to confirm the correct creature
   is actually selected (cross-reference with what the panel shows).
7. **Bridge:** Step time — `step_exactly(100)` to advance.
8. **UI:** Yield frames with `_wait_for` to let UI catch up (sim ticks
   don't trigger `_process`), then verify panel updated — task label
   text should reflect the creature's current activity.

## Deterministic time control

### Bridge addition: `step_exactly(n: int)` (NEW — does not exist yet)

A new bridge function that advances the simulation by exactly N ticks,
synchronously. Implemented as `step_to_tick(current_tick() + n)` under
the hood, but provides a clearer API for tests. Ignores the speed
multiplier and wall clock entirely — gives tests direct, deterministic
tick control without going through the frame-loop pacing in `LocalRelay`.

**Critical constraint:** The sim MUST be paused before calling
`step_exactly`. If the sim is unpaused, `main.gd._process()` calls
`bridge.frame_update(delta)` every frame, which also advances ticks via
`LocalRelay`. Calling `step_exactly` while unpaused would double-advance
ticks and produce unpredictable state. The start-paused-on-load behavior
(below) ensures this constraint is met for all tests.

### GDScript-side pattern: `_step_until`

A helper that advances tick-by-tick until a GDScript-side predicate is met:

```gdscript
## Advance tick-by-tick until predicate returns true, or timeout.
## Returns ticks stepped, or -1 on timeout.
func _step_until(predicate: Callable, max_ticks: int) -> int:
    var stepped := 0
    while stepped < max_ticks:
        _bridge.step_exactly(1)
        stepped += 1
        if predicate.call():
            return stepped
    return -1
```

This is faster than hardcoded tick numbers (stops as soon as the condition
is met) and robust against timing shifts from config changes.

**Important:** `_step_until` runs in a tight loop without yielding
frames. The predicate can only check sim/bridge state, not UI state
(panel text, visibility). GDScript `_process()` code that updates UI in
response to sim changes will not run between ticks. After any
`_step_ticks` or `_step_until` call, tests must follow up with
`await _wait_for(...)` to let the UI catch up before asserting on
UI state.

### Start-paused behavior

When tests load a save, the game starts paused so tests can set up
assertions before time advances. This uses F-config-file's
`start_paused_on_load` setting — a standard game option (useful for
players loading saves in dangerous situations, not just tests). The
GameConfig autoload reads it on startup; `main.gd` checks it and calls
`bridge.set_sim_speed("Paused")` in `_setup_common()`.

**Note:** The GameConfig autoload does not exist yet — it is part of
F-config-file and must be implemented as a prerequisite (see
"Prerequisites"). Tests enable it via
`GameConfig.override_setting("start_paused_on_load", true)` before
loading a save. The override lives only in memory — no file writes, no
cleanup needed.

**Fallback if F-config-file is not ready:** Tests can call
`bridge.set_sim_speed("Paused")` immediately after scene load. There is
a small race window (a few ticks may process before the call), but for
Phase 1 proof-of-concept this is acceptable. The config-based approach
eliminates the race entirely.

### Sim-speed pause vs tree pause

Godot has two independent pause mechanisms. Tests must understand the
difference:

- **Sim-speed pause** (`bridge.set_sim_speed("Paused")`): Sets the
  speed multiplier to zero in `LocalRelay`. No sim ticks advance, but
  `main.gd._process()` still runs every frame — renderers refresh,
  UI updates, input is processed. This is what tests use for normal
  time control.

- **Tree pause** (`get_tree().paused = true`): Freezes `_process()` and
  `_physics_process()` for all nodes with default process mode
  (`PROCESS_MODE_INHERIT`). The escape menu uses this — it sets
  `get_tree().paused = true` when opened and uses
  `PROCESS_MODE_ALWAYS` on itself to stay responsive.

Tests use **sim-speed pause only** (not tree pause). This keeps
renderers and UI responsive so `_wait_for` polls work and UI state
can be read. The `_process()` loop still calls `frame_update()` each
frame, but with zero speed no ticks advance — this is safe.

**Pause menu caveat:** Test 4 opens the escape menu, which sets
`get_tree().paused = true`. This freezes the test node (which uses
default process mode), preventing `await` from resuming. Tests that
interact with the escape menu must set
`process_mode = Node.PROCESS_MODE_ALWAYS` in `before_each()` so they
continue processing while the tree is paused.

## Scene loading strategy

GUT tests live in the scene tree. Calling
`get_tree().change_scene_to_file()` would free the current scene — including
the test node itself — making subsequent assertions undefined behavior.

Instead, tests use `PackedScene.instantiate()` to create the Main node
as a child of the test:

```gdscript
var _main_scene: Node

func _load_game_scene(save_json: String) -> void:
    # Write save to temp location
    var path := "user://saves/_test_fixture_%d.json" % randi()
    var file := FileAccess.open(path, FileAccess.WRITE)
    file.store_string(save_json)
    file.close()
    # Configure GameSession for load
    GameSession.load_save_path = path
    if GameConfig:
        GameConfig.override_setting("start_paused_on_load", true)
    # Instantiate main scene as child (not change_scene)
    var packed := load("res://scenes/main.tscn") as PackedScene
    _main_scene = packed.instantiate()
    add_child(_main_scene)
    # Wait for setup to complete
    var ready := await _wait_for(func():
        var bridge = _main_scene.get_node_or_null("SimBridge")
        return bridge != null and bridge.is_initialized())
    assert_true(ready, "Game scene failed to initialize")
    # Fallback pause if GameConfig not available
    if not GameConfig:
        _main_scene.get_node("SimBridge").set_sim_speed("Paused")
```

Setup and cleanup:
```gdscript
func before_each() -> void:
    # Ensure test node keeps processing even when tree is paused
    # (needed for Test 4 which opens the escape menu)
    process_mode = Node.PROCESS_MODE_ALWAYS

func after_each() -> void:
    if _main_scene:
        _main_scene.queue_free()
        _main_scene = null
    # Clean up temp save files
    var dir := DirAccess.open("user://saves")
    if dir:
        for f in dir.get_files():
            if f.begins_with("_test_fixture_"):
                dir.remove(f)
```

This approach keeps the test node alive throughout, avoids scene-change
lifecycle issues, and ensures temp files don't pollute the player's
Load Game dialog.

**Autoload interactions:** When `main.tscn` is instantiated as a child,
autoloads like `GameSession` are still accessible (they live on the
scene tree root). Key things to know:
- `GameSession.load_save_path` must be set before instantiation.
- `GameSession._ready()` starts the elfcyclopedia HTTP server (static
  state — runs once regardless of how many SimBridges are created).
- `GameSession._ready()` calls `get_tree().set_auto_accept_quit(false)`.
  This only affects window close requests, not `get_tree().quit()`, so
  GUT's `--quit-when-done` behavior is unaffected.

**Fixture bridge independence:** The SimBridge created in
`_generate_save` is completely independent from the one in `main.tscn`.
Each bridge owns its own sim session. Two sessions exist briefly during
the transition (fixture generation → game load), but they don't
interact.

## Scene readiness

After instantiating `main.tscn`, tests must wait until `main.gd._ready()`
and `_setup_common()` have fully completed before interacting.

The primary mechanism is a `setup_complete` signal added to `main.gd`,
emitted at the end of `_setup_common()`. This is useful beyond testing —
other systems may want to know when the game is fully ready.

As a backup, tests can poll using `_wait_for`:
```gdscript
await _wait_for(func():
    var bridge = _main_scene.get_node_or_null("SimBridge")
    return bridge != null and bridge.is_initialized())
```
Polling is a reasonable general-purpose pattern for UI integration tests
where signal wiring isn't available.

## GDScript UI introspection capabilities

GDScript has full access to the scene tree and all UI state. This is the
foundation of the high-level testing layer.

### Node tree traversal

- `get_children()` — immediate children as `Array[Node]`
- `get_node(path)` / `$Path` — by relative or absolute path
- `get_tree().root` — scene root access
- `find_child(pattern, recursive, owned)` — wildcard name search
- `get_node_or_null(path)` — safe variant returning null on miss

### Control properties

All UI nodes inherit from `Control` and expose:
- `visible` — whether the node is rendered (cascading)
- `size` / `position` — layout geometry
- `modulate` / `self_modulate` — color tinting
- `mouse_filter` — input handling mode

### Text content

- `Label.text` — plain text (read/write)
- `RichTextLabel.text` — BBCode text; `.get_parsed_text()` strips markup
- `LineEdit.text` — single-line input field
- `Button.text` — button label text

### Button and interaction state

- `BaseButton.pressed` — toggle state
- `BaseButton.disabled` — grayed out
- `OptionButton.selected` — current index
- `ItemList.is_selected(idx)` — item selection

### Programmatic input simulation

```gdscript
# Key press:
var key := InputEventKey.new()
key.keycode = KEY_HOME
key.pressed = true
Input.parse_input_event(key)  # Full pipeline
# Or: node._unhandled_input(key)  # Direct to handler

# Mouse click:
var click := InputEventMouseButton.new()
click.button_index = MOUSE_BUTTON_LEFT
click.pressed = true
click.position = Vector2(100, 200)
Input.parse_input_event(click)
```

Existing tests (`test_orbital_camera.gd`) demonstrate creating
`InputEventKey` events and calling `_unhandled_input()` directly.

### Viewport and rendering

- `get_viewport().get_texture().get_image()` — viewport screenshot
- `Camera3D.unproject_position(world_pos)` — world-to-screen conversion
- `Camera3D.project_position(screen_pos, depth)` — screen-to-world

Note: headless Godot with xvfb does not produce GPU-rendered 3D frames.
Screenshots will show UI elements but not the 3D world. Full visual testing
would require a headed Godot instance.

## Helper library

A GDScript utility class providing reusable test primitives. Starts as
helper functions within the test file, following the project's existing
pattern (see `test_status_bar.gd`'s `_get_speed_label()` helper). Extract
to a standalone `test_harness.gd` class if they grow large enough.

### Fixture generation

```gdscript
## Create a save file programmatically. Calls setup_fn with a SimBridge
## so the test can configure the world. Returns the JSON string.
func _generate_save(setup_fn: Callable) -> String:
    var bridge := SimBridge.new()
    add_child(bridge)
    setup_fn.call(bridge)
    var json := bridge.save_game_json()
    bridge.queue_free()
    return json
```

Note: uses `add_child()` + `queue_free()` rather than
`add_child_autofree()` to control the bridge's lifetime explicitly.
The bridge is short-lived fixture generation code that should be freed
before the game scene loads.

### Scene loading

See "Scene loading strategy" above for the full `_load_game_scene`
implementation.

### Camera and clicking

```gdscript
## Position the camera to look at a world position.
func _move_camera_to(world_pos: Vector3) -> void:
    var pivot := _main_scene.get_node("CameraPivot")
    pivot.position = world_pos

## Click at a world position by projecting to screen coords.
## Sends a motion event first to establish cursor position (some
## controls only update hover state on motion), then press+release
## in the same frame (treated as a click, not a drag).
func _click_at_world_pos(world_pos: Vector3) -> void:
    var camera := _main_scene.get_node("CameraPivot/Camera3D")
    var screen_pos := camera.unproject_position(world_pos)
    # Move cursor to position first
    var motion := InputEventMouseMotion.new()
    motion.position = screen_pos
    Input.parse_input_event(motion)
    # Press
    var click := InputEventMouseButton.new()
    click.button_index = MOUSE_BUTTON_LEFT
    click.pressed = true
    click.position = screen_pos
    Input.parse_input_event(click)
    # Release
    var release := InputEventMouseButton.new()
    release.button_index = MOUSE_BUTTON_LEFT
    release.pressed = false
    release.position = screen_pos
    Input.parse_input_event(release)
```

### UI reading

```gdscript
## Read text from a Label or RichTextLabel by name (recursive search).
func _read_panel_text(node_name: String) -> String:
    var node := _main_scene.find_child(node_name, true, false)
    if node is RichTextLabel:
        return node.get_parsed_text()
    elif node is Label:
        return node.text
    return ""

## Check if a named Control node is visible (recursive search).
func _is_panel_visible(node_name: String) -> bool:
    var node := _main_scene.find_child(node_name, true, false)
    return node != null and node.visible
```

Note: uses `find_child()` with `recursive=true` because panels are
children of CanvasLayer nodes, not direct children of Main.

### Time control

```gdscript
## Step the sim exactly N ticks via the bridge. Sim MUST be paused.
## After calling, use _wait_for to let UI catch up before UI assertions.
func _step_ticks(n: int) -> void:
    _get_bridge().step_exactly(n)

## Step tick-by-tick until predicate, or timeout. Returns ticks or -1.
## Predicate can only check bridge/sim state (not UI) — no frames are
## yielded between ticks. Follow with _wait_for for UI assertions.
func _step_until(predicate: Callable, max_ticks: int) -> int:
    var bridge := _get_bridge()
    var stepped := 0
    while stepped < max_ticks:
        bridge.step_exactly(1)
        stepped += 1
        if predicate.call():
            return stepped
    return -1

func _get_bridge() -> SimBridge:
    return _main_scene.get_node("SimBridge")
```

### Frame polling

```gdscript
## Poll each frame until predicate is true, or fail after max_frames.
func _wait_for(predicate: Callable, max_frames: int = 30) -> bool:
    for i in max_frames:
        if predicate.call():
            return true
        await get_tree().process_frame
    return false
```

Used for all UI assertions (panel visibility, text content, scene load
completion). The frame-domain equivalent of `_step_until` in the tick
domain.

### Assertion helpers

```gdscript
## Assert a dictionary has all expected keys.
func _assert_has_keys(dict: Dictionary, keys: Array, msg: String) -> void:
    for key in keys:
        assert_true(dict.has(key), "%s: missing key '%s'" % [msg, key])
```

## Scene tree reference

**This section describes the TARGET state after prerequisites are
implemented.** Currently, most dynamic nodes do not have `.name`
assignments — adding them is a Phase 1 prerequisite.

When main.tscn loads, the scene tree is built programmatically by
`main.gd`'s `_setup_common()`. As part of this feature, key nodes get
explicit `.name` assignments for test discoverability.

**Static nodes (in main.tscn — exist today):**
- `SimBridge` — the SimBridge native class
- `CameraPivot` — orbital camera controller (orbital_camera.gd)
- `CameraPivot/Camera3D` — the actual Camera3D
- `TreeRenderer`, `ElfRenderer`, `CapybaraRenderer` — renderers

**Dynamic nodes (created in _setup_common — names to be added):**
- `"SelectionController"` — `Node3D` with selection_controller.gd
- `"CreatureInfoPanel"` — `PanelContainer` on CanvasLayer 3
- `"GroupInfoPanel"` — `PanelContainer` on CanvasLayer 3
- `"StructureInfoPanel"` — `PanelContainer` on CanvasLayer 3
- `"GroundPileInfoPanel"` — `PanelContainer` on CanvasLayer 3
- `"TreeInfoPanel"` — `PanelContainer` on CanvasLayer 1
- `"TaskPanel"` — `ColorRect` on CanvasLayer 2
- `"UnitsPanel"` — `ColorRect` on CanvasLayer 2
- `"StructureListPanel"` — `ColorRect` on CanvasLayer 2
- `"HelpPanel"` — `ColorRect` on CanvasLayer 2
- `"EscapeMenu"` — `ColorRect` on CanvasLayer 2
- `"MilitaryPanel"` — `PanelContainer` on CanvasLayer 3
- `"StatusBar"` — `PanelContainer` on base CanvasLayer
- `"Minimap"` — `PanelContainer` on base CanvasLayer
- `"ActionToolbar"` — `MarginContainer` on base CanvasLayer

Tests locate these via `_main_scene.find_child("CreatureInfoPanel", true,
false)`. The `recursive=true` parameter is needed because panels are
children of intermediate CanvasLayer nodes (which have auto-generated
names like "CanvasLayer4"), not direct children of Main. Consider also
naming the CanvasLayers (e.g., `"InfoPanelLayer"`, `"OverlayLayer"`,
`"EscapeMenuLayer"`) for debuggability, though this is not strictly
required for tests since `find_child` with recursion handles it.

## Test designs

### Test 1: Game startup and world display

**Goal:** Verify that loading a save via the normal UI flow produces a
playable game with correct initial state visible in the UI.

**Fixture generation:**
1. Create SimBridge, `init_sim(42)`, `step_to_tick(100)`
2. Record: `elf_count()`, `get_home_tree_info()` (tree position, mana)
3. `save_game_json()`, write to temp file

**UI-driven test:**
1. Load save via `_load_game_scene()`, which handles GameSession setup,
   config override, and scene readiness polling
2. Verify the SimBridge in the scene is initialized:
   `_get_bridge().is_initialized() == true`
3. Verify `_get_bridge().current_tick()` matches the saved tick (100)
4. Verify the status bar ("StatusBar") is visible and shows population
   count matching the recorded elf count
5. Verify the camera pivot is at its default position
   (`Vector3(128, 20, 128)` from the .tscn initial transform)
6. Press Home key (synthetic `InputEventKey` with `KEY_HOME`) — poll with
   `_wait_for` until camera pivot moved to the home tree's position
7. Verify escape menu ("EscapeMenu") is hidden

**Bridge assertions:**
- `get_home_tree_info()` returns a dict with expected keys
- `elf_count() > 0`

### Test 2: Creature selection and info panel

**Goal:** Verify the full click-to-select-to-info-panel pipeline: clicking
on a creature sprite in the viewport causes the creature info panel to
appear with correct data.

**Fixture generation:**
1. Create SimBridge, `init_sim(42)`, `step_to_tick(50)`
2. Get first elf: `get_creature_uuid("Elf", 0)` -> uuid. Note: index 0
   means "first elf in internal order" — the specific elf may vary if
   sim internals change. Tests should only rely on getting *a valid* elf,
   not a *specific* one.
3. Get elf info: `get_creature_info_by_id(uuid, 50.0)` -> record species,
   name, position (x, y, z)
4. `save_game_json()`, write to temp file

**UI-driven test:**
1. Load save via `_load_game_scene()`, await scene ready
2. Compute elf world position: `Vector3(x + 0.5, y + 0.48, z + 0.5)`
   (0.48 is the elf Y offset from `Main.SPECIES_Y_OFFSETS`)
3. Position camera close to the elf: set `CameraPivot.position` to
   `Vector3(x + 0.5, y + 5, z + 0.5)`, ensure camera is zoomed in
   enough (set the camera pivot's zoom distance to ~15)
4. Use `Camera3D.unproject_position(elf_world_pos)` to get screen coords
5. Dispatch a click event at those screen coords via `_click_at_world_pos`
6. Poll with `_wait_for` until "CreatureInfoPanel" is visible
7. Read the species label text — should contain "Elf"
8. Read the name label text — should match the recorded name

**Bridge assertions:**
- `get_creature_info_by_id(uuid, tick)` should return non-empty dict

### Test 3: Construction workflow via UI

**Goal:** Verify the build-mode-to-platform-construction pipeline using
keyboard shortcuts and UI interactions.

**Note:** This is the most complex test — the construction UI involves a
multi-step flow (enter build mode → select type → specify placement →
confirm). The exact interaction sequence depends on
`construction_controller.gd` and `placement_controller.gd`, which should
be consulted during implementation. The steps below are approximate.

**Fixture generation:**
1. Create SimBridge, `init_sim(42)`, `step_to_tick(200)` (let tree grow,
   elves settle)
2. Find a valid build position near the tree: get tree anchor from
   `get_home_tree_info()`, then scan outward at y = anchor_y + 5
   (above the trunk base) in a spiral or grid pattern, calling
   `validate_build_position(x, y, z)` until one returns true. Positions
   must be Air with at least one solid neighbor (tree trunk/branch).
3. Record the valid build position and tree position
4. `save_game_json()`, write to temp file

**UI-driven test:**
1. Load save, await scene ready
2. Position camera near the tree (set pivot to tree position)
3. Press B key to enter construction mode (synthetic `InputEventKey` with
   `KEY_B` dispatched through the full input pipeline)
4. Poll with `_wait_for` until the construction panel is visible
5. The platform placement flow requires clicking on valid build positions.
   Use `Camera3D.unproject_position()` to convert the known valid build
   position to screen coords, then click
6. Poll with `_wait_for` for the construction command to propagate

**Bridge assertions:**
- `get_blueprint_voxels()` should be non-empty after placement
- Use `_step_until(func(): return _get_bridge().get_structures().size() > 0,
  10000)` to wait for construction completion
- Once complete, `get_structures()` should contain at least one structure
  with expected keys

### Test 4: Save/load round-trip via escape menu

**Goal:** Verify that saving through the escape menu UI and reloading
produces identical game state.

**Note:** The save dialog interaction (finding Save Game button, entering
a save name, confirming) depends on `escape_menu.gd`'s node structure,
which should be consulted during implementation. If the dialog flow proves
too complex to automate reliably, this test can fall back to using
`save_game_json()` via bridge for the save step while still testing the
load-via-UI path.

**Fixture generation:**
1. Create SimBridge, `init_sim(42)`, `step_to_tick(300)`
2. Spawn a capybara at a known position
3. `step_to_tick(400)`
4. Record snapshot: tick, elf_count, creature_count_by_name("Capybara"),
   home_tree_mana, fruit_count
5. `save_game_json()`, write to temp file

**UI-driven test:**
1. Load save, await scene ready
2. Press ESC to open escape menu
3. Poll with `_wait_for` until "EscapeMenu" is visible
4. Find and click the Save Game button within "EscapeMenu"
5. Interact with save dialog (enter name, confirm) — exact node paths
   TBD during implementation based on escape_menu.gd
6. After saving, close escape menu (press ESC again)
7. Step time to modify state: `_step_ticks(100)` via bridge
8. Reload by creating a new `_load_game_scene()` with the saved file
9. Poll with `_wait_for` until scene ready, verify state matches the
   pre-save snapshot: tick should match the original saved tick (not the
   post-step tick)

**Bridge assertions:**
- `current_tick()` after reload matches original save tick
- `elf_count()` matches
- `creature_count_by_name("Capybara")` matches
- `home_tree_mana()` is close (float comparison with tolerance)

### Test 5: Sprite generation (standalone)

**Goal:** Verify sprite generation works for all species and fruit shapes.
This test does NOT need the full game scene — it exercises the
`SpriteGenerator` class directly.

**Steps:**
1. `SpriteGenerator.species_sprite("Elf", 42)` — assert non-null
   `ImageTexture` with positive dimensions
2. Repeat for all species: Capybara, Boar, Deer, Elephant, Goblin,
   Monkey, Orc, Squirrel, Troll
3. `SpriteGenerator.species_sprite("InvalidSpecies", 0)` — assert null
4. `SpriteGenerator.fruit_sprite("Round", 200, 100, 50, 100, false)` —
   assert non-null with positive dimensions (params: shape, r, g, b,
   size_percent, glows)
5. Test each fruit shape: Oblong, Clustered, Pod, Nut, Gourd
6. `SpriteGenerator.fruit_sprite_from_dict({})` — verify no crash

**Assertions catch:** Pixel buffer to ImageTexture conversion failures,
species name string marshalling, null-safety on unknown species.

### Test 6: Military groups via UI

**Goal:** Verify the military panel UI works end-to-end: creating groups,
assigning members, and seeing the results in the panel.

**Fixture generation:**
1. Create SimBridge, `init_sim(42)`, `step_to_tick(100)`
2. Call `create_military_group("Alpha Squad")` — this is a fire-and-forget
   command that returns void
3. `step_to_tick(101)` — step a tick so the command processes
4. Call `get_military_groups()`, find the group named "Alpha Squad" to
   get its ID (since `create_military_group` doesn't return the ID)
5. Get an elf UUID, call `reassign_military_group(uuid, group_id)`
6. `step_to_tick(102)` — step again so the assignment processes
7. Record the group_id and elf uuid
8. `save_game_json()`, write to temp file

**UI-driven test:**
1. Load save, await scene ready
2. Press M key to open the military panel (synthetic input)
3. Poll with `_wait_for` until "MilitaryPanel" is visible
4. Look for "Alpha Squad" text somewhere in the panel's label children
5. Look for the group member count (should be 1)
6. Press M again to close the panel, poll with `_wait_for` until hidden

**Bridge assertions:**
- `get_military_groups()` returns array containing the created group
- `get_military_group_members(group_id)` returns array with 1 member
- After closing and reopening, state is still consistent

## Build target and CI

These tests run alongside existing GDScript tests via `scripts/build.py
gdtest`. The test file follows the `test_` prefix convention and lives in
`godot/test/`, so GUT auto-discovers it via `.gutconfig.json`.

**Timeout consideration:** Full-scene tests are substantially slower than
pure-logic unit tests. Each test that instantiates `main.tscn` initializes
a sim (including tree generation), sets up all renderers, and creates the
full UI hierarchy. Expect 3-8 seconds per scene-loading test vs <1 second
for unit tests. The existing 60-second GUT timeout (`GUT_TIMEOUT` in
`build.py`) should be sufficient for 5 scene tests + 1 standalone test,
but this needs monitoring. If the full suite approaches the timeout, we
can:
- Increase `GUT_TIMEOUT`
- Split scene tests into a separate `build.py` target
- Share a single loaded scene across multiple test methods (load once in
  `before_all`, run multiple assertions)

**CI:** No CI changes needed. The existing `gdscript-unit-tests` job
builds the Rust GDExtension library before running GUT, so the SimBridge
native class is available.

## Risks

- **Headless `unproject_position`:** Click-to-select tests depend on
  `Camera3D.unproject_position()` producing valid screen coordinates under
  headless xvfb. The camera transform should be valid (it's set explicitly
  by tests), but this needs empirical verification in Phase 1. If it
  doesn't work, tests can fall back to dispatching clicks via the
  selection controller's API directly.

- **GUT + instantiated scenes:** Using `PackedScene.instantiate()` to load
  `main.tscn` as a child of the test node is non-standard for GUT. The
  main scene's `_ready()` and `_process()` will run within the test's
  scene tree. This should work but may surface edge cases (e.g., autoload
  singletons, input focus). Phase 1 validates this approach.

- **CI performance budget:** Five scene-loading tests at 3-8 seconds each
  could push the GUT suite to 20-40 seconds, approaching the 60-second
  timeout. Monitor in CI and increase `GUT_TIMEOUT` if needed.

## Implementation plan

### Prerequisites (things that don't exist yet)

- **F-config-file** (or minimal subset): GameConfig GDScript autoload
  (does not exist today) with `override_setting()` support and
  `start_paused_on_load` option. Can be deferred if using the
  `set_sim_speed("Paused")` fallback.
- **`setup_complete` signal** in `main.gd` (does not exist today).
- **Named nodes** in `main.gd`'s `_setup_common()` (most dynamic nodes
  do not have `.name` assignments today — see scene tree reference).
- **`step_exactly(n: int)`** bridge function in SimBridge (does not
  exist today — wraps `step_to_tick(current_tick() + n)`).

### File structure

Primary test file: `godot/test/test_harness_integration.gd`. Contains both
the helper functions and the test methods.

If the helpers grow large enough to warrant separation, extract them to
`godot/test/harness_helpers.gd` as a standalone class. But start with
everything in one file for simplicity.

### Phased implementation

**Phase 1: Infrastructure and proof of concept**
- Add `step_exactly()` to SimBridge
- Add `setup_complete` signal to `main.gd`
- Add `.name` assignments to key dynamic nodes in `_setup_common()`
- Write the helper library (fixture generation, scene loading, camera/click
  utilities, frame polling, assertion helpers)
- Implement Test 1 (startup) and Test 5 (sprites) as proof of concept
- Validate: does `PackedScene.instantiate()` work with GUT? Does
  `unproject_position` work under headless xvfb?
- Verify they run in CI via `build.py gdtest`

**Phase 2: Selection and interaction**
- Implement Test 2 (creature selection)
- Implement Test 6 (military panel)

**Phase 3: Complex workflows**
- Implement Test 3 (construction) — consult `construction_controller.gd`
  and `placement_controller.gd` for the full interaction sequence
- Implement Test 4 (save/load round-trip) — consult `escape_menu.gd` for
  the save dialog node structure
- These depend on `_step_until` working reliably with the full scene
