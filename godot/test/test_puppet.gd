## Unit tests for puppet_helpers.gd and puppet_server.gd.
##
## Tests the shared UI interaction helpers with synthetic node trees (no
## scene loading required) and the TCP framing helpers in isolation.
##
## See also: puppet_helpers.gd, puppet_server.gd,
## test_harness_integration.gd (integration tests using the same helpers).
extends GutTest

const PuppetHelpersScript := preload("res://scripts/puppet_helpers.gd")
const PuppetServerScript := preload("res://scripts/puppet_server.gd")

# ===========================================================================
# PuppetHelpers: find_button
# ===========================================================================


func test_find_button_returns_matching_button() -> void:
	var root := Control.new()
	var btn := Button.new()
	btn.text = "Build Platform"
	root.add_child(btn)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	var found := helpers.find_button(root, "Platform")
	assert_eq(found, btn, "Should find button containing 'Platform'")

	root.queue_free()


func test_find_button_returns_null_when_no_match() -> void:
	var root := Control.new()
	var btn := Button.new()
	btn.text = "Build Platform"
	root.add_child(btn)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	var found := helpers.find_button(root, "Destroy")
	assert_null(found, "Should return null when no button matches")

	root.queue_free()


func test_find_button_searches_nested_children() -> void:
	var root := Control.new()
	var container := VBoxContainer.new()
	var btn := Button.new()
	btn.text = "Add Recipe"
	container.add_child(btn)
	root.add_child(container)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	var found := helpers.find_button(root, "Recipe")
	assert_eq(found, btn, "Should find nested button")

	root.queue_free()


func test_find_button_null_root() -> void:
	var helpers := PuppetHelpersScript.new(Control.new())
	var found := helpers.find_button(null, "anything")
	assert_null(found, "Should return null for null root")


# ===========================================================================
# PuppetHelpers: find_all_buttons
# ===========================================================================


func test_find_all_buttons_returns_multiple() -> void:
	var root := Control.new()
	var btn1 := Button.new()
	btn1.text = "Details A"
	var btn2 := Button.new()
	btn2.text = "Details B"
	root.add_child(btn1)
	root.add_child(btn2)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	var found := helpers.find_all_buttons(root, "Details")
	assert_eq(found.size(), 2, "Should find both buttons matching 'Details'")

	root.queue_free()


# ===========================================================================
# PuppetHelpers: find_text_in_descendants
# ===========================================================================


func test_find_text_in_label() -> void:
	var root := Control.new()
	var label := Label.new()
	label.text = "Species: Elf"
	root.add_child(label)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	assert_true(
		helpers.find_text_in_descendants(root, "Species"),
		"Should find substring in Label text",
	)
	assert_false(
		helpers.find_text_in_descendants(root, "Dwarf"),
		"Should not find non-existent substring",
	)

	root.queue_free()


func test_find_text_in_button() -> void:
	var root := Control.new()
	var btn := Button.new()
	btn.text = "Crafting Enabled"
	root.add_child(btn)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	assert_true(
		helpers.find_text_in_descendants(root, "Crafting"),
		"Should find substring in Button text",
	)

	root.queue_free()


func test_find_text_null_root() -> void:
	var helpers := PuppetHelpersScript.new(Control.new())
	assert_false(
		helpers.find_text_in_descendants(null, "anything"),
		"Should return false for null root",
	)


# ===========================================================================
# PuppetHelpers: find_button_near_label
# ===========================================================================


func test_find_button_near_label_same_parent() -> void:
	var root := HBoxContainer.new()
	var label := Label.new()
	label.text = "Grow Oak Bow"
	var btn := Button.new()
	btn.text = "X"
	root.add_child(label)
	root.add_child(btn)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	var found := helpers.find_button_near_label(root, "Oak Bow", "X")
	assert_eq(found, btn, "Should find 'X' button near 'Oak Bow' label")

	root.queue_free()


func test_find_button_near_label_no_match() -> void:
	var root := HBoxContainer.new()
	var label := Label.new()
	label.text = "Grow Oak Bow"
	var btn := Button.new()
	btn.text = "Remove"
	root.add_child(label)
	root.add_child(btn)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	var found := helpers.find_button_near_label(root, "Oak Bow", "X")
	assert_null(found, "Should return null when button text doesn't match")

	root.queue_free()


