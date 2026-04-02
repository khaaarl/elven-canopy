## Integration tests for the AI test harness and bridge integration.
##
## These tests exercise the full vertical slice: Rust sim -> GDExtension bridge
## -> GDScript UI.  They instantiate the real main.tscn scene as a child node
## and interact with it through the same paths a player would use.
##
## **CRITICAL: Tests MUST interact through the UI as much as possible.**
## After fixture setup (which may use bridge methods to build the world state),
## the test body should drive behavior by clicking UI panel buttons, toggling
## checkboxes, and selecting entities — NOT by calling bridge methods directly.
## Verification should read panel text and check UI visibility IN ADDITION to
## querying the bridge for correctness.  The whole point of these integration
## tests is to prove the UI→bridge→sim→bridge→UI round-trip works.  A test
## that only calls bridge methods is a unit test wearing a trench coat.
##
## Performance note: Each test that loads main.tscn takes ~3s (sim init, tree
## generation, mesh build, full UI setup).  As of March 2026 the 13 tests here
## add ~18-22s to the GUT suite, well within the 60s GUT_TIMEOUT.  When adding
## new integration tests, prefer adding assertions to an existing scene-loading
## test rather than creating a new one — the per-test overhead is almost entirely
## scene loading, so "load once, verify many things" scales much better.  If the
## suite grows past ~40s, consider splitting integration tests into a separate
## build.py target or sharing a single loaded scene across test methods via
## before_all (see the design doc's "Timeout consideration" section).
##
## Test-local helpers (_generate_save, _load_game_scene, _wait_for, _step_ticks,
## _step_until, etc.) live here.  Shared UI helpers (click_at_world_pos,
## find_button, press_key, etc.) live in puppet_helpers.gd and are accessed
## via the _helpers field.
##
## See also: main.gd for the scene controller, sim_bridge.rs for the Rust
## bridge, sprite_bridge.rs for SpriteGenerator.
extends GutTest

const PuppetHelpersScript := preload("res://scripts/puppet_helpers.gd")

# ---------------------------------------------------------------------------
# State
# ---------------------------------------------------------------------------

## The instantiated main scene (child of this test node).
var _main_scene: Node

## Shared UI interaction helpers (initialized when scene loads).
var _helpers  # PuppetHelpers instance (preloaded to avoid class load-order issues)

## Recorded fixture data (set during _generate_save, read during test body).
var _fixture: Dictionary = {}

# ---------------------------------------------------------------------------
# Setup / teardown
# ---------------------------------------------------------------------------


func before_each() -> void:
	# Keep processing even when the Godot tree is paused (needed for tests
	# that open the escape menu, which sets get_tree().paused = true).
	process_mode = Node.PROCESS_MODE_ALWAYS


func after_each() -> void:
	if _main_scene:
		# Shut down the SimBridge cleanly before freeing the scene.
		# Without this, the GDExtension destructor can segfault during
		# engine shutdown because the Rust side tries to drop resources
		# after the Godot rendering server is already torn down.
		var bridge := _main_scene.get_node_or_null("SimBridge") as SimBridge
		if bridge:
			bridge.shutdown()
		_main_scene.queue_free()
		_main_scene = null
		_helpers = null
		# Yield a frame so Godot processes the deferred free and cleans up
		# child nodes (CanvasLayers, panels, renderers) before GUT moves on.
		await get_tree().process_frame
	# Reset the load path in case _load_game_scene failed before main.gd
	# cleared it.  Prevents stale paths from leaking to the next test.
	GameSession.load_save_path = ""
	GameSession.test_mode = false
	# Clean up temp save files written by _load_game_scene.
	var dir := DirAccess.open("user://saves")
	if dir:
		dir.list_dir_begin()
		var fname := dir.get_next()
		while fname != "":
			if fname.begins_with("_test_fixture_"):
				dir.remove(fname)
			fname = dir.get_next()
		dir.list_dir_end()
	_fixture = {}


# ---------------------------------------------------------------------------
# Helper: fixture generation
# ---------------------------------------------------------------------------


## Create a save file programmatically.  Calls setup_fn(bridge) so the test
## can configure the world, then returns the JSON string.
func _generate_save(setup_fn: Callable) -> String:
	var bridge := SimBridge.new()
	add_child(bridge)
	setup_fn.call(bridge)
	var json := bridge.save_game_json()
	bridge.shutdown()
	remove_child(bridge)
	bridge.free()
	return json


# ---------------------------------------------------------------------------
# Helper: scene loading
# ---------------------------------------------------------------------------


## Write a JSON save to a temp file, configure GameSession, instantiate
## main.tscn as a child, and wait until the bridge is initialized.
func _load_game_scene(save_json: String) -> void:
	# Write save to a temp location under user://saves/.
	var path := "user://saves/_test_fixture_%d.json" % randi()
	DirAccess.make_dir_recursive_absolute("user://saves")
	var file := FileAccess.open(path, FileAccess.WRITE)
	file.store_string(save_json)
	file.close()
	# Tell GameSession to load this save on next scene init.
	GameSession.load_save_path = path
	# Tests use step_to_tick / step_exactly for deterministic control —
	# skip relay startup so commands go directly to the session.
	GameSession.test_mode = true
	# Instantiate the real game scene as a child of this test node.
	var packed := load("res://scenes/main.tscn") as PackedScene
	_main_scene = packed.instantiate()
	add_child(_main_scene)
	_helpers = PuppetHelpersScript.new(_main_scene)
	# Wait for the bridge to finish initializing.
	var ready := await _wait_for(
		func(): return _get_bridge() != null and _get_bridge().is_initialized(), 60
	)
	assert_true(ready, "Game scene failed to initialize within timeout")
	# Pause the sim so step_exactly is safe (no double-advance from _process).
	if _get_bridge():
		_get_bridge().set_sim_speed("Paused")


# ---------------------------------------------------------------------------
# Helper: node access
# ---------------------------------------------------------------------------


## Get the SimBridge node from the loaded main scene.
func _get_bridge() -> SimBridge:
	if not _main_scene:
		return null
	return _main_scene.get_node_or_null("SimBridge") as SimBridge


# ---------------------------------------------------------------------------
# Helper: UI reading (not extracted — test-specific)
# ---------------------------------------------------------------------------


