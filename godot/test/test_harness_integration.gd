## Integration tests for the AI test harness and bridge integration.
##
## These tests exercise the full vertical slice: Rust sim -> GDExtension bridge
## -> GDScript UI.  They instantiate the real main.tscn scene as a child node
## and interact with it through the same paths a player would use.
##
## Performance note: Each test that loads main.tscn takes ~3s (sim init, tree
## generation, mesh build, full UI setup).  As of March 2026 the 12 tests here
## add ~15-17s to the GUT suite, well within the 60s GUT_TIMEOUT.  When adding
## new integration tests, prefer adding assertions to an existing scene-loading
## test rather than creating a new one — the per-test overhead is almost entirely
## scene loading, so "load once, verify many things" scales much better.  If the
## suite grows past ~40s, consider splitting integration tests into a separate
## build.sh target or sharing a single loaded scene across test methods via
## before_all (see the design doc's "Timeout consideration" section).
##
## Helper functions (_generate_save, _load_game_scene, _wait_for, _step_ticks,
## _step_until, _click_at_world_pos, etc.) form the reusable test harness
## toolkit described in docs/drafts/F-bridge-integ-tests-and-ai-test-harness.md.
##
## See also: main.gd for the scene controller, sim_bridge.rs for the Rust
## bridge, sprite_bridge.rs for SpriteGenerator.
extends GutTest

# ---------------------------------------------------------------------------
# State
# ---------------------------------------------------------------------------

## The instantiated main scene (child of this test node).
var _main_scene: Node

## Recorded fixture data (set during _generate_save, read during test body).
var _fixture: Dictionary = {}

# ---------------------------------------------------------------------------
# Setup / teardown
# ---------------------------------------------------------------------------


func before_each() -> void:
	# Keep processing even when the Godot tree is paused (needed for tests
	# that open the pause menu, which sets get_tree().paused = true).
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
		# Yield a frame so Godot processes the deferred free and cleans up
		# child nodes (CanvasLayers, panels, renderers) before GUT moves on.
		await get_tree().process_frame
	# Reset the load path in case _load_game_scene failed before main.gd
	# cleared it.  Prevents stale paths from leaking to the next test.
	GameSession.load_save_path = ""
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
	# Instantiate the real game scene as a child of this test node.
	var packed := load("res://scenes/main.tscn") as PackedScene
	_main_scene = packed.instantiate()
	add_child(_main_scene)
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
# Helper: camera and clicking
# ---------------------------------------------------------------------------


## Position the camera to look at a world position.
func _move_camera_to(world_pos: Vector3) -> void:
	var pivot := _main_scene.get_node("CameraPivot")
	pivot.position = world_pos


## Send a key press+release event through the full input pipeline.
func _press_key(keycode: int) -> void:
	var press := InputEventKey.new()
	press.keycode = keycode
	press.pressed = true
	Input.parse_input_event(press)
	var release := InputEventKey.new()
	release.keycode = keycode
	release.pressed = false
	Input.parse_input_event(release)


## Click at a world position by projecting to screen coords.
## Sends motion -> press -> release so controls register hover + click.
func _click_at_world_pos(world_pos: Vector3) -> void:
	var camera: Camera3D = _main_scene.get_node("CameraPivot/Camera3D")
	var screen_pos := camera.unproject_position(world_pos)
	# Move cursor to position first.
	var motion := InputEventMouseMotion.new()
	motion.position = screen_pos
	Input.parse_input_event(motion)
	# Press.
	var click := InputEventMouseButton.new()
	click.button_index = MOUSE_BUTTON_LEFT
	click.pressed = true
	click.position = screen_pos
	Input.parse_input_event(click)
	# Release.
	var release := InputEventMouseButton.new()
	release.button_index = MOUSE_BUTTON_LEFT
	release.pressed = false
	release.position = screen_pos
	Input.parse_input_event(release)


# ---------------------------------------------------------------------------
# Helper: UI reading
# ---------------------------------------------------------------------------


## Read text from a Label or RichTextLabel by name (recursive search).
func _read_panel_text(node_name: String) -> String:
	var node := _main_scene.find_child(node_name, true, false)
	if node is RichTextLabel:
		return node.get_parsed_text()
	if node is Label:
		return node.text
	return ""


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


## Check if a named Control node is visible (recursive search).
func _is_panel_visible(node_name: String) -> bool:
	var node := _main_scene.find_child(node_name, true, false)
	return node != null and node.visible


