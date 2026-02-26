## New Game screen — lets the player configure a new game before starting.
##
## Builds a centered vertical layout with a seed input field, a tree type
## preset dropdown, 6 tree shape sliders, and Start/Back buttons. When the
## player clicks "Start Game":
## 1. If the seed field is blank or non-numeric, generate a random seed.
## 2. Build a full TreeProfile dictionary from the active preset + slider values.
## 3. Write seed and tree_profile to GameSession (autoload singleton).
## 4. Transition to the game scene via change_scene_to_file().
##
## The 6 sliders each control one player-visible tree shape parameter. Selecting
## a preset updates all sliders to that preset's values. The underlying preset
## dictionary contains the FULL TreeProfile structure (matching the Rust serde
## JSON schema in config.rs), so fields not exposed as sliders are still passed
## through correctly.
##
## See also: main_menu.gd (previous screen), game_session.gd (stores seed +
## tree_profile), main.gd (reads them and passes to SimBridge),
## sim_bridge.rs (init_sim_with_tree_profile_json), config.rs (TreeProfile).

extends Control

var _seed_input: LineEdit
var _preset_button: OptionButton
var _sliders: Dictionary = {}  # slider_name → HSlider
var _value_labels: Dictionary = {}  # slider_name → Label
var _active_preset_data: Dictionary = {}  # full TreeProfile dict for active preset

# Preset names in display order (index must match OptionButton item indices).
const PRESET_NAMES: Array[String] = ["Fantasy Mega", "Oak", "Conifer", "Willow"]

# Slider definitions: [label, preset_field_path, min, max, step].
# preset_field_path is used to read/write the value in the active preset dict.
const SLIDER_DEFS: Array[Array] = [
	["Tree Size", "growth.initial_energy", 100.0, 600.0, 10.0],
	["Branchiness", "split.split_chance_base", 0.02, 0.30, 0.01],
	["Branch Spread", "split.split_angle", 0.2, 1.4, 0.05],
	["Growth Direction", "curvature.gravitropism", -0.2, 0.3, 0.01],
	["Canopy Density", "leaves.canopy_density", 0.0, 1.5, 0.05],
	["Trunk Thickness", "growth.energy_to_radius", 0.03, 0.15, 0.005],
]

