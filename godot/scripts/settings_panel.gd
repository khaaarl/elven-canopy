## Settings overlay panel — displays and edits GameConfig values.
##
## A full-screen modal overlay (same pattern as save_dialog.gd / load_dialog.gd)
## with a centered panel containing settings controls and Save/Cancel buttons.
## Opened from both the main menu and the escape menu.
##
## Current settings:
##   - Player name (LineEdit, max 32 chars)
##   - Start paused (toggle button)
##   - Draw distance (LineEdit, 0–500 voxels, 0 = unlimited)
##   - Edge scrolling (cycle button: Off → Pan → Rotate)
##   - Audio volume (HSlider, 0–100%)
##   - Fog enabled (toggle button)
##   - Fog begin (LineEdit, voxels)
##   - Fog end (LineEdit, voxels)
##   - Window mode (dropdown: Windowed / Borderless Fullscreen / Exclusive Fullscreen)
##   - Edge outline (toggle button)
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

const EDGE_SCROLL_MODES := ["off", "pan", "rotate"]
const EDGE_SCROLL_LABELS := {"off": "OFF", "pan": "PAN", "rotate": "ROTATE"}
const WINDOW_MODES := ["windowed", "borderless_fullscreen", "exclusive_fullscreen"]
const WINDOW_MODE_LABELS := {
	"windowed": "Windowed",
	"borderless_fullscreen": "Borderless Fullscreen",
	"exclusive_fullscreen": "Exclusive Fullscreen",
}

