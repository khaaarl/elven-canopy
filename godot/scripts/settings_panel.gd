## Settings overlay panel — displays and edits GameConfig values.
##
## A full-screen modal overlay (same pattern as save_dialog.gd / load_dialog.gd)
## with a centered panel containing settings controls and Save/Cancel buttons.
## Opened from both the main menu and the escape menu.
##
## Current settings:
##   - Player name (LineEdit, max 32 chars)
##   - Start paused on load (toggle button)
##
## On open, reads current values from the provided GameConfig instance.
## Save writes values back to GameConfig and closes. Cancel discards edits.
## ESC acts as Cancel.
##
## The panel layout uses a VBoxContainer for sections, so future features
## (e.g. F-controls-config-C keybinding section) can append sections easily.
##
## Exposes get/set helpers for controls so tests can drive the panel without
## needing to dig into the node tree.
##
## See also: game_config.gd (data model), main_menu.gd (opens from main menu),
## escape_menu.gd (opens from pause menu), test_settings_panel.gd.

extends ColorRect

signal closed

var _config: Node  ## GameConfig (or test stand-in)
var _name_input: LineEdit
var _paused_toggle: Button
var _save_btn: Button
var _paused_value: bool = false


func _ready() -> void:
	# Run even while the tree is paused (escape menu pauses the tree).
	process_mode = Node.PROCESS_MODE_ALWAYS

	# Full-screen semi-transparent overlay.
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	color = Color(0, 0, 0, 0.7)

	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)

	var panel := PanelContainer.new()
	center.add_child(panel)

	var outer_vbox := VBoxContainer.new()
	outer_vbox.add_theme_constant_override("separation", 16)
	panel.add_child(outer_vbox)

	# Header.
	var header := Label.new()
	header.text = "Settings"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 28)
	outer_vbox.add_child(header)

	# --- General section ---
	var section_label := Label.new()
	section_label.text = "General"
	section_label.add_theme_font_size_override("font_size", 18)
	outer_vbox.add_child(section_label)

	var settings_vbox := VBoxContainer.new()
	settings_vbox.add_theme_constant_override("separation", 10)
	outer_vbox.add_child(settings_vbox)

	# Player name row.
	var name_row := HBoxContainer.new()
	name_row.add_theme_constant_override("separation", 10)
	settings_vbox.add_child(name_row)

	var name_label := Label.new()
	name_label.text = "Player Name"
	name_label.custom_minimum_size = Vector2(160, 0)
	name_row.add_child(name_label)

	_name_input = LineEdit.new()
	_name_input.custom_minimum_size = Vector2(200, 0)
	_name_input.max_length = 32
	_name_input.placeholder_text = "Enter your name..."
	name_row.add_child(_name_input)

	# Start paused on load row.
	# NOTE: Avoid CheckBox — Godot's default theme renders checkboxes as
	# near-invisible dark marks on dark background with no outline. Use a
	# text-based toggle button instead.
	var paused_row := HBoxContainer.new()
	paused_row.add_theme_constant_override("separation", 10)
	settings_vbox.add_child(paused_row)

	var paused_label := Label.new()
	paused_label.text = "Start Paused on Load"
	paused_label.custom_minimum_size = Vector2(160, 0)
	paused_row.add_child(paused_label)

	_paused_toggle = Button.new()
	_paused_toggle.custom_minimum_size = Vector2(120, 0)
	_paused_toggle.pressed.connect(_toggle_paused)
	paused_row.add_child(_paused_toggle)

	# --- Button row ---
	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 8)
	outer_vbox.add_child(spacer)

	var btn_row := HBoxContainer.new()
	btn_row.add_theme_constant_override("separation", 12)
	btn_row.alignment = BoxContainer.ALIGNMENT_CENTER
	outer_vbox.add_child(btn_row)

	_save_btn = Button.new()
	_save_btn.text = "Save"
	_save_btn.custom_minimum_size = Vector2(100, 40)
	_save_btn.pressed.connect(save_and_close)
	btn_row.add_child(_save_btn)

	var cancel_btn := Button.new()
	cancel_btn.text = "Cancel"
	cancel_btn.custom_minimum_size = Vector2(100, 40)
	cancel_btn.pressed.connect(cancel_and_close)
	btn_row.add_child(cancel_btn)

	# Enable save button only when name is non-empty.
	_name_input.text_changed.connect(_on_name_text_changed)


func _on_name_text_changed(_text: String) -> void:
	_save_btn.disabled = _name_input.text.strip_edges().is_empty()


func _toggle_paused() -> void:
	_paused_value = not _paused_value
	_update_paused_label()


func _update_paused_label() -> void:
	_paused_toggle.text = "\u2713 ENABLED" if _paused_value else "\u2717 DISABLED"


## Populate controls from the given config and show the panel.
func open(config: Node) -> void:
	_config = config
	_name_input.text = config.get_setting("player_name")
	_paused_value = config.get_setting("start_paused_on_load")
	_update_paused_label()
	_save_btn.disabled = _name_input.text.strip_edges().is_empty()


## Write edited values to GameConfig and close.
func save_and_close() -> void:
	var name_text := _name_input.text.strip_edges()
	if not name_text.is_empty():
		_config.set_setting("player_name", name_text)
	_config.set_setting("start_paused_on_load", _paused_value)
	closed.emit()
	queue_free()


## Discard edits and close.
func cancel_and_close() -> void:
	closed.emit()
	queue_free()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == KEY_ESCAPE:
			get_viewport().set_input_as_handled()
			cancel_and_close()
			return
		# Consume all key input while open.
		get_viewport().set_input_as_handled()


# --- Test helpers ---


func get_player_name_text() -> String:
	return _name_input.text


func set_player_name_text(value: String) -> void:
	_name_input.text = value


func get_start_paused_checked() -> bool:
	return _paused_value


func set_start_paused_checked(value: bool) -> void:
	_paused_value = value
	_update_paused_label()
