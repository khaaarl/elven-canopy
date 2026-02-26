## New Game screen — lets the player configure a new game before starting.
##
## Builds a centered vertical layout with a seed input field, "Start Game"
## button, and "Back" button. When the player clicks "Start Game":
## 1. If the seed field is blank or non-numeric, generate a random seed.
## 2. Write the seed to GameSession.sim_seed (autoload singleton).
## 3. Transition to the game scene via change_scene_to_file().
##
## In the future, additional world-generation settings (world size, difficulty,
## biome, etc.) will be added to this screen.
##
## See also: main_menu.gd (previous screen), game_session.gd (stores the seed),
## main.gd (reads the seed and passes it to SimBridge.init_sim()).

extends Control

var _seed_input: LineEdit


func _ready() -> void:
	# Full-rect dark background.
	var bg := ColorRect.new()
	bg.color = Color(0.12, 0.14, 0.10, 1.0)
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg)

	# Center container.
	var center := CenterContainer.new()
	center.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(center)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 16)
	center.add_child(vbox)

	# Header.
	var header := Label.new()
	header.text = "New Game"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 36)
	vbox.add_child(header)

	# Spacer.
	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 20)
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
	spacer2.custom_minimum_size = Vector2(0, 20)
	vbox.add_child(spacer2)

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
	get_tree().change_scene_to_file("res://scenes/main.tscn")