var _config: Node  ## GameConfig (or test stand-in)
var _name_input: LineEdit
var _paused_toggle: Button
var _draw_distance_input: LineEdit
var _volume_slider: HSlider
var _volume_label: Label
var _fog_toggle: Button
var _fog_begin_input: LineEdit
var _fog_end_input: LineEdit
var _edge_outline_toggle: Button
var _edge_scroll_toggle: Button
var _window_mode_dropdown: OptionButton
var _save_btn: Button
var _paused_value: bool = false
var _fog_enabled_value: bool = true
var _edge_outline_value: bool = true
var _edge_scroll_mode: String = "off"


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

	# Start paused row.
	# NOTE: Avoid CheckBox — Godot's default theme renders checkboxes as
	# near-invisible dark marks on dark background with no outline. Use a
	# text-based toggle button instead.
	var paused_row := HBoxContainer.new()
	paused_row.add_theme_constant_override("separation", 10)
	settings_vbox.add_child(paused_row)

	var paused_label := Label.new()
	paused_label.text = "Start Paused"
	paused_label.custom_minimum_size = Vector2(160, 0)
	paused_row.add_child(paused_label)

	_paused_toggle = Button.new()
	_paused_toggle.custom_minimum_size = Vector2(120, 0)
	_paused_toggle.pressed.connect(_toggle_paused)
	paused_row.add_child(_paused_toggle)

	# Draw distance row.
	var draw_dist_row := HBoxContainer.new()
	draw_dist_row.add_theme_constant_override("separation", 10)
	settings_vbox.add_child(draw_dist_row)

	var draw_dist_label := Label.new()
	draw_dist_label.text = "Draw Distance"
	draw_dist_label.custom_minimum_size = Vector2(160, 0)
	draw_dist_row.add_child(draw_dist_label)

	_draw_distance_input = LineEdit.new()
	_draw_distance_input.custom_minimum_size = Vector2(120, 0)
	_draw_distance_input.placeholder_text = "0–500 (0 = unlimited)"
	_draw_distance_input.tooltip_text = "Chunk draw distance in voxels (0 = unlimited)"
	draw_dist_row.add_child(_draw_distance_input)

	var draw_dist_unit := Label.new()
	draw_dist_unit.text = "voxels"
	draw_dist_row.add_child(draw_dist_unit)

	# Edge scroll mode row.
	var edge_scroll_row := HBoxContainer.new()
	edge_scroll_row.add_theme_constant_override("separation", 10)
	settings_vbox.add_child(edge_scroll_row)

	var edge_scroll_label := Label.new()
	edge_scroll_label.text = "Edge Scrolling"
	edge_scroll_label.custom_minimum_size = Vector2(160, 0)
	edge_scroll_row.add_child(edge_scroll_label)

	_edge_scroll_toggle = Button.new()
	_edge_scroll_toggle.custom_minimum_size = Vector2(120, 0)
	_edge_scroll_toggle.pressed.connect(_cycle_edge_scroll)
	edge_scroll_row.add_child(_edge_scroll_toggle)

	# --- Audio section ---
	var audio_label := Label.new()
	audio_label.text = "Audio"
	audio_label.add_theme_font_size_override("font_size", 18)
	outer_vbox.add_child(audio_label)

	var audio_vbox := VBoxContainer.new()
	audio_vbox.add_theme_constant_override("separation", 10)
	outer_vbox.add_child(audio_vbox)

	# Volume row.
	var volume_row := HBoxContainer.new()
	volume_row.add_theme_constant_override("separation", 10)
	audio_vbox.add_child(volume_row)

	var volume_label := Label.new()
	volume_label.text = "Volume"
	volume_label.custom_minimum_size = Vector2(160, 0)
	volume_row.add_child(volume_label)

	_volume_slider = HSlider.new()
	_volume_slider.min_value = 0
	_volume_slider.max_value = 100
	_volume_slider.step = 1
	_volume_slider.custom_minimum_size = Vector2(200, 0)
	_volume_slider.value_changed.connect(_on_volume_changed)
	volume_row.add_child(_volume_slider)

	_volume_label = Label.new()
	_volume_label.custom_minimum_size = Vector2(40, 0)
	volume_row.add_child(_volume_label)

	# --- Visual section ---
	var visual_label := Label.new()
	visual_label.text = "Visual"
	visual_label.add_theme_font_size_override("font_size", 18)
	outer_vbox.add_child(visual_label)

	var visual_vbox := VBoxContainer.new()
	visual_vbox.add_theme_constant_override("separation", 10)
	outer_vbox.add_child(visual_vbox)

	# Window mode row.
	var window_mode_row := HBoxContainer.new()
	window_mode_row.add_theme_constant_override("separation", 10)
	visual_vbox.add_child(window_mode_row)

	var window_mode_label := Label.new()
	window_mode_label.text = "Window Mode"
	window_mode_label.custom_minimum_size = Vector2(160, 0)
	window_mode_row.add_child(window_mode_label)

	_window_mode_dropdown = OptionButton.new()
	_window_mode_dropdown.custom_minimum_size = Vector2(200, 0)
	for mode: String in WINDOW_MODES:
		_window_mode_dropdown.add_item(WINDOW_MODE_LABELS[mode])
	window_mode_row.add_child(_window_mode_dropdown)

	# Fog enabled row.
	var fog_row := HBoxContainer.new()
	fog_row.add_theme_constant_override("separation", 10)
	visual_vbox.add_child(fog_row)

	var fog_label := Label.new()
	fog_label.text = "Distance Fog"
	fog_label.custom_minimum_size = Vector2(160, 0)
	fog_row.add_child(fog_label)

	_fog_toggle = Button.new()
	_fog_toggle.custom_minimum_size = Vector2(120, 0)
	_fog_toggle.pressed.connect(_toggle_fog)
	fog_row.add_child(_fog_toggle)

	# Fog begin row.
	var fog_begin_row := HBoxContainer.new()
	fog_begin_row.add_theme_constant_override("separation", 10)
	visual_vbox.add_child(fog_begin_row)

	var fog_begin_label := Label.new()
	fog_begin_label.text = "Fog Begin"
	fog_begin_label.custom_minimum_size = Vector2(160, 0)
	fog_begin_row.add_child(fog_begin_label)

	_fog_begin_input = LineEdit.new()
	_fog_begin_input.custom_minimum_size = Vector2(120, 0)
	_fog_begin_input.placeholder_text = "0–500"
	_fog_begin_input.tooltip_text = "Distance (voxels) where fog starts"
	fog_begin_row.add_child(_fog_begin_input)

	var fog_begin_unit := Label.new()
	fog_begin_unit.text = "voxels"
	fog_begin_row.add_child(fog_begin_unit)

	# Fog end row.
	var fog_end_row := HBoxContainer.new()
	fog_end_row.add_theme_constant_override("separation", 10)
	visual_vbox.add_child(fog_end_row)

	var fog_end_label := Label.new()
	fog_end_label.text = "Fog End"
	fog_end_label.custom_minimum_size = Vector2(160, 0)
	fog_end_row.add_child(fog_end_label)

	_fog_end_input = LineEdit.new()
	_fog_end_input.custom_minimum_size = Vector2(120, 0)
	_fog_end_input.placeholder_text = "0–500"
	_fog_end_input.tooltip_text = "Distance (voxels) where fog is fully opaque"
	fog_end_row.add_child(_fog_end_input)

	var fog_end_unit := Label.new()
	fog_end_unit.text = "voxels"
	fog_end_row.add_child(fog_end_unit)

	# Edge outline row.
	var edge_outline_row := HBoxContainer.new()
	edge_outline_row.add_theme_constant_override("separation", 10)
	visual_vbox.add_child(edge_outline_row)

	var edge_outline_label := Label.new()
	edge_outline_label.text = "Edge Outline"
	edge_outline_label.custom_minimum_size = Vector2(160, 0)
	edge_outline_row.add_child(edge_outline_label)

	_edge_outline_toggle = Button.new()
	_edge_outline_toggle.custom_minimum_size = Vector2(120, 0)
	_edge_outline_toggle.pressed.connect(_toggle_edge_outline)
	edge_outline_row.add_child(_edge_outline_toggle)

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