# Full TreeProfile dictionaries matching the Rust serde JSON schema.
# These are the 4 named presets from config.rs.
const PRESETS: Dictionary = {
	"Fantasy Mega": {
		"growth": {
			"initial_energy": 400.0,
			"energy_to_radius": 0.08,
			"min_radius": 0.5,
			"growth_step_length": 1.0,
			"energy_per_step": 1.0,
		},
		"split": {
			"split_chance_base": 0.12,
			"split_count": 1,
			"split_energy_ratio": 0.35,
			"split_angle": 0.7,
			"split_angle_variance": 0.3,
			"min_progress_for_split": 0.15,
		},
		"curvature": {
			"gravitropism": 0.08,
			"random_deflection": 0.15,
			"deflection_coherence": 0.7,
		},
		"roots": {
			"root_energy_fraction": 0.15,
			"root_initial_count": 5,
			"root_gravitropism": 0.12,
			"root_initial_angle": 0.3,
			"root_surface_tendency": 0.8,
		},
		"leaves": {
			"leaf_shape": "Sphere",
			"leaf_density": 0.65,
			"leaf_size": 3,
			"canopy_density": 1.0,
		},
		"trunk": {
			"base_flare": 0.5,
			"initial_direction": [0.0, 1.0, 0.0],
		},
	},
	"Oak": {
		"growth": {
			"initial_energy": 250.0,
			"energy_to_radius": 0.1,
			"min_radius": 0.5,
			"growth_step_length": 1.0,
			"energy_per_step": 1.2,
		},
		"split": {
			"split_chance_base": 0.18,
			"split_count": 1,
			"split_energy_ratio": 0.4,
			"split_angle": 0.9,
			"split_angle_variance": 0.4,
			"min_progress_for_split": 0.1,
		},
		"curvature": {
			"gravitropism": 0.04,
			"random_deflection": 0.2,
			"deflection_coherence": 0.6,
		},
		"roots": {
			"root_energy_fraction": 0.12,
			"root_initial_count": 4,
			"root_gravitropism": 0.15,
			"root_initial_angle": 0.2,
			"root_surface_tendency": 0.9,
		},
		"leaves": {
			"leaf_shape": "Cloud",
			"leaf_density": 0.7,
			"leaf_size": 3,
			"canopy_density": 1.0,
		},
		"trunk": {
			"base_flare": 0.3,
			"initial_direction": [0.0, 1.0, 0.0],
		},
	},
	"Conifer": {
		"growth": {
			"initial_energy": 300.0,
			"energy_to_radius": 0.06,
			"min_radius": 0.5,
			"growth_step_length": 1.0,
			"energy_per_step": 0.8,
		},
		"split": {
			"split_chance_base": 0.08,
			"split_count": 2,
			"split_energy_ratio": 0.2,
			"split_angle": 0.6,
			"split_angle_variance": 0.2,
			"min_progress_for_split": 0.05,
		},
		"curvature": {
			"gravitropism": 0.15,
			"random_deflection": 0.05,
			"deflection_coherence": 0.8,
		},
		"roots": {
			"root_energy_fraction": 0.1,
			"root_initial_count": 3,
			"root_gravitropism": 0.2,
			"root_initial_angle": 0.5,
			"root_surface_tendency": 0.5,
		},
		"leaves": {
			"leaf_shape": "Sphere",
			"leaf_density": 0.5,
			"leaf_size": 2,
			"canopy_density": 0.8,
		},
		"trunk": {
			"base_flare": 0.2,
			"initial_direction": [0.0, 1.0, 0.0],
		},
	},
	"Willow": {
		"growth": {
			"initial_energy": 200.0,
			"energy_to_radius": 0.07,
			"min_radius": 0.5,
			"growth_step_length": 1.0,
			"energy_per_step": 1.0,
		},
		"split": {
			"split_chance_base": 0.15,
			"split_count": 2,
			"split_energy_ratio": 0.3,
			"split_angle": 0.5,
			"split_angle_variance": 0.3,
			"min_progress_for_split": 0.1,
		},
		"curvature": {
			"gravitropism": -0.1,
			"random_deflection": 0.1,
			"deflection_coherence": 0.9,
		},
		"roots": {
			"root_energy_fraction": 0.1,
			"root_initial_count": 4,
			"root_gravitropism": 0.1,
			"root_initial_angle": 0.3,
			"root_surface_tendency": 0.7,
		},
		"leaves": {
			"leaf_shape": "Sphere",
			"leaf_density": 0.4,
			"leaf_size": 2,
			"canopy_density": 1.2,
		},
		"trunk": {
			"base_flare": 0.15,
			"initial_direction": [0.0, 1.0, 0.0],
		},
	},
}


func _ready() -> void:
	# Full-rect dark background.
	var bg := ColorRect.new()
	bg.color = Color(0.12, 0.14, 0.10, 1.0)
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg)

	# ScrollContainer so the UI doesn't overflow on small windows.
	var scroll := ScrollContainer.new()
	scroll.set_anchors_preset(Control.PRESET_FULL_RECT)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	add_child(scroll)

	# Center container.
	var center := CenterContainer.new()
	center.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	center.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.add_child(center)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 12)
	center.add_child(vbox)

	# Header.
	var header := Label.new()
	header.text = "New Game"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 36)
	vbox.add_child(header)

	# Spacer.
	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 12)
	vbox.add_child(spacer)

	# Seed label + input.
	var seed_label := Label.new()
	seed_label.text = "Seed (blank = random):"
	vbox.add_child(seed_label)

	_seed_input = LineEdit.new()
	_seed_input.placeholder_text = "e.g. 42"
	_seed_input.custom_minimum_size = Vector2(300, 40)
	vbox.add_child(_seed_input)
	# Defer focus grab — the control may not be ready for focus during _ready().
	_seed_input.call_deferred("grab_focus")

	# Spacer.
	var spacer2 := Control.new()
	spacer2.custom_minimum_size = Vector2(0, 12)
	vbox.add_child(spacer2)

	# --- Tree Type preset dropdown ---
	var preset_label := Label.new()
	preset_label.text = "Tree Type:"
	vbox.add_child(preset_label)

	_preset_button = OptionButton.new()
	_preset_button.custom_minimum_size = Vector2(300, 36)
	for preset_name in PRESET_NAMES:
		_preset_button.add_item(preset_name)
	_preset_button.selected = 0
	_preset_button.item_selected.connect(_on_preset_selected)
	vbox.add_child(_preset_button)

	# Spacer.
	var spacer3 := Control.new()
	spacer3.custom_minimum_size = Vector2(0, 8)
	vbox.add_child(spacer3)

	# --- Tree shape sliders ---
	for slider_def in SLIDER_DEFS:
		var slider_label: String = slider_def[0]
		var slider_min: float = slider_def[2]
		var slider_max: float = slider_def[3]
		var slider_step: float = slider_def[4]

		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 8)
		vbox.add_child(row)

		var lbl := Label.new()
		lbl.text = slider_label + ":"
		lbl.custom_minimum_size = Vector2(130, 0)
		row.add_child(lbl)

		var slider := HSlider.new()
		slider.min_value = slider_min
		slider.max_value = slider_max
		slider.step = slider_step
		slider.custom_minimum_size = Vector2(200, 20)
		slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		row.add_child(slider)

		var val_lbl := Label.new()
		val_lbl.custom_minimum_size = Vector2(50, 0)
		val_lbl.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
		row.add_child(val_lbl)

		_sliders[slider_label] = slider
		_value_labels[slider_label] = val_lbl

		slider.value_changed.connect(_on_slider_changed.bind(slider_label))

	# Initialize with first preset.
	_apply_preset(0)

	# Spacer.
	var spacer4 := Control.new()
	spacer4.custom_minimum_size = Vector2(0, 16)
	vbox.add_child(spacer4)

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

	var start_btn := Button.new()
	start_btn.text = "Start Game"
	start_btn.custom_minimum_size = Vector2(180, 45)
	start_btn.pressed.connect(_on_start_pressed)
	hbox.add_child(start_btn)


