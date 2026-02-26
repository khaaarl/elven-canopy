## In-game pause menu overlay.
##
## A semi-transparent full-screen overlay with Resume, Save (disabled),
## Main Menu, and Quit buttons. Uses Godot's built-in pause system:
## when shown, get_tree().paused = true freezes _process in main.gd
## (and all other default-mode nodes), stopping the sim. This node's
## process_mode is PROCESS_MODE_ALWAYS so it keeps receiving input.
##
## ESC key toggles the menu via _unhandled_input. When the menu is hidden,
## placement_controller.gd and selection_controller.gd consume ESC first
## (via set_input_as_handled), so the pause menu only opens when nothing
## else claims ESC.
##
## Exposes toggle(), open(), close() so main.gd can wire a menu button.
##
## See also: main.gd (creates and wires this menu), main_menu.gd (target
## of the "Main Menu" button).

extends ColorRect


func _ready() -> void:
	# Run even while the tree is paused.
	process_mode = Node.PROCESS_MODE_ALWAYS

	# Full-screen semi-transparent overlay.
	set_anchors_preset(Control.PRESET_FULL_RECT)
	color = Color(0.12, 0.14, 0.10, 0.85)

	# Centered button column.
	var center := CenterContainer.new()
	center.set_anchors_preset(Control.PRESET_FULL_RECT)
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

	# Save Game button (disabled placeholder).
	var save_btn := Button.new()
	save_btn.text = "Save Game"
	save_btn.custom_minimum_size = Vector2(200, 50)
	save_btn.disabled = true
	vbox.add_child(save_btn)

	# Main Menu button.
	var main_menu_btn := Button.new()
	main_menu_btn.text = "Main Menu"
	main_menu_btn.custom_minimum_size = Vector2(200, 50)
	main_menu_btn.pressed.connect(_on_main_menu_pressed)
	vbox.add_child(main_menu_btn)

	# Quit Game button.
	var quit_btn := Button.new()
	quit_btn.text = "Quit Game"
	quit_btn.custom_minimum_size = Vector2(200, 50)
	quit_btn.pressed.connect(func(): get_tree().quit())
	vbox.add_child(quit_btn)

	# Start hidden.
	visible = false


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("ui_cancel"):
		toggle()
		get_viewport().set_input_as_handled()


func toggle() -> void:
	if visible:
		close()
	else:
		open()


func open() -> void:
	visible = true
	get_tree().paused = true


func close() -> void:
	visible = false
	get_tree().paused = false


func _on_main_menu_pressed() -> void:
	get_tree().paused = false
	get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