func _on_volume_changed(value: float) -> void:
	_volume_label.text = "%d%%" % int(value)


func _cycle_edge_scroll() -> void:
	var idx := EDGE_SCROLL_MODES.find(_edge_scroll_mode)
	_edge_scroll_mode = EDGE_SCROLL_MODES[(idx + 1) % EDGE_SCROLL_MODES.size()]
	_update_edge_scroll_label()


func _update_edge_scroll_label() -> void:
	_edge_scroll_toggle.text = EDGE_SCROLL_LABELS.get(_edge_scroll_mode, "OFF")


func _toggle_fog() -> void:
	_fog_enabled_value = not _fog_enabled_value
	_update_fog_label()


func _update_fog_label() -> void:
	_fog_toggle.text = "\u2713 ENABLED" if _fog_enabled_value else "\u2717 DISABLED"


func _toggle_edge_outline() -> void:
	_edge_outline_value = not _edge_outline_value
	_update_edge_outline_label()


func _update_edge_outline_label() -> void:
	_edge_outline_toggle.text = "\u2713 ENABLED" if _edge_outline_value else "\u2717 DISABLED"


## Parse draw distance text to a clamped int (0–500). Invalid input returns
## the current config value (or the default 50 if no config is set).
func _parse_draw_distance() -> int:
	var text := _draw_distance_input.text.strip_edges()
	if not text.is_valid_int():
		return _config.get_setting("draw_distance") if _config else 50
	return clampi(text.to_int(), 0, 500)


## Parse fog begin text to a clamped int (0–500). Invalid input returns
## the current config value (or default 40).
func _parse_fog_begin() -> int:
	var text := _fog_begin_input.text.strip_edges()
	if not text.is_valid_int():
		return _config.get_setting("fog_begin") if _config else 40
	return clampi(text.to_int(), 0, 500)


## Parse fog end text to a clamped int (0–500). Invalid input returns
## the current config value (or default 80).
func _parse_fog_end() -> int:
	var text := _fog_end_input.text.strip_edges()
	if not text.is_valid_int():
		return _config.get_setting("fog_end") if _config else 80
	return clampi(text.to_int(), 0, 500)