func test_find_button_near_label_nested() -> void:
	var outer := VBoxContainer.new()
	var row := HBoxContainer.new()
	var label := Label.new()
	label.text = "Recipe: Arrow"
	var btn := Button.new()
	btn.text = "+1"
	row.add_child(label)
	row.add_child(btn)
	outer.add_child(row)
	add_child(outer)

	var helpers := PuppetHelpersScript.new(outer)
	var found := helpers.find_button_near_label(outer, "Arrow", "+1")
	assert_eq(found, btn, "Should find button in nested container")

	outer.queue_free()


# ===========================================================================
# PuppetHelpers: press_button
# ===========================================================================


func test_press_button_emits_signal() -> void:
	var btn := Button.new()
	btn.text = "Test"
	add_child(btn)
	var pressed_count := [0]
	btn.pressed.connect(func(): pressed_count[0] += 1)

	var helpers := PuppetHelpersScript.new(btn)
	helpers.press_button(btn)
	assert_eq(pressed_count[0], 1, "Should emit pressed signal")

	btn.queue_free()


func test_press_button_skips_disabled() -> void:
	var btn := Button.new()
	btn.text = "Disabled"
	btn.disabled = true
	add_child(btn)
	var pressed_count := [0]
	btn.pressed.connect(func(): pressed_count[0] += 1)

	var helpers := PuppetHelpersScript.new(btn)
	helpers.press_button(btn)
	assert_eq(pressed_count[0], 0, "Should not emit pressed for disabled button")

	btn.queue_free()


# ===========================================================================
# PuppetHelpers: find_line_edit_with_text
# ===========================================================================


func test_find_line_edit_with_text_match() -> void:
	var root := Control.new()
	var edit := LineEdit.new()
	edit.text = "5"
	root.add_child(edit)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	assert_true(
		helpers.find_line_edit_with_text(root, "5"),
		"Should find LineEdit with matching text",
	)
	assert_false(
		helpers.find_line_edit_with_text(root, "10"),
		"Should not match different text",
	)

	root.queue_free()


# ===========================================================================
# PuppetHelpers: is_panel_visible / read_panel_text
# ===========================================================================


func test_is_panel_visible() -> void:
	var root := Control.new()
	root.name = "Root"
	var panel := PanelContainer.new()
	panel.name = "TestPanel"
	panel.visible = true
	root.add_child(panel)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	assert_true(helpers.is_panel_visible("TestPanel"), "Visible panel should return true")
	panel.visible = false
	assert_false(helpers.is_panel_visible("TestPanel"), "Hidden panel should return false")
	assert_false(helpers.is_panel_visible("NonExistent"), "Missing panel should return false")

	root.queue_free()


func test_read_panel_text_label() -> void:
	var root := Control.new()
	root.name = "Root"
	var label := Label.new()
	label.name = "StatusLabel"
	label.text = "5 Elves"
	root.add_child(label)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)
	assert_eq(helpers.read_panel_text("StatusLabel"), "5 Elves")
	assert_eq(helpers.read_panel_text("Missing"), "", "Missing node returns empty string")

	root.queue_free()


# ===========================================================================
# PuppetServer: key name to code mapping
# ===========================================================================


func test_key_name_to_code() -> void:
	var server := PuppetServerScript.new()
	add_child(server)

	# Letters (case-insensitive).
	assert_eq(server._key_name_to_code("A"), KEY_A)
	assert_eq(server._key_name_to_code("z"), KEY_Z)
	assert_eq(server._key_name_to_code("m"), KEY_M)
	# Digits.
	assert_eq(server._key_name_to_code("0"), KEY_0)
	assert_eq(server._key_name_to_code("9"), KEY_9)
	# Named keys.
	assert_eq(server._key_name_to_code("ESCAPE"), KEY_ESCAPE)
	assert_eq(server._key_name_to_code("esc"), KEY_ESCAPE)
	assert_eq(server._key_name_to_code("HOME"), KEY_HOME)
	assert_eq(server._key_name_to_code("F1"), KEY_F1)
	assert_eq(server._key_name_to_code("SPACE"), KEY_SPACE)
	assert_eq(server._key_name_to_code("unknown_key"), KEY_NONE)

	server.queue_free()


# ===========================================================================
# PuppetServer: TCP framing (_send_response encoding)
# ===========================================================================
# We can't easily test the full TCP flow without a real connection, but we
# can test the _variant_to_json_safe conversion.