## Recursively search a node's descendants for a Label or Button whose text
## contains the given substring.  Returns true if found.
func _find_text_in_descendants(root: Node, substring: String) -> bool:
	if root == null:
		return false
	if root is Label and substring in root.text:
		return true
	if root is Button and substring in root.text:
		return true
	if root is RichTextLabel and substring in root.get_parsed_text():
		return true
	for child in root.get_children():
		if _find_text_in_descendants(child, substring):
			return true
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
	var status_bar_visible := await _wait_for(func(): return _is_panel_visible("StatusBar"))
	assert_true(status_bar_visible, "Status bar should be visible")

	# -- Verify pause menu is hidden --
	assert_false(_is_panel_visible("PauseMenu"), "Pause menu should be hidden on startup")

	# -- Verify status bar shows correct population count --
	# Status bar updates in _process, so wait for it to refresh.
	var pop_str := "%d Elves" % _fixture.elf_count
	if _fixture.elf_count == 1:
		pop_str = "1 Elf"
	var found_pop := await _wait_for(
		func():
			var sb := _main_scene.find_child("StatusBar", true, false)
			return _find_text_in_descendants(sb, pop_str),
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
	_press_key(KEY_HOME)
	var home_moved := await _wait_for(_check_camera_at_home.bind(tree_x, tree_z), 30)
	assert_true(home_moved, "Camera should move to home tree on Home key press")

	# -- Verify home tree info has expected keys --
	_assert_has_keys(
		tree_info, ["position_x", "position_y", "position_z", "mana_stored"], "Home tree info"
	)


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
	_move_camera_to(Vector3(_fixture.x + 0.5, _fixture.y + 5.0, _fixture.z + 0.5))
	# Set zoom close enough to see the elf.
	var pivot := _main_scene.get_node("CameraPivot")
	pivot._zoom = 15.0
	pivot._update_camera_transform()

	# -- Click on the elf's projected screen position --
	# Note: unproject_position may not produce valid coords under headless xvfb.
	# If this test fails, the design doc acknowledges this risk and suggests
	# falling back to the selection controller API directly.
	_click_at_world_pos(elf_world_pos)

	# -- Wait for the creature info panel to appear --
	var panel_visible := await _wait_for(func(): return _is_panel_visible("CreatureInfoPanel"), 30)
	if not panel_visible:
		# Fallback: select via the selection controller API directly.
		# This validates the panel wiring even if click-to-select doesn't work
		# under headless rendering.
		var selector := _main_scene.find_child("SelectionController", true, false)
		if selector:
			selector.select_creature_by_id(_fixture.uuid)
			panel_visible = await _wait_for(
				func(): return _is_panel_visible("CreatureInfoPanel"), 30
			)
	assert_true(panel_visible, "CreatureInfoPanel should be visible after selection")

	# -- Verify panel shows correct species --
	# The species label shows "Species: Elf" — use specific text to avoid
	# false positives on other labels containing "Elf".
	var panel := _main_scene.find_child("CreatureInfoPanel", true, false)
	var found_species := _find_text_in_descendants(panel, "Species: Elf")
	assert_true(found_species, "Panel should show 'Species: Elf'")


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
	_press_key(KEY_M)
	var panel_visible := await _wait_for(func(): return _is_panel_visible("MilitaryPanel"), 30)
	assert_true(panel_visible, "MilitaryPanel should open on M key press")

	# -- Verify panel contains "Alpha Squad" text --
	# Wait a frame for the panel to refresh its content.
	await _wait_for(func(): return true, 3)
	var panel := _main_scene.find_child("MilitaryPanel", true, false)
	var found_text := _find_text_in_descendants(panel, "Alpha Squad")
	assert_true(found_text, "Panel should contain 'Alpha Squad' text")

	# -- Verify member count via bridge (more reliable than UI text search) --
	var members: Array = _get_bridge().get_military_group_members(_fixture.group_id)
	assert_eq(members.size(), 1, "Alpha Squad should have 1 member via bridge query")

	# -- Close military panel via M key --
	_press_key(KEY_M)
	var panel_hidden := await _wait_for(func(): return not _is_panel_visible("MilitaryPanel"), 30)
	assert_true(panel_hidden, "MilitaryPanel should close on second M key press")

	# -- Reopen and verify consistency --
	_press_key(KEY_M)
	var reopened := await _wait_for(func(): return _is_panel_visible("MilitaryPanel"), 30)
	assert_true(reopened, "MilitaryPanel should reopen on third M key press")
	# Group data should still be consistent after close+reopen.
	var members_after: Array = _get_bridge().get_military_group_members(_fixture.group_id)
	assert_eq(members_after.size(), 1, "Group should still have 1 member after panel reopen")


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
	# Using the bridge directly rather than the full drag-and-confirm UI flow,
	# as mouse drag simulation is unreliable under headless xvfb.
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
	await _wait_for(func(): return true, 3)

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

	# -- Verify the construction panel opens via B key --
	_press_key(KEY_B)
	var found_construction := await _wait_for(func(): return _is_construction_active(), 30)
	assert_true(found_construction, "Construction mode should activate on B key")
	# Exit construction mode.
	_press_key(KEY_ESCAPE)
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

	# -- Verify initial state matches fixture --
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
