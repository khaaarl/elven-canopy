## Host Game screen — configure tree shape, session name, and start hosting.
##
## Combines tree config (seed, presets, sliders — same pattern as new_game_menu.gd)
## with multiplayer session config (name, password, port, max players). Writes
## everything to GameSession with `multiplayer_mode = "host"`, then transitions
## to main.tscn where the SimBridge will start the relay and connect.
##
## See also: new_game_menu.gd (similar tree config UI), multiplayer_menu.gd
## (previous screen), game_session.gd (stores config), main.gd (reads it).

extends Control

const PRESET_NAMES: Array[String] = ["Fantasy Mega", "Oak", "Conifer", "Willow"]

const PRESETS: Dictionary = {
	"Fantasy Mega":
	{
		"growth":
		{
			"initial_energy": 400.0,
			"energy_to_radius": 0.08,
			"min_radius": 0.5,
			"growth_step_length": 1.0,
			"energy_per_step": 1.0
		},
		"split":
		{
			"split_chance_base": 0.12,
			"split_count": 1,
			"split_energy_ratio": 0.35,
			"split_angle": 0.7,
			"split_angle_variance": 0.3,
			"min_progress_for_split": 0.15
		},
		"curvature": {"gravitropism": 0.08, "random_deflection": 0.15, "deflection_coherence": 0.7},
		"roots":
		{
			"root_energy_fraction": 0.15,
			"root_initial_count": 5,
			"root_gravitropism": 0.12,
			"root_initial_angle": 0.3,
			"root_surface_tendency": 0.8
		},
		"leaves":
		{"leaf_shape": "Sphere", "leaf_density": 0.65, "leaf_size": 3, "canopy_density": 1.0},
		"trunk": {"base_flare": 0.5, "initial_direction": [0.0, 1.0, 0.0]},
	},
	"Oak":
	{
		"growth":
		{
			"initial_energy": 250.0,
			"energy_to_radius": 0.1,
			"min_radius": 0.5,
			"growth_step_length": 1.0,
			"energy_per_step": 1.2
		},
		"split":
		{
			"split_chance_base": 0.18,
			"split_count": 1,
			"split_energy_ratio": 0.4,
			"split_angle": 0.9,
			"split_angle_variance": 0.4,
			"min_progress_for_split": 0.1
		},
		"curvature": {"gravitropism": 0.04, "random_deflection": 0.2, "deflection_coherence": 0.6},
		"roots":
		{
			"root_energy_fraction": 0.12,
			"root_initial_count": 4,
			"root_gravitropism": 0.15,
			"root_initial_angle": 0.2,
			"root_surface_tendency": 0.9
		},
		"leaves":
		{"leaf_shape": "Cloud", "leaf_density": 0.7, "leaf_size": 3, "canopy_density": 1.0},
		"trunk": {"base_flare": 0.3, "initial_direction": [0.0, 1.0, 0.0]},
	},
	"Conifer":
	{
		"growth":
		{
			"initial_energy": 300.0,
			"energy_to_radius": 0.06,
			"min_radius": 0.5,
			"growth_step_length": 1.0,
			"energy_per_step": 0.8
		},
		"split":
		{
			"split_chance_base": 0.08,
			"split_count": 2,
			"split_energy_ratio": 0.2,
			"split_angle": 0.6,
			"split_angle_variance": 0.2,
			"min_progress_for_split": 0.05
		},
		"curvature": {"gravitropism": 0.15, "random_deflection": 0.05, "deflection_coherence": 0.8},
		"roots":
		{
			"root_energy_fraction": 0.1,
			"root_initial_count": 3,
			"root_gravitropism": 0.2,
			"root_initial_angle": 0.5,
			"root_surface_tendency": 0.5
		},
		"leaves":
		{"leaf_shape": "Sphere", "leaf_density": 0.5, "leaf_size": 2, "canopy_density": 0.8},
		"trunk": {"base_flare": 0.2, "initial_direction": [0.0, 1.0, 0.0]},
	},
	"Willow":
	{
		"growth":
		{
			"initial_energy": 200.0,
			"energy_to_radius": 0.07,
			"min_radius": 0.5,
			"growth_step_length": 1.0,
			"energy_per_step": 1.0
		},
		"split":
		{
			"split_chance_base": 0.15,
			"split_count": 2,
			"split_energy_ratio": 0.3,
			"split_angle": 0.5,
			"split_angle_variance": 0.3,
			"min_progress_for_split": 0.1
		},
		"curvature": {"gravitropism": -0.1, "random_deflection": 0.1, "deflection_coherence": 0.9},
		"roots":
		{
			"root_energy_fraction": 0.1,
			"root_initial_count": 4,
			"root_gravitropism": 0.1,
			"root_initial_angle": 0.3,
			"root_surface_tendency": 0.7
		},
		"leaves":
		{"leaf_shape": "Sphere", "leaf_density": 0.4, "leaf_size": 2, "canopy_density": 1.2},
		"trunk": {"base_flare": 0.15, "initial_direction": [0.0, 1.0, 0.0]},
	},
}

