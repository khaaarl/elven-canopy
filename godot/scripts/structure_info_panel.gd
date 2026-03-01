## Structure info panel displayed on the right side of the screen.
##
## Shows information about the currently selected structure. Built
## programmatically as a PanelContainer with an editable name LineEdit,
## type/ID labels, dimensions, position, a Zoom button, and furnishing
## controls. Sits on the CanvasLayer alongside the creature info panel.
##
## The editable name LineEdit shows the structure's display name (custom or
## auto-generated). On Enter or focus loss, emits rename_requested so main.gd
## can call bridge.rename_structure(). Clearing the field resets to the
## auto-generated default.
##
## For Building-type structures without a furnishing, a "Furnish" button is
## shown. Clicking it opens a sub-panel with furnishing type buttons (Concert
## Hall, Dining Hall, Dormitory, Home, Kitchen, Storehouse, Workshop). For
## buildings with a furnishing in progress, shows progress like "Furnishing:
## Dormitory (3/8 beds)". For fully furnished buildings, shows "Dormitory
## (8 beds)". The furniture noun is returned by the bridge per furnishing type.
##
## The panel is ~25% screen width, full height, anchored to the right edge.
## Updated every frame by main.gd while visible.
##
## See also: selection_controller.gd which triggers show/hide,
## creature_info_panel.gd for the creature equivalent,
## main.gd which wires everything together,
## sim_bridge.rs for rename_structure() and furnish_structure().

extends PanelContainer

signal zoom_requested(x: float, y: float, z: float)
signal panel_closed
signal rename_requested(structure_id: int, new_name: String)
signal furnish_requested(structure_id: int, furnishing_type: String)

var _name_edit: LineEdit
var _type_label: Label
var _id_label: Label
var _dimensions_label: Label
var _position_label: Label
var _furnish_label: Label
var _furnish_button: Button
var _furnish_picker: VBoxContainer
var _zoom_button: Button
var _anchor_x: float = 0.0
var _anchor_y: float = 0.0
var _anchor_z: float = 0.0
var _current_structure_id: int = -1
## Tracks whether the user is actively editing the name field. While true,
## _update_info() skips overwriting the LineEdit text so the user's in-progress
## edits aren't clobbered by per-frame refreshes.
var _editing_name: bool = false


func _ready() -> void:
	# Anchor to the right edge, full height.
	set_anchors_preset(PRESET_RIGHT_WIDE)
	custom_minimum_size.x = 320

	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	margin.add_theme_constant_override("margin_right", 12)
	margin.add_theme_constant_override("margin_top", 12)
	margin.add_theme_constant_override("margin_bottom", 12)
	add_child(margin)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 8)
	margin.add_child(vbox)

	# Header with title and close button.
	var header := HBoxContainer.new()
	vbox.add_child(header)

	var title := Label.new()
	title.text = "Structure Info"
	title.add_theme_font_size_override("font_size", 20)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close_pressed)
	header.add_child(close_btn)

	# Separator.
	vbox.add_child(HSeparator.new())

	# Editable name.
	_name_edit = LineEdit.new()
	_name_edit.placeholder_text = "Structure name..."
	_name_edit.add_theme_font_size_override("font_size", 16)
	_name_edit.text_submitted.connect(_on_name_submitted)
	_name_edit.focus_entered.connect(func(): _editing_name = true)
	_name_edit.focus_exited.connect(_on_name_focus_exited)
	vbox.add_child(_name_edit)

	# Type.
	_type_label = Label.new()
	vbox.add_child(_type_label)

	# ID.
	_id_label = Label.new()
	vbox.add_child(_id_label)

	# Dimensions.
	_dimensions_label = Label.new()
	vbox.add_child(_dimensions_label)

	# Position.
	_position_label = Label.new()
	vbox.add_child(_position_label)

	# Furnishing status label (visible when furnishing is set).
	_furnish_label = Label.new()
	vbox.add_child(_furnish_label)

	# Furnish button (visible for unfurnished buildings).
	_furnish_button = Button.new()
	_furnish_button.text = "Furnish..."
	_furnish_button.pressed.connect(_on_furnish_pressed)
	vbox.add_child(_furnish_button)

	# Furnishing type picker (hidden by default, shown when Furnish is clicked).
	_furnish_picker = VBoxContainer.new()
	_furnish_picker.visible = false
	vbox.add_child(_furnish_picker)

	var furnishing_types := [
		["Concert Hall", "ConcertHall"],
		["Dining Hall", "DiningHall"],
		["Dormitory", "Dormitory"],
		["Home", "Home"],
		["Kitchen", "Kitchen"],
		["Storehouse", "Storehouse"],
		["Workshop", "Workshop"],
	]
	for entry in furnishing_types:
		var btn := Button.new()
		btn.text = entry[0]
		var type_id: String = entry[1]
		btn.pressed.connect(_on_furnishing_type_pressed.bind(type_id))
		_furnish_picker.add_child(btn)

	# Spacer to push the zoom button toward the bottom-ish area.
	var spacer := Control.new()
	spacer.size_flags_vertical = Control.SIZE_EXPAND_FILL
	vbox.add_child(spacer)

	# Zoom button.
	_zoom_button = Button.new()
	_zoom_button.text = "Zoom"
	_zoom_button.pressed.connect(_on_zoom_pressed)
	vbox.add_child(_zoom_button)

	visible = false


