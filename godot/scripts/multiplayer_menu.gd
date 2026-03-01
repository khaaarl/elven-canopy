## Multiplayer mode selection screen — choose Host or Join.
##
## Simple two-button menu that branches to host_game.tscn or join_game.tscn.
## Hotkeys: H = Host Game, J = Join Game, B = Back.
##
## See also: main_menu.gd (previous screen), host_game_menu.gd, join_game_menu.gd.

extends Control


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
	vbox.add_theme_constant_override("separation", 20)
	center.add_child(vbox)

	# Header.
	var header := Label.new()
	header.text = "Multiplayer"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 36)
	vbox.add_child(header)

	# Spacer.
	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 20)
	vbox.add_child(spacer)

	# Host Game button.
	var host_btn := Button.new()
	host_btn.text = "Host Game"
	host_btn.custom_minimum_size = Vector2(200, 50)
	host_btn.pressed.connect(_on_host_pressed)
	vbox.add_child(host_btn)

	# Join Game button.
	var join_btn := Button.new()
	join_btn.text = "Join Game"
	join_btn.custom_minimum_size = Vector2(200, 50)
	join_btn.pressed.connect(_on_join_pressed)
	vbox.add_child(join_btn)

	# Back button.
	var back_btn := Button.new()
	back_btn.text = "Back"
	back_btn.custom_minimum_size = Vector2(200, 50)
	back_btn.pressed.connect(_on_back_pressed)
	vbox.add_child(back_btn)


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == KEY_H:
			get_viewport().set_input_as_handled()
			_on_host_pressed()
		elif event.keycode == KEY_J:
			get_viewport().set_input_as_handled()
			_on_join_pressed()
		elif event.keycode == KEY_B:
			get_viewport().set_input_as_handled()
			_on_back_pressed()


func _on_host_pressed() -> void:
	get_tree().change_scene_to_file("res://scenes/host_game.tscn")


func _on_join_pressed() -> void:
	get_tree().change_scene_to_file("res://scenes/join_game.tscn")


func _on_back_pressed() -> void:
	get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