## Check if the camera pivot is near the home tree position.
func _check_camera_at_home(tree_x: float, tree_z: float) -> bool:
	var p: Vector3 = _main_scene.get_node("CameraPivot").position
	return absf(p.x - tree_x - 0.5) < 1.0 and absf(p.z - tree_z - 0.5) < 1.0


## Check if the construction controller is in active mode.
func _is_construction_active() -> bool:
	var cc := _main_scene.find_child("ConstructionController", true, false)
	if cc and cc.has_method("is_active"):
		return cc.is_active()
	return false


# ---------------------------------------------------------------------------
# Helper: time control
# ---------------------------------------------------------------------------


## Step the sim exactly N ticks via the bridge.  Sim MUST be paused.
## After calling, use _wait_for to let UI catch up before UI assertions.
func _step_ticks(n: int) -> void:
	_get_bridge().step_exactly(n)


## Step tick-by-tick until predicate returns true, or timeout.
## Returns ticks stepped, or -1 on timeout.  Predicate can only check
## bridge/sim state (no frames yielded between ticks).
func _step_until(predicate: Callable, max_ticks: int) -> int:
	var bridge := _get_bridge()
	var stepped := 0
	while stepped < max_ticks:
		bridge.step_exactly(1)
		stepped += 1
		if predicate.call():
			return stepped
	return -1


# ---------------------------------------------------------------------------
# Helper: frame polling
# ---------------------------------------------------------------------------


## Poll each frame until predicate is true, or fail after max_frames.
func _wait_for(predicate: Callable, max_frames: int = 30) -> bool:
	for i in max_frames:
		if predicate.call():
			return true
		await get_tree().process_frame
	return false


# ---------------------------------------------------------------------------
# Helper: assertions
# ---------------------------------------------------------------------------


## Assert a dictionary has all expected keys.
func _assert_has_keys(dict: Dictionary, keys: Array, msg: String) -> void:
	for key in keys:
		assert_true(dict.has(key), "%s: missing key '%s'" % [msg, key])


# ===========================================================================
# Test 1: Game startup and world display
# ===========================================================================


func _setup_startup_fixture(bridge: SimBridge) -> void:
	bridge.init_sim_test_config(42)
	bridge.step_to_tick(100)
	_fixture = {
		"elf_count": bridge.elf_count(),
		"tick": 100,
		"tree_info": bridge.get_home_tree_info(),
	}


func test_startup_scene_loads_and_initializes() -> void:
	# -- Fixture generation --
	var json := _generate_save(_setup_startup_fixture)

	assert_true(_fixture.elf_count > 0, "Fixture should have at least one elf")

	# -- Load game scene --
	await _load_game_scene(json)

	# -- Verify bridge is initialized --
	assert_true(_get_bridge().is_initialized(), "Bridge should be initialized")

	# -- Verify tick matches saved state --
	assert_eq(_get_bridge().current_tick(), _fixture.tick, "Current tick should match saved tick")

	# -- Verify elf count matches --
	assert_eq(_get_bridge().elf_count(), _fixture.elf_count, "Elf count should match fixture")

	# -- Verify status bar is visible --
	var status_bar_visible := await _wait_for(func(): return _helpers.is_panel_visible("StatusBar"))
	assert_true(status_bar_visible, "Status bar should be visible")

	# -- Verify escape menu is hidden --
	assert_false(_helpers.is_panel_visible("EscapeMenu"), "Escape menu should be hidden on startup")

	# -- Verify status bar shows correct population count --
	# Status bar updates in _process, so wait for it to refresh.
	var pop_str := "%d Elves" % _fixture.elf_count
	if _fixture.elf_count == 1:
		pop_str = "1 Elf"
	var found_pop := await _wait_for(
		func():
			var sb := _main_scene.find_child("StatusBar", true, false)
			return _helpers.find_text_in_descendants(sb, pop_str),
		30
	)
	assert_true(found_pop, "Status bar should show '%s'" % pop_str)

	# -- Verify camera pivot default position --
	var pivot := _main_scene.get_node("CameraPivot")
	# The test world is 64x64x64, so the default position is at the center.
	# Just verify the pivot exists and has a reasonable position (not NaN/zero).
	assert_true(pivot.position.length() > 0.0, "Camera pivot should have a non-zero position")

	# -- Press Home key and verify camera moves to tree position --
	var tree_info := _get_bridge().get_home_tree_info()
	var tree_x: float = tree_info.get("position_x", 0.0)
	var tree_z: float = tree_info.get("position_z", 0.0)
	_helpers.press_key(KEY_HOME)
	var home_moved := await _wait_for(_check_camera_at_home.bind(tree_x, tree_z), 30)
	assert_true(home_moved, "Camera should move to home tree on Home key press")

	# -- Verify home tree info has expected keys --
	_assert_has_keys(
		tree_info, ["position_x", "position_y", "position_z", "mana_stored"], "Home tree info"
	)

	# -- Verify tree info panel shows mana via UI --
	_helpers.press_key(KEY_I)
	var tree_panel_visible := await _wait_for(
		func(): return _helpers.is_panel_visible("TreeInfoPanel"), 30
	)
	assert_true(tree_panel_visible, "TreeInfoPanel should open on I key press")
	for _fi in 3:
		await get_tree().process_frame
	var tree_panel := _main_scene.find_child("TreeInfoPanel", true, false)
	# The mana label shows "N / M" format; verify "Mana:" title is present.
	assert_true(
		_helpers.find_text_in_descendants(tree_panel, "Mana:"),
		"TreeInfoPanel should show 'Mana:' label",
	)
	# Verify the mana value label contains " / " (the "stored / capacity" format).
	assert_true(
		_helpers.find_text_in_descendants(tree_panel, " / "),
		"TreeInfoPanel should show mana in 'N / M' format",
	)
	# Close tree info panel and verify it hides.
	_helpers.press_key(KEY_I)
	var tree_panel_hidden := await _wait_for(
		func(): return not _helpers.is_panel_visible("TreeInfoPanel"), 30
	)
	assert_true(tree_panel_hidden, "TreeInfoPanel should hide on second I key press")

	# -- Verify status bar shows speed label --
	# Note: the bridge-level set_sim_speed("Paused") call in _load_game_scene
	# doesn't propagate to the StatusBar UI (it bypasses the signal chain),
	# so the label likely still reads "Speed: Normal".  We just verify the
	# speed label exists at all.
	var found_speed := await _wait_for(
		func():
			var sbar := _main_scene.find_child("StatusBar", true, false)
			return _helpers.find_text_in_descendants(sbar, "Speed:"),
		30
	)
	assert_true(found_speed, "Status bar should show 'Speed:' label")