## Apply a preset by index — update all sliders and the active preset data.
func _apply_preset(index: int) -> void:
	var preset_name: String = PRESET_NAMES[index]
	# Deep-copy the preset so slider tweaks don't mutate the const.
	_active_preset_data = _deep_copy_dict(PRESETS[preset_name])
	# Set each slider to the preset's value (this triggers _on_slider_changed
	# which updates the value labels).
	for slider_def in SLIDER_DEFS:
		var slider_label: String = slider_def[0]
		var field_path: String = slider_def[1]
		var value: float = _get_nested(_active_preset_data, field_path)
		_sliders[slider_label].value = value


## Called when the preset dropdown selection changes.
func _on_preset_selected(index: int) -> void:
	_apply_preset(index)


## Called when any slider value changes — update its value label and the
## active preset data so the final TreeProfile reflects the tweak.
func _on_slider_changed(value: float, slider_label: String) -> void:
	# Find the field path for this slider.
	for slider_def in SLIDER_DEFS:
		if slider_def[0] == slider_label:
			var field_path: String = slider_def[1]
			_set_nested(_active_preset_data, field_path, value)
			break
	# Update value label with appropriate precision.
	var display: String
	if slider_label == "Tree Size":
		display = "%d" % int(value)
	else:
		display = "%.3f" % value
	_value_labels[slider_label].text = display


func _on_back_pressed() -> void:
	get_tree().change_scene_to_file("res://scenes/main_menu.tscn")


func _on_start_pressed() -> void:
	var seed_text := _seed_input.text.strip_edges()
	var seed_value: int

	if seed_text.is_empty() or not seed_text.is_valid_int():
		# Generate a random seed from system time. Godot's global randi() uses
		# a fixed default seed (same sequence every launch), so we derive a
		# seed from Time.get_ticks_usec() instead for actual randomness.
		seed_value = Time.get_ticks_usec()
	else:
		seed_value = seed_text.to_int()

	GameSession.sim_seed = seed_value
	GameSession.tree_profile = _active_preset_data
	get_tree().change_scene_to_file("res://scenes/main.tscn")


# --- Utility: nested dictionary access via "group.field" paths ---

## Read a value from a nested dictionary using a dot-separated path.
func _get_nested(dict: Dictionary, path: String) -> Variant:
	var parts := path.split(".")
	var current: Variant = dict
	for part in parts:
		current = current[part]
	return current


## Write a value into a nested dictionary using a dot-separated path.
func _set_nested(dict: Dictionary, path: String, value: Variant) -> void:
	var parts := path.split(".")
	var current: Variant = dict
	for i in range(parts.size() - 1):
		current = current[parts[i]]
	current[parts[parts.size() - 1]] = value


## Deep-copy a Dictionary (one level of nesting — sub-dicts are duplicated).
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