## Populate controls from the given config and show the panel.
func open(config: Node) -> void:
	_config = config
	_name_input.text = config.get_setting("player_name")
	_paused_value = config.get_setting("start_paused")
	_update_paused_label()
	_draw_distance_input.text = str(config.get_setting("draw_distance"))
	_volume_slider.value = config.get_setting("audio_volume")
	_volume_label.text = "%d%%" % config.get_setting("audio_volume")
	_fog_enabled_value = config.get_setting("fog_enabled")
	_update_fog_label()
	_fog_begin_input.text = str(config.get_setting("fog_begin"))
	_fog_end_input.text = str(config.get_setting("fog_end"))
	_edge_outline_value = config.get_setting("edge_outline")
	_update_edge_outline_label()
	_edge_scroll_mode = config.get_setting("edge_scroll_mode")
	if _edge_scroll_mode not in EDGE_SCROLL_MODES:
		_edge_scroll_mode = "off"
	_update_edge_scroll_label()
	var wm: String = config.get_setting("window_mode")
	var wm_idx := WINDOW_MODES.find(wm)
	_window_mode_dropdown.selected = wm_idx if wm_idx >= 0 else 0
	_save_btn.disabled = _name_input.text.strip_edges().is_empty()


## Write edited values to GameConfig and close.
func save_and_close() -> void:
	var name_text := _name_input.text.strip_edges()
	if not name_text.is_empty():
		_config.set_setting("player_name", name_text)
	_config.set_setting("start_paused", _paused_value)
	_config.set_setting("draw_distance", _parse_draw_distance())
	_config.set_setting("audio_volume", int(_volume_slider.value))
	_config.set_setting("fog_enabled", _fog_enabled_value)
	_config.set_setting("fog_begin", _parse_fog_begin())
	_config.set_setting("fog_end", _parse_fog_end())
	_config.set_setting("edge_outline", _edge_outline_value)
	_config.set_setting("edge_scroll_mode", _edge_scroll_mode)
	var wm_value: String = WINDOW_MODES[_window_mode_dropdown.selected]
	_config.set_setting("window_mode", wm_value)
	_apply_window_mode(wm_value)
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


func get_draw_distance_value() -> int:
	return _parse_draw_distance()


func set_draw_distance_value(value: int) -> void:
	_draw_distance_input.text = str(value)


func get_audio_volume_value() -> int:
	return int(_volume_slider.value)


func set_audio_volume_value(value: int) -> void:
	_volume_slider.value = value
	_volume_label.text = "%d%%" % value


func get_fog_enabled() -> bool:
	return _fog_enabled_value


func set_fog_enabled(value: bool) -> void:
	_fog_enabled_value = value
	_update_fog_label()


func get_fog_begin_value() -> int:
	return _parse_fog_begin()


func set_fog_begin_value(value: int) -> void:
	_fog_begin_input.text = str(value)


func get_fog_end_value() -> int:
	return _parse_fog_end()


func set_fog_end_value(value: int) -> void:
	_fog_end_input.text = str(value)


func get_edge_outline_enabled() -> bool:
	return _edge_outline_value


func set_edge_outline_enabled(value: bool) -> void:
	_edge_outline_value = value
	_update_edge_outline_label()


func get_edge_scroll_mode() -> String:
	return _edge_scroll_mode


func set_edge_scroll_mode(value: String) -> void:
	_edge_scroll_mode = value
	_update_edge_scroll_label()


func get_window_mode() -> String:
	return WINDOW_MODES[_window_mode_dropdown.selected]


func set_window_mode(value: String) -> void:
	var idx := WINDOW_MODES.find(value)
	_window_mode_dropdown.selected = idx if idx >= 0 else 0


## Apply a window mode string via DisplayServer.
static func _apply_window_mode(mode_str: String) -> void:
	match mode_str:
		"borderless_fullscreen":
			DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_FULLSCREEN)
		"exclusive_fullscreen":
			DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_EXCLUSIVE_FULLSCREEN)
		_:
			DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_MAXIMIZED)