var _seed_input: LineEdit
var _preset_button: OptionButton
var _name_input: LineEdit
var _password_input: LineEdit
var _port_input: LineEdit
var _max_players_input: LineEdit
var _active_preset_data: Dictionary = {}


func _ready() -> void:
	# Full-rect dark background.
	var bg := ColorRect.new()
	bg.color = Color(0.12, 0.14, 0.10, 1.0)
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg)

	# ScrollContainer.
	var scroll := ScrollContainer.new()
	scroll.set_anchors_preset(Control.PRESET_FULL_RECT)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	add_child(scroll)

	var center := CenterContainer.new()
	center.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	center.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.add_child(center)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 10)
	center.add_child(vbox)

	# Header.
	var header := Label.new()
	header.text = "Host Game"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 36)
	vbox.add_child(header)

	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 8)
	vbox.add_child(spacer)

	# --- Session config ---
	_add_label(vbox, "Session Name:")
	_name_input = _add_line_edit(vbox, "my-session")
	_name_input.text = "elven-canopy-session"

	_add_label(vbox, "Password (blank = none):")
	_password_input = _add_line_edit(vbox, "")

	_add_label(vbox, "Port:")
	_port_input = _add_line_edit(vbox, "7878")
	_port_input.text = "7878"

	_add_label(vbox, "Max Players:")
	_max_players_input = _add_line_edit(vbox, "4")
	_max_players_input.text = "4"

	var spacer2 := Control.new()
	spacer2.custom_minimum_size = Vector2(0, 8)
	vbox.add_child(spacer2)

	# --- Tree config ---
	_add_label(vbox, "Seed (blank = random):")
	_seed_input = _add_line_edit(vbox, "e.g. 42")

	_add_label(vbox, "Tree Type:")
	_preset_button = OptionButton.new()
	_preset_button.custom_minimum_size = Vector2(300, 36)
	for preset_name in PRESET_NAMES:
		_preset_button.add_item(preset_name)
	_preset_button.selected = 0
	vbox.add_child(_preset_button)

	# Initialize preset data.
	_active_preset_data = _deep_copy_dict(PRESETS["Fantasy Mega"])

	var spacer3 := Control.new()
	spacer3.custom_minimum_size = Vector2(0, 12)
	vbox.add_child(spacer3)

	# Button row.
	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 16)
	hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	vbox.add_child(hbox)

	var back_btn := Button.new()
	back_btn.text = "Back"
	back_btn.custom_minimum_size = Vector2(120, 45)
	back_btn.pressed.connect(_on_back_pressed)
	hbox.add_child(back_btn)

	var host_btn := Button.new()
	host_btn.text = "Host"
	host_btn.custom_minimum_size = Vector2(180, 45)
	host_btn.pressed.connect(_on_host_pressed)
	hbox.add_child(host_btn)


func _add_label(parent: VBoxContainer, text: String) -> void:
	var lbl := Label.new()
	lbl.text = text
	parent.add_child(lbl)


func _add_line_edit(parent: VBoxContainer, placeholder: String) -> LineEdit:
	var edit := LineEdit.new()
	edit.placeholder_text = placeholder
	edit.custom_minimum_size = Vector2(300, 36)
	parent.add_child(edit)
	return edit


func _on_back_pressed() -> void:
	get_tree().change_scene_to_file("res://scenes/multiplayer_menu.tscn")


func _on_host_pressed() -> void:
	# Parse seed.
	var seed_text := _seed_input.text.strip_edges()
	var seed_value: int
	if seed_text.is_empty() or not seed_text.is_valid_int():
		seed_value = Time.get_ticks_usec()
	else:
		seed_value = seed_text.to_int()

	# Parse port.
	var port_text := _port_input.text.strip_edges()
	var port_value: int = 7878
	if port_text.is_valid_int():
		port_value = port_text.to_int()

	# Parse max players.
	var max_text := _max_players_input.text.strip_edges()
	var max_value: int = 4
	if max_text.is_valid_int():
		max_value = max_text.to_int()

	# Get the active preset.
	var preset_name: String = PRESET_NAMES[_preset_button.selected]
	_active_preset_data = _deep_copy_dict(PRESETS[preset_name])

	# Write to GameSession.
	GameSession.multiplayer_mode = "host"
	GameSession.sim_seed = seed_value
	GameSession.tree_profile = _active_preset_data
	GameSession.mp_port = port_value
	GameSession.mp_session_name = _name_input.text.strip_edges()
	GameSession.mp_password = _password_input.text
	GameSession.mp_max_players = max_value

	get_tree().change_scene_to_file("res://scenes/main.tscn")


func _deep_copy_dict(src: Dictionary) -> Dictionary:
	var copy := {}
	for key in src:
		var val = src[key]
		if val is Dictionary:
			copy[key] = _deep_copy_dict(val)
		elif val is Array:
			copy[key] = val.duplicate()
		else:
			copy[key] = val
	return copy
