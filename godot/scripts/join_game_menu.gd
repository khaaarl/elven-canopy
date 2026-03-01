## Join Game screen — enter address, password, and player name to join a session.
##
## Writes connection info to GameSession with `multiplayer_mode = "join"`,
## then transitions to main.tscn where the SimBridge will connect to the relay.
##
## See also: multiplayer_menu.gd (previous screen), game_session.gd, main.gd.

extends Control

var _address_input: LineEdit
var _password_input: LineEdit
var _name_input: LineEdit


func _ready() -> void:
	# Full-rect dark background.
	var bg := ColorRect.new()
	bg.color = Color(0.12, 0.14, 0.10, 1.0)
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg)

	var center := CenterContainer.new()
	center.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(center)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 12)
	center.add_child(vbox)

	# Header.
	var header := Label.new()
	header.text = "Join Game"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 36)
	vbox.add_child(header)

	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 12)
	vbox.add_child(spacer)

	# Address input.
	_add_label(vbox, "Server Address (host:port):")
	_address_input = _add_line_edit(vbox, "127.0.0.1:7878")
	_address_input.text = "127.0.0.1:7878"
	_address_input.call_deferred("grab_focus")

	# Password input.
	_add_label(vbox, "Password (blank if none):")
	_password_input = _add_line_edit(vbox, "")

	# Player name input.
	_add_label(vbox, "Player Name:")
	_name_input = _add_line_edit(vbox, "Player")
	_name_input.text = "Player"

	var spacer2 := Control.new()
	spacer2.custom_minimum_size = Vector2(0, 16)
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

	var connect_btn := Button.new()
	connect_btn.text = "Connect"
	connect_btn.custom_minimum_size = Vector2(180, 45)
	connect_btn.pressed.connect(_on_connect_pressed)
	hbox.add_child(connect_btn)


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


func _on_connect_pressed() -> void:
	GameSession.multiplayer_mode = "join"
	GameSession.mp_relay_address = _address_input.text.strip_edges()
	GameSession.mp_password = _password_input.text
	GameSession.mp_player_name = _name_input.text.strip_edges()

	get_tree().change_scene_to_file("res://scenes/main.tscn")
