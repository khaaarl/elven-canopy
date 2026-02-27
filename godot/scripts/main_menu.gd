## Main menu screen — the first thing the player sees on launch.
##
## Builds a centered vertical layout with the game title, "New Game" button,
## "Load Game" button (enabled when saves exist), and "Quit Game" button.
## Transitions to the new-game screen or loads a save via GameSession.
##
## All UI elements are built programmatically in _ready(), consistent with
## the project's existing UI style (see spawn_toolbar.gd).
##
## See also: new_game_menu.gd (next screen in flow), game_session.gd (autoload
## singleton for passing seed/load path to the game scene), load_dialog.gd
## (modal save browser).

extends Control


func _ready() -> void:
	# Full-rect dark background.
	var bg := ColorRect.new()
	bg.color = Color(0.12, 0.14, 0.10, 1.0)
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg)

	# Center container for vertical layout.
	var center := CenterContainer.new()
	center.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(center)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 20)
	center.add_child(vbox)

	# Title label.
	var title := Label.new()
	title.text = "Elven Canopy"
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	title.add_theme_font_size_override("font_size", 48)
	vbox.add_child(title)

	# Spacer.
	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 40)
	vbox.add_child(spacer)

	# New Game button.
	var new_game_btn := Button.new()
	new_game_btn.text = "New Game"
	new_game_btn.custom_minimum_size = Vector2(200, 50)
	new_game_btn.pressed.connect(_on_new_game_pressed)
	vbox.add_child(new_game_btn)

	# Load Game button — enabled only when saves exist.
	var load_game_btn := Button.new()
	load_game_btn.text = "Load Game"
	load_game_btn.custom_minimum_size = Vector2(200, 50)
	load_game_btn.disabled = not _has_save_files()
	load_game_btn.pressed.connect(_on_load_game_pressed)
	vbox.add_child(load_game_btn)

	# Quit Game button.
	var quit_btn := Button.new()
	quit_btn.text = "Quit Game"
	quit_btn.custom_minimum_size = Vector2(200, 50)
	quit_btn.pressed.connect(func(): get_tree().quit())
	vbox.add_child(quit_btn)


func _has_save_files() -> bool:
	var dir := DirAccess.open("user://saves")
	if dir == null:
		return false
	dir.list_dir_begin()
	var file_name := dir.get_next()
	while file_name != "":
		if not dir.current_is_dir() and file_name.ends_with(".json"):
			dir.list_dir_end()
			return true
		file_name = dir.get_next()
	dir.list_dir_end()
	return false


func _on_new_game_pressed() -> void:
	get_tree().change_scene_to_file("res://scenes/new_game.tscn")


func _on_load_game_pressed() -> void:
	var dialog_script = load("res://scripts/load_dialog.gd")
	var dialog := ColorRect.new()
	dialog.set_script(dialog_script)
	add_child(dialog)
	dialog.load_requested.connect(_on_load_selected)


func _on_load_selected(save_path: String) -> void:
	GameSession.load_save_path = save_path
	get_tree().change_scene_to_file("res://scenes/main.tscn")