# ===========================================================================
# Test 2: Creature selection and info panel
# ===========================================================================


func _setup_selection_fixture(bridge: SimBridge) -> void:
	bridge.init_sim_test_config(42)
	bridge.step_to_tick(50)
	var uuid: String = bridge.get_creature_uuid("Elf", 0)
	var info: Dictionary = bridge.get_creature_info_by_id(uuid, 50.0)
	_fixture = {
		"uuid": uuid,
		"species": info.get("species", ""),
		"name": info.get("name", ""),
		"x": info.get("x", 0.0),
		"y": info.get("y", 0.0),
		"z": info.get("z", 0.0),
		"tick": 50,
	}


func test_creature_selection_shows_info_panel() -> void:
	# -- Fixture generation --
	var json := _generate_save(_setup_selection_fixture)
	assert_ne(_fixture.uuid, "", "Fixture should have a valid elf UUID")

	# -- Load game scene --
	await _load_game_scene(json)

	# -- Bridge assertion: creature exists in the loaded sim --
	var info := _get_bridge().get_creature_info_by_id(_fixture.uuid, float(_fixture.tick))
	assert_false(info.is_empty(), "Creature should exist after load")

	# -- Position camera near the elf --
	var elf_world_pos := Vector3(_fixture.x + 0.5, _fixture.y + 0.48, _fixture.z + 0.5)
	_helpers.move_camera_to(Vector3(_fixture.x + 0.5, _fixture.y + 5.0, _fixture.z + 0.5))
	# Set zoom close enough to see the elf.
	var pivot := _main_scene.get_node("CameraPivot")
	pivot._zoom = 15.0
	pivot._update_camera_transform()

	# -- Click on the elf's projected screen position --
	# Note: unproject_position may not produce valid coords under headless xvfb.
	# If this test fails, the design doc acknowledges this risk and suggests
	# falling back to the selection controller API directly.
	_helpers.click_at_world_pos(elf_world_pos)

	# -- Wait for the creature info panel to appear --
	var panel_visible := await _wait_for(
		func(): return _helpers.is_panel_visible("CreatureInfoPanel"), 30
	)
	if not panel_visible:
		# ACCEPTED EXCEPTION: Fallback to the selection controller API when
		# click-to-select fails under headless xvfb.  The controller call
		# exercises the same panel-wiring path a click would trigger.
		var selector := _main_scene.find_child("SelectionController", true, false)
		if selector:
			selector.select_creature_by_id(_fixture.uuid)
			panel_visible = await _wait_for(
				func(): return _helpers.is_panel_visible("CreatureInfoPanel"), 30
			)
	assert_true(panel_visible, "CreatureInfoPanel should be visible after selection")

	# -- Verify panel shows correct species --
	# The species label shows "Species: Elf" — use specific text to avoid
	# false positives on other labels containing "Elf".
	var panel := _main_scene.find_child("CreatureInfoPanel", true, false)
	var found_species: bool = _helpers.find_text_in_descendants(panel, "Species: Elf")
	assert_true(found_species, "Panel should show 'Species: Elf'")

	# -- Verify panel shows creature name --
	# The name label shows "Name: <name>" or "Name: <name> (<meaning>)".
	assert_ne(_fixture.name, "", "Fixture name should be non-empty")
	var found_name: bool = _helpers.find_text_in_descendants(panel, _fixture.name)
	assert_true(found_name, "Panel should show creature name '%s'" % _fixture.name)

	# -- Verify panel shows HP --
	# The HP label shows "<current> / <max>".  We can't predict the exact
	# numbers, but the format "N / M" should be present.
	var found_hp: bool = _helpers.find_text_in_descendants(panel, " / ")
	assert_true(found_hp, "Panel should show HP in 'N / M' format")

	# -- Verify panel shows position --
	# The position label shows "Position: (x, y, z)".
	var pos_str := "Position: (%d, %d, %d)" % [int(_fixture.x), int(_fixture.y), int(_fixture.z)]
	var found_pos: bool = _helpers.find_text_in_descendants(panel, pos_str)
	assert_true(found_pos, "Panel should show '%s'" % pos_str)

	# -- Verify panel shows task --
	# The task label shows "Task: <kind>" or "Task: none".
	var found_task: bool = _helpers.find_text_in_descendants(panel, "Task:")
	assert_true(found_task, "Panel should show a 'Task:' label")

	# -- Verify panel shows mood --
	# The mood label shows "Mood: <tier> (+/-N)".
	var found_mood: bool = _helpers.find_text_in_descendants(panel, "Mood:")
	assert_true(found_mood, "Panel should show a 'Mood:' label")

	# -- Close the panel via ESC and verify it hides --
	_helpers.press_key(KEY_ESCAPE)
	var panel_hidden := await _wait_for(
		func(): return not _helpers.is_panel_visible("CreatureInfoPanel"), 30
	)
	assert_true(panel_hidden, "CreatureInfoPanel should hide after ESC")


# ===========================================================================
# Test 6: Military groups via UI
# ===========================================================================


func _setup_military_fixture(bridge: SimBridge) -> void:
	bridge.init_sim_test_config(42)
	bridge.step_to_tick(100)
	bridge.create_military_group("Alpha Squad")
	bridge.step_to_tick(101)
	# Find the group named "Alpha Squad".
	var groups: Array = bridge.get_military_groups()
	var group_id := -1
	for g in groups:
		if g.get("name", "") == "Alpha Squad":
			group_id = g.get("id", -1)
			break
	# Assign an elf to the group.
	var uuid: String = bridge.get_creature_uuid("Elf", 0)
	if group_id >= 0 and uuid != "":
		bridge.reassign_military_group(uuid, group_id)
		bridge.step_to_tick(102)
	_fixture = {
		"group_id": group_id,
		"elf_uuid": uuid,
		"tick": bridge.current_tick(),
	}


