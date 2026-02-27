## Modal save-game dialog.
##
## A semi-transparent full-screen overlay with a name input field (pre-filled
## with the current date/time) and Save/Cancel buttons. Emits
## `save_requested(save_name)` when the player confirms.
##
## Keyboard: ESC cancels the dialog (caught via _input before it reaches
## pause_menu). An _unhandled_input catch-all blocks stray keys (not consumed
## by the LineEdit) from triggering pause_menu hotkeys.
##
## Created dynamically by pause_menu.gd when the Save button is pressed.
## The dialog runs in PROCESS_MODE_ALWAYS so it works while the tree is paused.
##
## See also: pause_menu.gd (creates this dialog), load_dialog.gd (counterpart
## for loading saves).

extends ColorRect

signal save_requested(save_name: String)

var _name_edit: LineEdit


func _ready() -> void:
	process_mode = Node.PROCESS_MODE_ALWAYS

	# Full-screen semi-transparent overlay.
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	color = Color(0.0, 0.0, 0.0, 0.6)

	# Centered panel.
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)

	var panel := PanelContainer.new()
	panel.custom_minimum_size = Vector2(400, 200)
	center.add_child(panel)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 16)
	panel.add_child(vbox)

	# Header.
	var header := Label.new()
	header.text = "Save Game"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 24)
	vbox.add_child(header)

	# Name input.
	_name_edit = LineEdit.new()
	var now := Time.get_datetime_dict_from_system()
	var msec := Time.get_ticks_msec() % 1000
	_name_edit.text = (
		"%04d-%02d-%02d %02d.%02d.%02d.%03d"
		% [now["year"], now["month"], now["day"], now["hour"], now["minute"], now["second"], msec]
	)
	_name_edit.select_all_on_focus = true
	vbox.add_child(_name_edit)
	_name_edit.text_submitted.connect(_on_save_confirmed)

	# Button row.
	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 12)
	hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	vbox.add_child(hbox)

	var save_btn := Button.new()
	save_btn.text = "Save"
	save_btn.custom_minimum_size = Vector2(120, 40)
	save_btn.pressed.connect(func(): _on_save_confirmed(_name_edit.text))
	hbox.add_child(save_btn)

	var cancel_btn := Button.new()
	cancel_btn.text = "Cancel"
	cancel_btn.custom_minimum_size = Vector2(120, 40)
	cancel_btn.pressed.connect(func(): queue_free())
	hbox.add_child(cancel_btn)

	# Focus the name field.
	_name_edit.call_deferred("grab_focus")


func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		queue_free()
		get_viewport().set_input_as_handled()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey:
		get_viewport().set_input_as_handled()


func _on_save_confirmed(save_name: String) -> void:
	var trimmed := save_name.strip_edges()
	if trimmed.is_empty():
		return
	save_requested.emit(trimmed)
	queue_free()