func show_structure(info: Dictionary) -> void:
	_editing_name = false
	_furnish_picker.visible = false
	_update_info(info)
	visible = true


func update_info(info: Dictionary) -> void:
	_update_info(info)


func hide_panel() -> void:
	_editing_name = false
	if _name_edit.has_focus():
		_name_edit.release_focus()
	visible = false


func _update_info(info: Dictionary) -> void:
	var build_type: String = info.get("build_type", "?")
	var sid: int = info.get("id", 0)
	var display_name: String = info.get("name", "")
	var w: int = info.get("width", 0)
	var d: int = info.get("depth", 0)
	var h: int = info.get("height", 0)
	_anchor_x = info.get("anchor_x", 0)
	_anchor_y = info.get("anchor_y", 0)
	_anchor_z = info.get("anchor_z", 0)
	_current_structure_id = sid

	if not _editing_name:
		_name_edit.text = display_name
	_type_label.text = "Type: %s" % build_type
	_id_label.text = "ID: #%d" % sid
	_dimensions_label.text = "Dimensions: %d x %d x %d" % [w, d, h]
	_position_label.text = (
		"Position: (%d, %d, %d)" % [int(_anchor_x), int(_anchor_y), int(_anchor_z)]
	)

	# Furnishing state.
	var furnishing: String = info.get("furnishing", "")
	var furniture_noun: String = info.get("furniture_noun", "items")
	var furniture_count: int = info.get("furniture_count", 0)
	var planned_furniture_count: int = info.get("planned_furniture_count", 0)
	var is_furnishing: bool = info.get("is_furnishing", false)

	if furnishing != "":
		if is_furnishing:
			_furnish_label.text = (
				"Furnishing: %s (%d/%d %s)"
				% [furnishing, furniture_count, planned_furniture_count, furniture_noun]
			)
		else:
			_furnish_label.text = "%s (%d %s)" % [furnishing, furniture_count, furniture_noun]
		_furnish_label.visible = true
		_furnish_button.visible = false
		_furnish_picker.visible = false
	elif build_type == "Building":
		_furnish_label.visible = false
		_furnish_button.visible = true
		# Don't touch _furnish_picker.visible here — it's toggled by
		# _on_furnish_pressed() and must survive per-frame refreshes.
		# It's reset to hidden in show_structure() when a new structure
		# is selected.
	else:
		_furnish_label.visible = false
		_furnish_button.visible = false
		_furnish_picker.visible = false


func _on_name_submitted(new_text: String) -> void:
	_editing_name = false
	_name_edit.release_focus()
	if _current_structure_id >= 0:
		rename_requested.emit(_current_structure_id, new_text)


func _on_name_focus_exited() -> void:
	if not _editing_name:
		return
	_editing_name = false
	if _current_structure_id >= 0:
		rename_requested.emit(_current_structure_id, _name_edit.text)


func _on_zoom_pressed() -> void:
	zoom_requested.emit(_anchor_x, _anchor_y, _anchor_z)


func _on_furnish_pressed() -> void:
	_furnish_picker.visible = not _furnish_picker.visible


func _on_furnishing_type_pressed(type_id: String) -> void:
	_furnish_picker.visible = false
	if _current_structure_id >= 0:
		furnish_requested.emit(_current_structure_id, type_id)


func _on_close_pressed() -> void:
	hide_panel()
	panel_closed.emit()