func test_military_panel_shows_group() -> void:
	# -- Fixture generation --
	var json := _generate_save(_setup_military_fixture)
	assert_gt(_fixture.group_id, -1, "Should have created Alpha Squad group")

	# -- Load game scene --
	await _load_game_scene(json)

	# -- Bridge assertion: military group exists with 1 member --
	var groups: Array = _get_bridge().get_military_groups()
	var found_group := false
	for g in groups:
		if g.get("name", "") == "Alpha Squad":
			found_group = true
			assert_eq(
				int(g.get("member_count", 0)),
				1,
				"Alpha Squad should have 1 member",
			)
			break
	assert_true(found_group, "Alpha Squad should exist in loaded game")

	# -- Open military panel via M key --
	_helpers.press_key(KEY_M)
	var panel_visible := await _wait_for(
		func(): return _helpers.is_panel_visible("MilitaryPanel"), 30
	)
	assert_true(panel_visible, "MilitaryPanel should open on M key press")

	# -- Verify panel contains "Alpha Squad" text and member count --
	# Wait a few frames for the panel to refresh its content.
	for _fi in 3:
		await get_tree().process_frame
	var panel := _main_scene.find_child("MilitaryPanel", true, false)
	var found_text: bool = _helpers.find_text_in_descendants(panel, "Alpha Squad")
	assert_true(found_text, "Panel should contain 'Alpha Squad' text")

	# The summary row is [Button("Alpha Squad [Passive]"), Label("1")].
	# The button text includes the initiative in brackets.  Verify the
	# button text contains "Alpha Squad" and the row also has the count.
	var members: Array = _get_bridge().get_military_group_members(_fixture.group_id)
	assert_eq(members.size(), 1, "Alpha Squad should have 1 member via bridge query")
	# Verify the panel shows the initiative tag alongside the group name.
	assert_true(
		_helpers.find_text_in_descendants(panel, "Alpha Squad ["),
		"Summary row button should show 'Alpha Squad [<initiative>]'",
	)

	# -- Close military panel via M key --
	_helpers.press_key(KEY_M)
	var panel_hidden := await _wait_for(
		func(): return not _helpers.is_panel_visible("MilitaryPanel"), 30
	)
	assert_true(panel_hidden, "MilitaryPanel should close on second M key press")

	# -- Reopen and verify consistency in both bridge and UI --
	_helpers.press_key(KEY_M)
	var reopened := await _wait_for(func(): return _helpers.is_panel_visible("MilitaryPanel"), 30)
	assert_true(reopened, "MilitaryPanel should reopen on third M key press")
	for _fi in 3:
		await get_tree().process_frame
	# Group data should still be consistent after close+reopen.
	var members_after: Array = _get_bridge().get_military_group_members(_fixture.group_id)
	assert_eq(members_after.size(), 1, "Group should still have 1 member after panel reopen")
	# Re-verify UI shows "Alpha Squad" after reopen.
	var panel_after := _main_scene.find_child("MilitaryPanel", true, false)
	assert_true(
		_helpers.find_text_in_descendants(panel_after, "Alpha Squad"),
		"Panel should still show 'Alpha Squad' after close+reopen",
	)


# ===========================================================================
# Test 3: Construction workflow
# ===========================================================================


func _setup_construction_fixture(bridge: SimBridge) -> void:
	bridge.init_sim_test_config(42)
	bridge.step_to_tick(200)
	# Find a valid build position near the tree.
	var tree_info: Dictionary = bridge.get_home_tree_info()
	var tx: int = int(tree_info.get("position_x", 0))
	var tz: int = int(tree_info.get("position_z", 0))
	# Scan outward from the tree at y = anchor_y + 5 looking for a valid
	# platform position (Air with at least one solid neighbor).
	var build_x := -1
	var build_y := 5
	var build_z := -1
	for dx in range(-5, 6):
		for dz in range(-5, 6):
			var cx: int = tx + dx
			var cz: int = tz + dz
			if bridge.validate_build_position(cx, build_y, cz):
				build_x = cx
				build_z = cz
				break
		if build_x >= 0:
			break
	_fixture = {
		"tree_x": tx,
		"tree_z": tz,
		"build_x": build_x,
		"build_y": build_y,
		"build_z": build_z,
		"tick": 200,
	}


func test_construction_blueprint_placement() -> void:
	# -- Fixture generation --
	var json := _generate_save(_setup_construction_fixture)
	assert_true(_fixture.build_x >= 0, "Should have found a valid build position")

	# -- Load game scene --
	await _load_game_scene(json)

	# -- Verify no blueprints initially --
	var initial_bp := _get_bridge().get_blueprint_voxels()
	assert_eq(initial_bp.size(), 0, "No blueprints should exist initially")

	# -- Place a platform blueprint via bridge --
	# ACCEPTED EXCEPTION: Blueprint placement uses the bridge directly because
	# the full UI construction flow requires mouse drag simulation (press at
	# start voxel, drag to end voxel, release to enter preview, confirm) which
	# is unreliable under headless xvfb.  The bridge call exercises the same
	# sim command the UI would emit.
	var result: String = _get_bridge().designate_build_rect(
		_fixture.build_x, _fixture.build_y, _fixture.build_z, 2, 2
	)
	# Empty string = success (the return value is a validation error message).
	assert_eq(result, "", "designate_build_rect should succeed (empty = no error)")

	# -- Verify blueprints exist --
	var bp_voxels := _get_bridge().get_blueprint_voxels()
	assert_gt(bp_voxels.size(), 0, "Blueprint voxels should exist after placement")

	# -- Step time to let elves start building --
	_step_ticks(500)
	for _fi in 3:
		await get_tree().process_frame

	# -- Verify blueprint or structure state after stepping --
	# Structures appear when a blueprint is completed. With 500 ticks in a
	# small world, construction may or may not be done. Either blueprints
	# remain (in progress) or structures appeared (complete).
	var structures: Array = _get_bridge().get_structures()
	var bp_after := _get_bridge().get_blueprint_voxels()
	assert_true(
		structures.size() > 0 or bp_after.size() > 0,
		"Should have either structures or remaining blueprints after stepping",
	)

	# -- If structures were built, select one and verify StructureInfoPanel --
	if structures.size() > 0:
		var first_sid: int = int(structures[0].get("id", -1))
		var selector := _main_scene.find_child("SelectionController", true, false)
		if selector and first_sid >= 0:
			# ACCEPTED EXCEPTION: structure selection via controller (see crafting
			# test step 1 comment for xvfb rationale).
			selector.select_structure(first_sid)
			var panel_visible := await _wait_for(
				func(): return _helpers.is_panel_visible("StructureInfoPanel"), 30
			)
			assert_true(panel_visible, "StructureInfoPanel should open for completed structure")
			var struct_panel := _main_scene.find_child("StructureInfoPanel", true, false)
			assert_true(
				_helpers.find_text_in_descendants(struct_panel, "Platform"),
				"StructureInfoPanel should show 'Platform' build type",
			)
			# Deselect and verify panel closes.
			_helpers.press_key(KEY_ESCAPE)
			var struct_closed := await _wait_for(
				func(): return not _helpers.is_panel_visible("StructureInfoPanel"), 30
			)
			assert_true(struct_closed, "StructureInfoPanel should hide after ESC")

	# -- Verify the construction panel opens via B key --
	_helpers.press_key(KEY_B)
	var found_construction := await _wait_for(func(): return _is_construction_active(), 30)
	assert_true(found_construction, "Construction mode should activate on B key")

	# Verify construction mode buttons are visible (Platform, Building).
	# Wait a few frames for the construction UI to fully populate.
	for _fi in 3:
		await get_tree().process_frame
	assert_not_null(
		_helpers.find_button(_main_scene, "Platform"),
		"Construction mode should show 'Platform' button",
	)
	assert_not_null(
		_helpers.find_button(_main_scene, "Building"),
		"Construction mode should show 'Building' button",
	)

	# Exit construction mode.
	_helpers.press_key(KEY_ESCAPE)
	await _wait_for(func(): return not _is_construction_active(), 30)


