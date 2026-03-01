## In-game pause menu overlay.
##
## A semi-transparent full-screen overlay with Resume, Save Game,
## Main Menu, and Quit buttons. Uses Godot's built-in pause system:
## when shown, get_tree().paused = true freezes _process in main.gd
## (and all other default-mode nodes), stopping the sim. This node's
## process_mode is PROCESS_MODE_ALWAYS so it keeps receiving input.
##
## Save Game opens a save_dialog.gd modal; the pause menu handles
## writing the JSON to `user://saves/<name>.json` via the SimBridge.
##
## ESC key toggles the menu via _unhandled_input. When the menu is hidden,
## placement_controller.gd and selection_controller.gd consume ESC first
## (via set_input_as_handled), so the pause menu only opens when nothing
## else claims ESC. While visible: Q = Quit, S = Save (if enabled). These
## hotkeys are suppressed while the save dialog is open (_save_dialog_open).
##
## Call `setup(bridge)` after construction to enable saving. Without it,
## the Save button remains disabled.
##
## Exposes toggle(), open(), close() so main.gd can wire a menu button.
##
## See also: main.gd (creates and wires this menu), main_menu.gd (target
## of the "Main Menu" button), save_dialog.gd (modal save name input).

extends ColorRect

var _bridge: SimBridge
var _save_btn: Button
var _main_menu_btn: Button
var _save_dialog_open: bool = false
var _is_multiplayer: bool = false


func _ready() -> void:
	# Run even while the tree is paused.
	process_mode = Node.PROCESS_MODE_ALWAYS

	# Full-screen semi-transparent overlay.
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	color = Color(0.12, 0.14, 0.10, 0.85)

	# Centered button column.
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 20)
	center.add_child(vbox)

	# "Paused" header.
	var header := Label.new()
	header.text = "Paused"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 36)
	vbox.add_child(header)

	# Resume button.
	var resume_btn := Button.new()
	resume_btn.text = "Resume"
	resume_btn.custom_minimum_size = Vector2(200, 50)
	resume_btn.pressed.connect(close)
	vbox.add_child(resume_btn)

	# Save Game button (enabled after setup() provides a bridge).
	_save_btn = Button.new()
	_save_btn.text = "Save Game"
	_save_btn.custom_minimum_size = Vector2(200, 50)
	_save_btn.disabled = true
	_save_btn.pressed.connect(_on_save_pressed)
	vbox.add_child(_save_btn)

	# Main Menu / Disconnect button.
	_main_menu_btn = Button.new()
	_main_menu_btn.text = "Main Menu"
	_main_menu_btn.custom_minimum_size = Vector2(200, 50)
	_main_menu_btn.pressed.connect(_on_main_menu_pressed)
	vbox.add_child(_main_menu_btn)

	# Quit Game button.
	var quit_btn := Button.new()
	quit_btn.text = "Quit Game"
	quit_btn.custom_minimum_size = Vector2(200, 50)
	quit_btn.pressed.connect(func(): get_tree().quit())
	vbox.add_child(quit_btn)

	# Start hidden.
	visible = false


## Provide the SimBridge reference so Save Game can function.
func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_save_btn.disabled = false
	_is_multiplayer = bridge.is_multiplayer()
	if _is_multiplayer:
		_main_menu_btn.text = "Disconnect"


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("ui_cancel"):
		toggle()
		get_viewport().set_input_as_handled()
	elif (
		visible
		and not _save_dialog_open
		and event is InputEventKey
		and event.pressed
		and not event.echo
	):
		if event.keycode == KEY_Q:
			get_tree().quit()
		elif event.keycode == KEY_S and not _save_btn.disabled:
			_on_save_pressed()
			get_viewport().set_input_as_handled()


func toggle() -> void:
	if visible:
		close()
	else:
		open()


func open() -> void:
	visible = true
	if not _is_multiplayer:
		get_tree().paused = true


func close() -> void:
	visible = false
	if not _is_multiplayer:
		get_tree().paused = false


func _on_save_pressed() -> void:
	var dialog_script = load("res://scripts/save_dialog.gd")
	var dialog := ColorRect.new()
	dialog.set_script(dialog_script)
	add_child(dialog)
	dialog.save_requested.connect(_do_save)
	_save_dialog_open = true
	dialog.tree_exiting.connect(func(): _save_dialog_open = false)


func _do_save(save_name: String) -> void:
	if _bridge == null:
		return
	var json := _bridge.save_game_json()
	if json.is_empty():
		push_error("PauseMenu: save_game_json returned empty string")
		return

	# Ensure saves directory exists.
	DirAccess.make_dir_recursive_absolute("user://saves")

	# Sanitize file name: replace characters unsafe on Windows/macOS/Linux.
	# Windows forbids: \ / : * ? " < > |
	var safe_name := save_name
	for ch in ["\\", "/", ":", "*", "?", '"', "<", ">", "|"]:
		safe_name = safe_name.replace(ch, "_")
	var path := "user://saves/" + safe_name + ".json"

	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		push_error("PauseMenu: failed to open %s for writing" % path)
		return
	file.store_string(json)
	file.close()
	print("PauseMenu: saved game to %s" % path)


func _on_main_menu_pressed() -> void:
	if _is_multiplayer and _bridge:
		_bridge.disconnect_multiplayer()
		GameSession.multiplayer_mode = ""
	get_tree().paused = false
	get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