func test_variant_to_json_safe() -> void:
	var server := PuppetServerScript.new()
	add_child(server)

	# Dictionary passthrough.
	var dict_out = server._variant_to_json_safe({"key": "value", "num": 42})
	assert_eq(dict_out["key"], "value")
	assert_eq(dict_out["num"], 42)

	# Vector3 -> {x, y, z}.
	var v3_out = server._variant_to_json_safe(Vector3(1.5, 2.0, 3.5))
	assert_eq(v3_out["x"], 1.5)
	assert_eq(v3_out["y"], 2.0)
	assert_eq(v3_out["z"], 3.5)

	# Array with mixed types.
	var arr_out = server._variant_to_json_safe([Vector3i(1, 2, 3), "hello"])
	assert_eq(arr_out[0]["x"], 1)
	assert_eq(arr_out[1], "hello")

	server.queue_free()


# ===========================================================================
# PuppetHelpers: collect_text and list_panels
# ===========================================================================


func test_collect_text_and_list_panels() -> void:
	var root := Control.new()
	root.name = "TestRoot"
	var label := Label.new()
	label.name = "InfoLabel"
	label.text = "Species: Elf"
	var btn := Button.new()
	btn.name = "ActionBtn"
	btn.text = "Build"
	var empty_label := Label.new()
	empty_label.name = "EmptyLabel"
	empty_label.text = ""
	root.add_child(label)
	root.add_child(btn)
	root.add_child(empty_label)
	add_child(root)

	var helpers := PuppetHelpersScript.new(root)

	# collect_text: returns non-empty text nodes.
	var texts: Array = helpers.collect_text(root)
	assert_eq(texts.size(), 2, "Should collect label and button, skip empty label")
	assert_eq(texts[0]["node_name"], "InfoLabel")
	assert_eq(texts[0]["node_type"], "Label")
	assert_eq(texts[0]["text"], "Species: Elf")
	assert_eq(texts[1]["node_name"], "ActionBtn")
	assert_eq(texts[1]["node_type"], "Button")
	assert_eq(texts[1]["text"], "Build")

	# list_panels: returns all Controls (not just "panels"), skips @ names.
	var panels: Array = helpers.list_panels()
	var names: Array = []
	for p in panels:
		names.append(p["name"])
	assert_true("TestRoot" in names, "Should include root Control")
	assert_true("InfoLabel" in names, "Should include Label (it's a Control)")
	assert_true("ActionBtn" in names, "Should include Button")

	root.queue_free()


# ===========================================================================
# PuppetServer: dispatch error paths
# ===========================================================================


func test_dispatch_errors_and_no_scene_guards() -> void:
	var server := PuppetServerScript.new()
	add_child(server)

	# Unknown method.
	var r1 = server._dispatch("nonexistent", [])
	assert_true(r1 is Dictionary and r1.has("_error"), "Unknown method should error")
	assert_true("unknown method" in r1["_error"], "Error should mention unknown method")

	# Insufficient args.
	var r2 = server._dispatch("press-key", [])
	assert_true(r2 is Dictionary and r2.has("_error"), "Missing args should error")
	assert_true("requires" in r2["_error"], "Error should mention required args")

	# Camera-dependent RPCs return "scene not loaded" when no scene is active.
	# (Coordinate format validation is only reached after the scene/camera guards.)
	var r3 = server._dispatch("click-at-world-pos", ["1,2"])
	assert_true(
		r3 is Dictionary and r3.has("_error"), "click-at-world-pos should error with no scene"
	)
	# Error is either "scene not loaded" or "no camera" depending on test tree state.
	assert_true("scene" in r3["_error"] or "camera" in r3["_error"], "Should guard on scene/camera")

	var r4 = server._dispatch("move-camera-to", ["just-a-string"])
	assert_true(r4 is Dictionary and r4.has("_error"), "move-camera-to should error with no scene")
	assert_true("scene" in r4["_error"] or "camera" in r4["_error"], "Should guard on scene/camera")

	server.queue_free()

	# --- has_camera guard ---
	# No CameraPivot — should return false.
	var root_no_cam := Control.new()
	add_child(root_no_cam)
	var helpers_no_cam := PuppetHelpersScript.new(root_no_cam)
	assert_false(helpers_no_cam.has_camera(), "Should return false without CameraPivot")
	root_no_cam.queue_free()

	# With CameraPivot — should return true.
	var root_cam := Node3D.new()
	var pivot := Node3D.new()
	pivot.name = "CameraPivot"
	root_cam.add_child(pivot)
	add_child(root_cam)
	var helpers_cam := PuppetHelpersScript.new(root_cam)
	assert_true(helpers_cam.has_camera(), "Should return true with CameraPivot")
	root_cam.queue_free()