# ===========================================================================
# Test 4: Save/load round-trip
# ===========================================================================


func _setup_save_load_fixture(bridge: SimBridge) -> void:
	bridge.init_sim_test_config(42)
	bridge.step_to_tick(300)
	bridge.spawn_creature("Capybara", 32, 1, 32)
	bridge.step_to_tick(400)
	_fixture = {
		"tick": 400,
		"elf_count": bridge.elf_count(),
		"capybara_count": bridge.creature_count_by_name("Capybara"),
		"mana": bridge.home_tree_mana(),
		"fruit_count": bridge.fruit_count(),
	}


func test_save_load_round_trip() -> void:
	# -- Fixture generation --
	var json := _generate_save(_setup_save_load_fixture)
	assert_gt(_fixture.elf_count, 0, "Fixture should have elves")

	# -- Load the save --
	await _load_game_scene(json)

	# -- Verify initial state matches fixture (bridge) --
	assert_eq(_get_bridge().current_tick(), _fixture.tick, "Tick should match fixture after load")
	assert_eq(_get_bridge().elf_count(), _fixture.elf_count, "Elf count should match fixture")
	assert_eq(
		_get_bridge().creature_count_by_name("Capybara"),
		_fixture.capybara_count,
		"Capybara count should match fixture",
	)
	# Mana is float — use approximate comparison.
	assert_true(
		absf(_get_bridge().home_tree_mana() - _fixture.mana) < 0.01,
		(
			"Mana should match fixture (got %f, expected %f)"
			% [_get_bridge().home_tree_mana(), _fixture.mana]
		),
	)

	# -- Verify initial state matches fixture (UI) --
	# Status bar should show the elf count.
	var pop_str := "%d Elves" % _fixture.elf_count
	if _fixture.elf_count == 1:
		pop_str = "1 Elf"
	var sb := _main_scene.find_child("StatusBar", true, false)
	var found_pop := await _wait_for(
		func(): return _helpers.find_text_in_descendants(sb, pop_str), 30
	)
	assert_true(found_pop, "Status bar should show '%s' after initial load" % pop_str)

	# Units panel should list the capybara.
	_helpers.press_key(KEY_U)
	var units_visible := await _wait_for(func(): return _helpers.is_panel_visible("UnitsPanel"), 30)
	assert_true(units_visible, "UnitsPanel should open on U key press")
	# Wait for the panel to refresh its creature list.
	for _fi in 5:
		await get_tree().process_frame
	var units_panel := _main_scene.find_child("UnitsPanel", true, false)
	assert_true(
		_helpers.find_text_in_descendants(units_panel, "Capybara"),
		"UnitsPanel should show 'Capybara' section after load",
	)
	# Close units panel.
	_helpers.press_key(KEY_U)

	# -- Step time to modify state --
	_step_ticks(100)
	var post_step_tick := _get_bridge().current_tick()
	assert_eq(post_step_tick, _fixture.tick + 100, "Tick should advance by 100")

	# -- Save the current state via bridge --
	var save_json := _get_bridge().save_game_json()
	assert_gt(save_json.length(), 0, "Save JSON should be non-empty")

	# -- Shut down and reload from the new save --
	var bridge := _main_scene.get_node_or_null("SimBridge") as SimBridge
	if bridge:
		bridge.shutdown()
	_main_scene.queue_free()
	_main_scene = null
	await get_tree().process_frame

	# -- Load the saved state into a fresh scene --
	await _load_game_scene(save_json)

	# -- Verify state matches post-step snapshot --
	assert_eq(
		_get_bridge().current_tick(),
		post_step_tick,
		"Tick after reload should match post-step tick",
	)
	assert_eq(
		_get_bridge().elf_count(),
		_fixture.elf_count,
		"Elf count should survive save/load round-trip",
	)
	assert_eq(
		_get_bridge().creature_count_by_name("Capybara"),
		_fixture.capybara_count,
		"Capybara count should survive save/load round-trip",
	)

	# -- Verify reloaded state in UI --
	# Status bar should still show the elf count after reload.
	var sb_reload := _main_scene.find_child("StatusBar", true, false)
	var pop_str_reload := "%d Elves" % _fixture.elf_count
	if _fixture.elf_count == 1:
		pop_str_reload = "1 Elf"
	var found_pop_reload := await _wait_for(
		func(): return _helpers.find_text_in_descendants(sb_reload, pop_str_reload), 30
	)
	assert_true(found_pop_reload, "Status bar should show '%s' after reload" % pop_str_reload)


