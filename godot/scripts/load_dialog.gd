## Modal load-game dialog.
##
## A semi-transparent full-screen overlay listing save files from
## `user://saves/` with Load, Delete, and Cancel buttons. Emits
## `load_requested(save_path)` when the player selects a save and clicks Load.
##
## Created dynamically by main_menu.gd when the Load button is pressed.
##
## See also: main_menu.gd (creates this dialog), save_dialog.gd (counterpart
## for saving).

extends ColorRect

signal load_requested(save_path: String)

var _item_list: ItemList
var _load_btn: Button
var _delete_btn: Button
## Parallel array: _save_paths[i] is the full `user://saves/...` path for
## the item at index i in the ItemList.
var _save_paths: Array[String] = []


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
	panel.custom_minimum_size = Vector2(450, 350)
	center.add_child(panel)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 12)
	panel.add_child(vbox)

	# Header.
	var header := Label.new()
	header.text = "Load Game"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 24)
	vbox.add_child(header)

	# Save file list.
	_item_list = ItemList.new()
	_item_list.custom_minimum_size = Vector2(0, 200)
	_item_list.item_selected.connect(_on_item_selected)
	_item_list.item_activated.connect(func(_idx: int): _on_load_pressed())
	vbox.add_child(_item_list)

	# Button row.
	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 12)
	hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	vbox.add_child(hbox)

	_load_btn = Button.new()
	_load_btn.text = "Load"
	_load_btn.custom_minimum_size = Vector2(100, 40)
	_load_btn.disabled = true
	_load_btn.pressed.connect(_on_load_pressed)
	hbox.add_child(_load_btn)

	_delete_btn = Button.new()
	_delete_btn.text = "Delete"
	_delete_btn.custom_minimum_size = Vector2(100, 40)
	_delete_btn.disabled = true
	_delete_btn.pressed.connect(_on_delete_pressed)
	hbox.add_child(_delete_btn)

	var cancel_btn := Button.new()
	cancel_btn.text = "Cancel"
	cancel_btn.custom_minimum_size = Vector2(100, 40)
	cancel_btn.pressed.connect(func(): queue_free())
	hbox.add_child(cancel_btn)

	_refresh_list()


func _refresh_list() -> void:
	_item_list.clear()
	_save_paths.clear()
	_load_btn.disabled = true
	_delete_btn.disabled = true

	var dir := DirAccess.open("user://saves")
	if dir == null:
		return

	# Collect save files with modification times for sorting.
	var entries: Array[Dictionary] = []
	dir.list_dir_begin()
	var file_name := dir.get_next()
	while file_name != "":
		if not dir.current_is_dir() and file_name.ends_with(".json"):
			var full_path := "user://saves/" + file_name
			var mod_time := FileAccess.get_modified_time(full_path)
			entries.append({"name": file_name, "path": full_path, "time": mod_time})
		file_name = dir.get_next()
	dir.list_dir_end()

	# Sort newest first.
	entries.sort_custom(func(a: Dictionary, b: Dictionary) -> bool:
		return a["time"] > b["time"]
	)

	for entry in entries:
		var display_name: String = entry["name"].get_basename()
		_item_list.add_item(display_name)
		_save_paths.append(entry["path"])


func _on_item_selected(_index: int) -> void:
	_load_btn.disabled = false
	_delete_btn.disabled = false


func _on_load_pressed() -> void:
	var selected := _item_list.get_selected_items()
	if selected.is_empty():
		return
	var path := _save_paths[selected[0]]
	load_requested.emit(path)
	queue_free()


func _on_delete_pressed() -> void:
	var selected := _item_list.get_selected_items()
	if selected.is_empty():
		return
	var path := _save_paths[selected[0]]
	DirAccess.remove_absolute(path)
	_refresh_list()
