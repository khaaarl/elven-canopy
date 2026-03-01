## Full-screen structure list overlay showing all completed structures.
##
## Displays a scrollable list of structure cards, each showing a sequential ID
## (e.g. "#0"), the build type (Platform, Bridge, etc.), and a "Zoom" button
## that emits zoom_to_structure to move the camera to the structure's anchor.
## The panel does NOT pause the sim (unlike pause_menu.gd).
##
## Data flow: main.gd calls update_structures(data) each frame while the panel
## is visible, passing the result of bridge.get_structures(). The panel uses a
## reconciliation pattern — it maintains a dictionary mapping structure id to
## card nodes, creating/updating/removing cards as structures appear/disappear.
##
## Signals:
## - zoom_to_structure(structure_id, x, y, z) — zoom camera to the structure's
##   anchor and select it (structure_id used by main.gd to open the info panel)
## - panel_closed — emitted when the panel is hidden (ESC or close button)
##
## ESC handling: when visible, consumes ESC in _unhandled_input and closes.
## This sits in the ESC precedence chain between task_panel and pause_menu
## (see main.gd docstring).
##
## See also: main.gd (creates and wires this panel), sim_bridge.rs for
## get_structures(), action_toolbar.gd for the "Structures" button,
## task_panel.gd for the similar task list pattern.

extends ColorRect

signal zoom_to_structure(structure_id: int, x: float, y: float, z: float)
signal panel_closed

## Maps structure id (int) -> PanelContainer card node.
var _structure_rows: Dictionary = {}
var _structure_list: VBoxContainer
var _empty_label: Label


func _ready() -> void:
	# Full-screen semi-transparent overlay.
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	color = Color(0.10, 0.12, 0.08, 0.90)

	var margin := MarginContainer.new()
	margin.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	margin.add_theme_constant_override("margin_left", 60)
	margin.add_theme_constant_override("margin_right", 60)
	margin.add_theme_constant_override("margin_top", 40)
	margin.add_theme_constant_override("margin_bottom", 40)
	add_child(margin)

	var outer_vbox := VBoxContainer.new()
	outer_vbox.add_theme_constant_override("separation", 12)
	margin.add_child(outer_vbox)

	# Header row.
	var header_hbox := HBoxContainer.new()
	outer_vbox.add_child(header_hbox)

	var title := Label.new()
	title.text = "Structures"
	title.add_theme_font_size_override("font_size", 28)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header_hbox.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.custom_minimum_size = Vector2(40, 40)
	close_btn.pressed.connect(hide_panel)
	header_hbox.add_child(close_btn)

	var sep := HSeparator.new()
	outer_vbox.add_child(sep)

	# Empty-state label (shown when no structures exist).
	_empty_label = Label.new()
	_empty_label.text = "No completed structures."
	_empty_label.add_theme_font_size_override("font_size", 18)
	_empty_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	outer_vbox.add_child(_empty_label)

	# Scrollable structure list.
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	outer_vbox.add_child(scroll)

	_structure_list = VBoxContainer.new()
	_structure_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_structure_list.add_theme_constant_override("separation", 8)
	scroll.add_child(_structure_list)

	# Start hidden.
	visible = false


func show_panel() -> void:
	visible = true


func hide_panel() -> void:
	visible = false
	panel_closed.emit()


func toggle() -> void:
	if visible:
		hide_panel()
	else:
		show_panel()


func _unhandled_input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey:
		var key := event as InputEventKey
		if key.pressed and key.keycode == KEY_ESCAPE:
			hide_panel()
			get_viewport().set_input_as_handled()


## Called each frame by main.gd with the result of bridge.get_structures().
## Uses reconciliation: creates new cards, updates existing ones, removes stale.
func update_structures(data: Array) -> void:
	# Collect ids present in this frame's data.
	var seen_ids: Dictionary = {}
	for i in data.size():
		var structure: Dictionary = data[i]
		var sid: int = structure.get("id", -1)
		seen_ids[sid] = true

		if _structure_rows.has(sid):
			_update_card(_structure_rows[sid], structure)
		else:
			var card := _create_card(structure)
			_structure_list.add_child(card)
			_structure_rows[sid] = card

	# Remove cards for structures no longer in data.
	var to_remove: Array = []
	for sid in _structure_rows:
		if not seen_ids.has(sid):
			to_remove.append(sid)
	for sid in to_remove:
		var card: PanelContainer = _structure_rows[sid]
		_structure_list.remove_child(card)
		card.queue_free()
		_structure_rows.erase(sid)

	# Show/hide empty label.
	_empty_label.visible = _structure_rows.is_empty()


func _create_card(structure: Dictionary) -> PanelContainer:
	var card := PanelContainer.new()

	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 12)
	card.add_child(hbox)

	# ID label (e.g. "#0").
	var id_label := Label.new()
	id_label.name = "IdLabel"
	id_label.custom_minimum_size = Vector2(60, 0)
	hbox.add_child(id_label)

	# Build type label.
	var type_label := Label.new()
	type_label.name = "TypeLabel"
	type_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	hbox.add_child(type_label)

	# Zoom button.
	var zoom_btn := Button.new()
	zoom_btn.name = "ZoomBtn"
	zoom_btn.text = "Zoom"
	var sid: int = structure.get("id", -1)
	zoom_btn.pressed.connect(
		func():
			var ax: float = structure.get("anchor_x", 0)
			var ay: float = structure.get("anchor_y", 0)
			var az: float = structure.get("anchor_z", 0)
			zoom_to_structure.emit(sid, ax, ay, az)
	)
	hbox.add_child(zoom_btn)

	_update_card(card, structure)
	return card


func _update_card(card: PanelContainer, structure: Dictionary) -> void:
	var sid: int = structure.get("id", 0)
	var build_type: String = structure.get("build_type", "?")

	var id_label: Label = card.find_child("IdLabel", true, false)
	if id_label:
		id_label.text = "#%d" % sid

	var type_label: Label = card.find_child("TypeLabel", true, false)
	if type_label:
		type_label.text = build_type