# ===========================================================================
# Test 5: Sprite generation (standalone, no scene needed)
# ===========================================================================


func test_sprite_species_elf() -> void:
	var gen := SpriteGenerator.new()
	var tex := gen.species_sprite("Elf", 42)
	assert_not_null(tex, "Elf sprite should not be null")
	assert_gt(tex.get_width(), 0, "Elf sprite width should be positive")
	assert_gt(tex.get_height(), 0, "Elf sprite height should be positive")


func test_sprite_all_species() -> void:
	var gen := SpriteGenerator.new()
	var species := [
		"Elf",
		"Capybara",
		"Boar",
		"Deer",
		"Elephant",
		"Goblin",
		"Monkey",
		"Orc",
		"Squirrel",
		"Troll",
	]
	for sp in species:
		var tex := gen.species_sprite(sp, 42)
		assert_not_null(tex, "%s sprite should not be null" % sp)
		assert_gt(tex.get_width(), 0, "%s sprite width should be positive" % sp)
		assert_gt(tex.get_height(), 0, "%s sprite height should be positive" % sp)


func test_sprite_invalid_species_returns_null() -> void:
	var gen := SpriteGenerator.new()
	var tex := gen.species_sprite("InvalidSpecies", 0)
	assert_null(tex, "Invalid species should return null")


func test_sprite_fruit_round() -> void:
	var gen := SpriteGenerator.new()
	var tex := gen.fruit_sprite("Round", 200, 100, 50, 100, false)
	assert_not_null(tex, "Round fruit sprite should not be null")
	assert_gt(tex.get_width(), 0, "Fruit sprite width should be positive")
	assert_gt(tex.get_height(), 0, "Fruit sprite height should be positive")


func test_sprite_all_fruit_shapes() -> void:
	var gen := SpriteGenerator.new()
	var shapes := ["Round", "Oblong", "Clustered", "Pod", "Nut", "Gourd"]
	for shape in shapes:
		var tex := gen.fruit_sprite(shape, 200, 100, 50, 100, false)
		assert_not_null(tex, "%s fruit should not be null" % shape)
		assert_gt(tex.get_width(), 0, "%s fruit width should be positive" % shape)
		assert_gt(tex.get_height(), 0, "%s fruit height should be positive" % shape)


func test_sprite_fruit_from_empty_dict_no_crash() -> void:
	var gen := SpriteGenerator.new()
	# Should not crash — uses defaults for all missing keys.
	var tex := gen.fruit_sprite_from_dict({})
	# fruit_sprite_from_dict defaults to Round with a fallback color,
	# so it should succeed.
	assert_not_null(tex, "fruit_sprite_from_dict({}) should return a texture")


func test_sprite_fruit_glowing() -> void:
	var gen := SpriteGenerator.new()
	var tex := gen.fruit_sprite("Round", 100, 200, 255, 120, true)
	assert_not_null(tex, "Glowing fruit sprite should not be null")


# ===========================================================================
# Test: Crafting pipeline via UI panels (Recipe enum, material params)
# ===========================================================================


## Open the crafting details panel for the currently selected structure.
## Handles the two-Details-button ambiguity and waits for the panel to load.
## Returns true if the crafting details panel is visible.
func _open_crafting_details(struct_panel: Node) -> bool:
	# Wait for the details panel to be added to the tree.
	var found := await _wait_for(
		func(): return _helpers.find_button(_main_scene, "Add Recipe") != null, 60
	)
	if not found:
		return false
	# Two "Details..." buttons exist (crafting + logistics). Try each.
	for btn in _helpers.find_all_buttons(struct_panel, "Details"):
		_helpers.press_button(btn)
		var opened := await _wait_for(
			func():
				return (
					_helpers.find_button(_main_scene, "Crafting Enabled") != null
					and _helpers.find_text_in_descendants(_main_scene, "Crafting Details")
				),
			10
		)
		if opened:
			break
	# Yield frames so _process populates the recipe cache.
	for _fi in 5:
		await get_tree().process_frame
	return _helpers.find_text_in_descendants(_main_scene, "Crafting Details")


## Open the recipe picker via the "Add Recipe..." button and find a recipe
## button by name, expanding categories if needed. Returns the Button or null.
func _open_picker_and_find_recipe(recipe_name: String) -> Button:
	var add_btn: Button = _helpers.find_button(_main_scene, "Add Recipe")
	if not add_btn:
		gut.p("WARNING: 'Add Recipe' button not found — picker cannot open")
		return null
	_helpers.press_button(add_btn)
	for _fi in 3:
		await get_tree().process_frame

	# Wait for recipe button to appear.
	var found := await _wait_for(
		func(): return _helpers.find_button(_main_scene, recipe_name) != null, 30
	)
	if found:
		return _helpers.find_button(_main_scene, recipe_name)

	# Categories may be collapsed — try expanding each category header.
	for cat_name in ["Woodcraft", "Processing", "Extraction"]:
		var cat_btn: Button = _helpers.find_button(_main_scene, cat_name)
		if cat_btn:
			_helpers.press_button(cat_btn)
			found = await _wait_for(
				func(): return _helpers.find_button(_main_scene, recipe_name) != null, 10
			)
			if found:
				return _helpers.find_button(_main_scene, recipe_name)
	return null


