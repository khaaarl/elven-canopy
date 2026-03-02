## Structure info panel displayed on the right side of the screen.
##
## Shows information about the currently selected structure. Built
## programmatically as a PanelContainer with an editable name LineEdit,
## type/ID labels, dimensions, position, a Zoom button, furnishing
## controls, and an inventory section. Sits on the CanvasLayer alongside
## the creature info panel.
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
## For fully-furnished Home buildings, an elf assignment section is shown:
## "Assigned: ElfName" or "Unassigned", plus an "Assign Elf..." / "Unassign"
## button. The elf picker is a scrollable list of all elves with rest % and
## existing home indicators. Emits assign_elf_requested / unassign_elf_requested
## which main.gd wires to bridge.assign_home().
##
## The panel is ~25% screen width, full height, anchored to the right edge.
## Updated every frame by main.gd while visible.
##
## See also: selection_controller.gd which triggers show/hide,
## creature_info_panel.gd for the creature equivalent,
## main.gd which wires everything together,
## sim_bridge.rs for rename_structure(), furnish_structure(), assign_home().

extends PanelContainer

signal zoom_requested(x: float, y: float, z: float)
signal panel_closed
signal rename_requested(structure_id: int, new_name: String)
signal furnish_requested(structure_id: int, furnishing_type: String)
signal assign_elf_requested(structure_id: int, creature_id_str: String)
signal unassign_elf_requested(structure_id: int, creature_id_str: String)

var _name_edit: LineEdit
var _type_label: Label
var _id_label: Label
var _dimensions_label: Label
var _position_label: Label
var _furnish_label: Label
var _furnish_button: Button
var _furnish_picker: VBoxContainer
var _assign_section: VBoxContainer
var _assign_label: Label
var _assign_button: Button
var _elf_picker_scroll: ScrollContainer
var _elf_picker_vbox: VBoxContainer
var _inventory_label: Label
var _zoom_button: Button
var _anchor_x: float = 0.0
var _anchor_y: float = 0.0
var _anchor_z: float = 0.0
var _current_structure_id: int = -1
var _current_assigned_elf_id: String = ""
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

	# Home assignment section (visible for fully-furnished homes).
	_assign_section = VBoxContainer.new()
	_assign_section.add_theme_constant_override("separation", 4)
	_assign_section.visible = false
	vbox.add_child(_assign_section)

	_assign_label = Label.new()
	_assign_section.add_child(_assign_label)

	_assign_button = Button.new()
	_assign_button.pressed.connect(_on_assign_button_pressed)
	_assign_section.add_child(_assign_button)

	_elf_picker_scroll = ScrollContainer.new()
	_elf_picker_scroll.custom_minimum_size.y = 150
	_elf_picker_scroll.visible = false
	_assign_section.add_child(_elf_picker_scroll)

	_elf_picker_vbox = VBoxContainer.new()
	_elf_picker_vbox.add_theme_constant_override("separation", 2)
	_elf_picker_scroll.add_child(_elf_picker_vbox)

	# Inventory section.
	vbox.add_child(HSeparator.new())

	var inv_title := Label.new()
	inv_title.text = "Inventory"
	inv_title.add_theme_font_size_override("font_size", 16)
	vbox.add_child(inv_title)

	_inventory_label = Label.new()
	_inventory_label.text = "(empty)"
	vbox.add_child(_inventory_label)

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
	_elf_picker_scroll.visible = false
	_update_info(info)
	visible = true


func update_info(info: Dictionary) -> void:
	_update_info(info)


func hide_panel() -> void:
	_editing_name = false
	if _name_edit.has_focus():
		_name_edit.release_focus()
	visible = false


## Returns true if the elf picker is currently visible, so main.gd knows
## whether to fetch and provide the elf list.
func is_elf_picker_visible() -> bool:
	return _elf_picker_scroll.visible


## Populate the elf picker with the provided array of elf dictionaries.
## Each dict has: creature_id, name, rest, rest_max, assigned_home.
func set_elf_list(elves: Array) -> void:
	# Clear existing buttons.
	for child in _elf_picker_vbox.get_children():
		child.queue_free()

	for elf_dict in elves:
		var elf_name: String = elf_dict.get("name", "?")
		var rest: int = elf_dict.get("rest", 0)
		var rest_max: int = elf_dict.get("rest_max", 1)
		var rest_pct: int = 0
		if rest_max > 0:
			rest_pct = rest * 100 / rest_max
		var cid: String = elf_dict.get("creature_id", "")
		var has_home: int = elf_dict.get("assigned_home", -1)

		var label_text := "%s — Rest: %d%%" % [elf_name, rest_pct]
		if has_home >= 0:
			label_text += " [has home]"

		var btn := Button.new()
		btn.text = label_text
		btn.pressed.connect(_on_elf_picked.bind(cid))
		_elf_picker_vbox.add_child(btn)


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

	# Home assignment section — visible for fully-furnished homes.
	var assigned_elf_id: String = info.get("assigned_elf_id", "")
	var assigned_elf_name: String = info.get("assigned_elf_name", "")
	_current_assigned_elf_id = assigned_elf_id
	var is_home := furnishing == "Home" and not is_furnishing and furniture_count > 0

	if is_home:
		_assign_section.visible = true
		if assigned_elf_name != "":
			_assign_label.text = "Assigned: %s" % assigned_elf_name
			_assign_button.text = "Unassign"
		else:
			_assign_label.text = "Unassigned"
			_assign_button.text = "Assign Elf..."
	else:
		_assign_section.visible = false
		_elf_picker_scroll.visible = false

	_update_inventory(info)


func _update_inventory(info: Dictionary) -> void:
	var inv: Array = info.get("inventory", [])
	if inv.is_empty():
		_inventory_label.text = "(empty)"
		return
	var lines: PackedStringArray = []
	for entry in inv:
		var kind: String = entry.get("kind", "?")
		var qty: int = entry.get("quantity", 0)
		lines.append("%s: %d" % [kind, qty])
	_inventory_label.text = "\n".join(lines)


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


func _on_assign_button_pressed() -> void:
	if _current_assigned_elf_id != "":
		# Currently assigned — unassign.
		unassign_elf_requested.emit(_current_structure_id, _current_assigned_elf_id)
	else:
		# Not assigned — toggle elf picker.
		_elf_picker_scroll.visible = not _elf_picker_scroll.visible


func _on_elf_picked(creature_id_str: String) -> void:
	_elf_picker_scroll.visible = false
	if _current_structure_id >= 0:
		assign_elf_requested.emit(_current_structure_id, creature_id_str)


func _on_close_pressed() -> void:
	hide_panel()
	panel_closed.emit()
