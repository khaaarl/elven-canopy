## Shared UI interaction helpers used by both the puppet TCP server
## (puppet_server.gd) and integration tests (test_harness_integration.gd).
##
## Instantiate with a scene root reference:
##   var helpers := PuppetHelpers.new(main_scene)
##
## All methods that search the scene tree start from `scene_root`.  Methods
## that need camera or input access (click_at_world_pos, press_key) go
## through Godot's Input singleton and assume a CameraPivot/Camera3D exists
## under the scene root.
##
## See also: puppet_server.gd (TCP server that dispatches RPCs to these
## helpers), test_harness_integration.gd (GUT integration tests).
extends RefCounted

## The root node of the game scene.  All find_child / get_node calls start
## here.  Set on construction; may be updated if the scene is reloaded.
var scene_root: Node


func _init(root: Node) -> void:
	scene_root = root


# ---------------------------------------------------------------------------
# Camera and clicking
# ---------------------------------------------------------------------------


## Whether the scene has a CameraPivot (false on menu scenes).
func has_camera() -> bool:
	return scene_root.get_node_or_null("CameraPivot") != null


## Position the camera pivot to look at a world position.
func move_camera_to(world_pos: Vector3) -> void:
	var pivot := scene_root.get_node("CameraPivot")
	pivot.position = world_pos


## Send a synthetic key press+release event through the full input pipeline.
func press_key(keycode: int) -> void:
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
func click_at_world_pos(world_pos: Vector3) -> void:
	var camera: Camera3D = scene_root.get_node("CameraPivot/Camera3D")
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
# UI reading
# ---------------------------------------------------------------------------


## Read text from a Label or RichTextLabel by name (recursive search).
func read_panel_text(node_name: String) -> String:
	var node := scene_root.find_child(node_name, true, false)
	if node is RichTextLabel:
		return node.get_parsed_text()
	if node is Label:
		return node.text
	return ""


## Check if a named Control node is visible (recursive search).
func is_panel_visible(node_name: String) -> bool:
	var node := scene_root.find_child(node_name, true, false)
	return node != null and node.visible


## Recursively search a node's descendants for a Label, Button, or
## RichTextLabel whose text contains the given substring.
func find_text_in_descendants(root: Node, substring: String) -> bool:
	if root == null:
		return false
	if root is Label and substring in root.text:
		return true
	if root is Button and substring in root.text:
		return true
	if root is RichTextLabel and substring in root.get_parsed_text():
		return true
	for child in root.get_children():
		if find_text_in_descendants(child, substring):
			return true
	return false


# ---------------------------------------------------------------------------
# Button search and interaction
# ---------------------------------------------------------------------------


## Recursively find a Button whose text contains `substring`.
## Searches all descendants regardless of visibility.
func find_button(root: Node, substring: String) -> Button:
	if root == null:
		return null
	if root is Button and substring in root.text:
		return root
	for child in root.get_children():
		var found := find_button(child, substring)
		if found:
			return found
	return null


## Recursively find ALL Buttons whose text contains `substring`.
func find_all_buttons(root: Node, substring: String) -> Array[Button]:
	var result: Array[Button] = []
	if root == null:
		return result
	if root is Button and substring in root.text:
		result.append(root)
	for child in root.get_children():
		result.append_array(find_all_buttons(child, substring))
	return result


## Recursively search for a LineEdit whose text matches `expected_text`.
func find_line_edit_with_text(root: Node, expected_text: String) -> bool:
	if root == null:
		return false
	if root is LineEdit and root.text == expected_text:
		return true
	for child in root.get_children():
		if find_line_edit_with_text(child, expected_text):
			return true
	return false


## Programmatically press a Button node (emits its pressed signal).
## Skips disabled buttons — a disabled button being pressed is a test bug.
func press_button(btn: Button) -> void:
	if btn.disabled:
		push_warning("press_button: button '%s' is disabled, skipping" % btn.text)
		return
	btn.emit_signal("pressed")


## Find a Button near a Label containing `label_text`.  Searches all
## containers in the subtree for one that has both a Label matching
## `label_text` and a Button matching `button_text`.
func find_button_near_label(root: Node, label_text: String, button_text: String) -> Button:
	if root == null:
		return null
	# Check if this node's children contain both the label and button.
	var has_label := false
	var candidate_btn: Button = null
	for child in root.get_children():
		if child is Label and label_text in child.text:
			has_label = true
		if child is Button and button_text == child.text:
			candidate_btn = child
	if has_label and candidate_btn:
		return candidate_btn
	# Recurse into children.
	for child in root.get_children():
		var found := find_button_near_label(child, label_text, button_text)
		if found:
			return found
	return null


# ---------------------------------------------------------------------------
# Text collection
# ---------------------------------------------------------------------------


## Collect all visible text from a subtree.  Returns an array of dicts:
## [{node_name, node_type, text}] for every Label, RichTextLabel, and Button
## with non-empty text.
func collect_text(root: Node) -> Array[Dictionary]:
	var result: Array[Dictionary] = []
	_collect_text_recursive(root, result)
	return result


func _collect_text_recursive(node: Node, out: Array[Dictionary]) -> void:
	if node == null:
		return
	var text := ""
	var node_type := ""
	if node is Label:
		text = node.text
		node_type = "Label"
	elif node is RichTextLabel:
		text = node.get_parsed_text()
		node_type = "RichTextLabel"
	elif node is Button:
		text = node.text
		node_type = "Button"
	if not text.is_empty():
		(
			out
			. append(
				{
					"node_name": str(node.name),
					"node_type": node_type,
					"text": text,
				}
			)
		)
	for child in node.get_children():
		_collect_text_recursive(child, out)


# ---------------------------------------------------------------------------
# Panel listing (used by puppet server for list-panels RPC)
# ---------------------------------------------------------------------------


## Return an array of dicts [{name, visible}] for all named Control nodes
## found recursively under scene_root.  Includes every Control whose name
## doesn't start with "@" (Godot's auto-generated prefix).
func list_panels() -> Array[Dictionary]:
	var result: Array[Dictionary] = []
	_collect_panels(scene_root, result)
	return result


func _collect_panels(node: Node, out: Array[Dictionary]) -> void:
	if node is Control and not node.name.begins_with("@"):
		out.append({"name": str(node.name), "visible": node.visible})
	for child in node.get_children():
		_collect_panels(child, out)