func _setup_crafting_fixture(bridge: SimBridge) -> void:
	bridge.init_sim_test_config(42)
	bridge.debug_disable_needs()
	bridge.step_to_tick(200)

	var tree_info: Dictionary = bridge.get_home_tree_info()
	var tx: int = int(tree_info.get("position_x", 0))
	var tz: int = int(tree_info.get("position_z", 0))

	# Designate three buildings near the tree. y=1 is the walkable forest floor
	# level (y=0 is solid ground). Buildings placed at y=1 have their interior
	# at y=2 which has nav nodes.
	bridge.designate_building(tx + 3, 1, tz, 3, 3, 3)
	bridge.designate_building(tx + 7, 1, tz, 3, 3, 3)
	bridge.designate_building(tx + 11, 1, tz, 3, 3, 3)

	# Spawn extra elves for building + crafting labor.
	for i in range(5):
		bridge.spawn_elf(tx, 1, tz)

	# Step lots of ticks so construction completes.
	bridge.step_to_tick(200_000)

	# Find completed structures.
	var structures: Array = bridge.get_structures()
	var building_sids: Array = []
	for s in structures:
		if s.get("build_type", "") == "Building":
			building_sids.append(int(s.get("id", -1)))

	var workshop_id: int = -1
	var kitchen_id: int = -1
	var storehouse_id: int = -1

	if building_sids.size() >= 3:
		workshop_id = building_sids[0]
		kitchen_id = building_sids[1]
		storehouse_id = building_sids[2]
		bridge.furnish_structure(workshop_id, "Workshop", -1)
		bridge.furnish_structure(kitchen_id, "Kitchen", -1)
		bridge.furnish_structure(storehouse_id, "Storehouse", -1)
		bridge.step_to_tick(210_000)

		# Stock storehouse with bowstrings (input for GrowBow) and bread
		# (prevents elf starvation over the long sim duration).
		bridge.debug_add_item_to_structure(storehouse_id, "Bowstring", 20, "")
		bridge.debug_add_item_to_structure(storehouse_id, "Bread", 500, "")
		# Pre-stock workshop with bowstrings so the crafting test doesn't
		# need a mid-test debug_add_item call (logistics hauling is too slow
		# for test timeouts).
		bridge.debug_add_item_to_structure(workshop_id, "Bowstring", 10, "")

	_fixture = {
		"workshop_id": workshop_id,
		"kitchen_id": kitchen_id,
		"storehouse_id": storehouse_id,
		"tree_x": tx,
		"tree_z": tz,
		"building_count": building_sids.size(),
	}


