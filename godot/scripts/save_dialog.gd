## Modal save-game dialog.
##
## A semi-transparent full-screen overlay with a name input field (pre-filled
## with the current date/time) and Save/Cancel buttons. Emits
## `save_requested(save_name)` when the player confirms.
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
	_name_edit.text = "%04d-%02d-%02d %02d:%02d" % [
		now["year"], now["month"], now["day"], now["hour"], now["minute"]
	]
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


func _on_save_confirmed(save_name: String) -> void:
	var trimmed := save_name.strip_edges()
	if trimmed.is_empty():
		return
	save_requested.emit(trimmed)
	queue_free()