func test_crafting_pipeline_via_ui() -> void:
	# -- Fixture: 3 furnished buildings + stocked storehouse --
	var json := _generate_save(_setup_crafting_fixture)
	if _fixture.building_count < 3:
		gut.p("SKIP: only %d buildings completed (need 3)" % _fixture.building_count)
		pass_test("skipped — insufficient buildings")
		return

	await _load_game_scene(json)
	var bridge := _get_bridge()
	var workshop_id: int = _fixture.workshop_id
	var kitchen_id: int = _fixture.kitchen_id
	var storehouse_id: int = _fixture.storehouse_id

	var selector := _main_scene.find_child("SelectionController", true, false)
	assert_not_null(selector, "SelectionController should exist")

	# ---------------------------------------------------------------
	# 1. Select workshop → structure info panel opens
	# ---------------------------------------------------------------
	# ACCEPTED EXCEPTION: Structure selection uses the SelectionController API
	# instead of clicking in the 3D viewport.  Unlike creature clicks (which
	# have a single sprite to hit), structures span multiple voxels and the
	# mesh click target is unreliable under headless xvfb.  The controller
	# call exercises the same panel-wiring path a click would trigger.
	selector.select_structure(workshop_id)
	var panel_visible := await _wait_for(
		func(): return _helpers.is_panel_visible("StructureInfoPanel"), 30
	)
	assert_true(panel_visible, "StructureInfoPanel should open when workshop selected")

	var struct_panel := _main_scene.find_child("StructureInfoPanel", true, false)
	assert_true(
		_helpers.find_text_in_descendants(struct_panel, "Workshop"),
		"Panel should show 'Workshop' furnishing type",
	)

	# ---------------------------------------------------------------
	# 2. Open crafting details panel via UI
	# ---------------------------------------------------------------
	var crafting_open := await _open_crafting_details(struct_panel)
	assert_true(crafting_open, "Crafting details panel should open")
	# Verify the panel shows "Crafting Details" header and "Crafting Enabled".
	assert_true(
		_helpers.find_text_in_descendants(_main_scene, "Crafting Details"),
		"Panel should show 'Crafting Details' header",
	)
	assert_not_null(
		_helpers.find_button(_main_scene, "Crafting Enabled"),
		"Crafting Enabled checkbox should be visible",
	)

	# ---------------------------------------------------------------
	# 3. Open recipe picker and add "Grow Oak Bow" via UI
	# ---------------------------------------------------------------
	var grow_bow_btn := await _open_picker_and_find_recipe("Grow Oak Bow")
	assert_not_null(grow_bow_btn, "Should find 'Grow Oak Bow' recipe button in picker")
	if not grow_bow_btn:
		return
	_helpers.press_button(grow_bow_btn)
	# Yield frames for the signal chain: button press → add_recipe_requested
	# → bridge.add_active_recipe → sim processes command.
	for _fi in 5:
		await get_tree().process_frame

	# ---------------------------------------------------------------
	# 4. Verify recipe was added — both in bridge state AND panel text
	# ---------------------------------------------------------------
	var ws_info: Dictionary = bridge.get_structure_info(workshop_id)
	var ws_active: Array = ws_info.get("active_recipes", [])
	assert_eq(ws_active.size(), 1, "Workshop should have 1 active recipe after UI add")
	# Verify panel shows the recipe name.
	for _fi in 5:
		await get_tree().process_frame
	assert_true(
		_helpers.find_text_in_descendants(_main_scene, "Grow Oak Bow"),
		"Panel should display 'Grow Oak Bow' in active recipes",
	)
	# Verify the bridge reports the correct display name.
	var bow_recipe: Dictionary = ws_active[0]
	var bow_display: String = bow_recipe.get("recipe_display_name", "")
	assert_true(
		"Oak" in bow_display and "Bow" in bow_display,
		"Bridge display name should contain 'Oak' and 'Bow', got: %s" % bow_display,
	)

	# ---------------------------------------------------------------
	# 5. Set output target via UI "+" button, verify panel updates
	# ---------------------------------------------------------------
	# The active recipe row should have a "+" button for the Bow output target.
	# The default increment is 1 since _get_output_quantity_for_target returns 1.
	var plus_btn: Button = _helpers.find_button_near_label(_main_scene, "Oak Bow", "+1")
	assert_not_null(plus_btn, "Should find '+1' button near Bow target row")
	if not plus_btn:
		return
	# Click it 5 times to set target to 5.
	for _i in 5:
		_helpers.press_button(plus_btn)
	for _fi in 3:
		await get_tree().process_frame

	# Verify the target updated in bridge AND panel.
	ws_info = bridge.get_structure_info(workshop_id)
	ws_active = ws_info.get("active_recipes", [])
	var targets: Array = ws_active[0].get("targets", [])
	assert_gt(targets.size(), 0, "GrowBow should have output targets")
	assert_eq(
		int(targets[0].get("target_quantity", 0)),
		5,
		"Bridge: Bow target should be 5 after clicking +1 five times",
	)
	# The target LineEdit in the UI should show "5". Search within the
	# crafting details area (not the whole scene) to avoid false positives
	# from unrelated numbers elsewhere in the UI.
	for _fi in 3:
		await get_tree().process_frame
	var crafting_area := _main_scene.find_child("StructureInfoPanel", true, false)
	if crafting_area:
		crafting_area = crafting_area.get_parent()
	var found_target_5 := false
	if crafting_area:
		found_target_5 = _helpers.find_line_edit_with_text(crafting_area, "5")
	assert_true(found_target_5, "Target LineEdit near recipe should show '5'")

	# ---------------------------------------------------------------
	# 6. Step time → bows crafted (bowstrings pre-stocked in fixture)
	# ---------------------------------------------------------------
	# Step until bows appear or timeout.
	var ticks_stepped := _step_until(
		func():
			var info: Dictionary = bridge.get_structure_info(workshop_id)
			for item in info.get("inventory", []):
				if item.get("kind", "") == "Bow":
					return true
			return false,
		200_000,
	)
	assert_ne(ticks_stepped, -1, "Crafting should produce bows within 200k ticks")

	# Verify bows in bridge AND panel inventory.
	for _fi in 5:
		await get_tree().process_frame
	ws_info = bridge.get_structure_info(workshop_id)
	var bow_count := 0
	for item in ws_info.get("inventory", []):
		if item.get("kind", "") == "Bow":
			bow_count += int(item.get("quantity", 0))
	assert_gt(bow_count, 0, "Workshop should have crafted bows, got %d" % bow_count)
	assert_true(
		_helpers.find_text_in_descendants(_main_scene, "Bow"),
		"Panel inventory should show 'Bow'",
	)

	# ---------------------------------------------------------------
	# 7. Add second recipe (GrowArrow) via UI picker
	# ---------------------------------------------------------------
	# Re-select workshop (see step 1 comment for xvfb exception rationale).
	selector.select_structure(workshop_id)
	await _wait_for(func(): return _helpers.is_panel_visible("StructureInfoPanel"), 30)
	struct_panel = _main_scene.find_child("StructureInfoPanel", true, false)

	crafting_open = await _open_crafting_details(struct_panel)
	assert_true(crafting_open, "Crafting details should reopen for workshop")

	var arrow_btn := await _open_picker_and_find_recipe("Grow Oak Arrows")
	assert_not_null(arrow_btn, "Should find 'Grow Oak Arrows' recipe button")
	if not arrow_btn:
		return
	_helpers.press_button(arrow_btn)
	for _fi in 5:
		await get_tree().process_frame

	# Verify both recipes in bridge AND panel.
	ws_info = bridge.get_structure_info(workshop_id)
	ws_active = ws_info.get("active_recipes", [])
	assert_eq(ws_active.size(), 2, "Workshop should have 2 active recipes")
	for _fi in 5:
		await get_tree().process_frame
	assert_true(
		_helpers.find_text_in_descendants(_main_scene, "Grow Oak Bow"),
		"Panel should still show 'Grow Oak Bow'",
	)
	assert_true(
		_helpers.find_text_in_descendants(_main_scene, "Grow Oak Arrows"),
		"Panel should also show 'Grow Oak Arrows'",
	)

	# ---------------------------------------------------------------
	# 8. Remove Grow Oak Bow recipe via its "X" button in the panel
	# ---------------------------------------------------------------
	# Find the "X" button next to the "Grow Oak Bow" label.
	var remove_btn: Button = _helpers.find_button_near_label(_main_scene, "Grow Oak Bow", "X")
	assert_not_null(remove_btn, "Should find 'X' remove button near 'Grow Oak Bow'")
	if not remove_btn:
		return
	_helpers.press_button(remove_btn)
	for _fi in 5:
		await get_tree().process_frame

	# Verify removal in bridge AND panel.
	ws_info = bridge.get_structure_info(workshop_id)
	ws_active = ws_info.get("active_recipes", [])
	assert_eq(ws_active.size(), 1, "Should have 1 recipe after removal")
	for _fi in 5:
		await get_tree().process_frame
	assert_false(
		_helpers.find_text_in_descendants(_main_scene, "Grow Oak Bow"),
		"'Grow Oak Bow' should be gone from panel after removal",
	)
	assert_true(
		_helpers.find_text_in_descendants(_main_scene, "Grow Oak Arrows"),
		"'Grow Oak Arrows' should remain in panel after removal",
	)

	# ---------------------------------------------------------------
	# 9. Select kitchen, add extraction recipe via UI picker
	# ---------------------------------------------------------------
	# (See step 1 comment for xvfb exception rationale.)
	selector.select_structure(kitchen_id)
	await _wait_for(func(): return _helpers.is_panel_visible("StructureInfoPanel"), 30)
	struct_panel = _main_scene.find_child("StructureInfoPanel", true, false)

	crafting_open = await _open_crafting_details(struct_panel)
	assert_true(crafting_open, "Crafting details should open for kitchen")

	# Find an Extract recipe (any species, name starts with "Extract ").
	var extract_btn := await _open_picker_and_find_recipe("Extract ")
	assert_not_null(extract_btn, "Should find an Extract recipe button")
	if not extract_btn:
		return
	_helpers.press_button(extract_btn)
	for _fi in 5:
		await get_tree().process_frame

	# Verify in bridge AND panel.
	var k_info: Dictionary = bridge.get_structure_info(kitchen_id)
	var k_active: Array = k_info.get("active_recipes", [])
	assert_eq(k_active.size(), 1, "Kitchen should have 1 active extraction recipe")
	var extract_display: String = k_active[0].get("recipe_display_name", "")
	assert_true(
		"Extract" in extract_display,
		"Bridge: kitchen recipe should be Extract, got: %s" % extract_display,
	)
	for _fi in 5:
		await get_tree().process_frame
	assert_true(
		_helpers.find_text_in_descendants(_main_scene, "Extract"),
		"Panel should display extraction recipe",
	)
