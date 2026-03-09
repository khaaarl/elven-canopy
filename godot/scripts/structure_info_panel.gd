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
## For furnished buildings, a Logistics section is shown with:
## - An enable checkbox + priority SpinBox (1-10)
## - A list of item wants (kind + quantity) with remove buttons
## - An "Add Item..." button to add new wants
## Emits logistics_priority_changed and logistics_wants_changed.
##
## For Kitchen buildings, a Cooking section is shown with:
## - A "Cooking Enabled" checkbox
## - A "Bread Target" SpinBox (0-500, step 10)
## - A status label showing current cooking activity
## Emits cooking_config_changed.
##
## For Workshop buildings, a Crafting section is shown with:
## - A "Crafting Enabled" checkbox
## - Per-recipe CheckButtons showing input → output descriptions
## - Per-recipe target controls (LineEdit + ±increment buttons, 0 = don't craft)
## - A status label showing current crafting activity
## Emits workshop_config_changed.
##
## The panel is ~25% screen width, full height, anchored to the right edge.
## Updated every frame by main.gd while visible.
##
## See also: selection_controller.gd which triggers show/hide,
## creature_info_panel.gd for the creature equivalent,
## main.gd which wires everything together,
## sim_bridge.rs for rename_structure(), furnish_structure(), assign_home(),
## set_logistics_priority(), set_logistics_wants(), set_cooking_config(),
## set_workshop_config(), get_recipes().

extends PanelContainer

signal zoom_requested(x: float, y: float, z: float)
signal panel_closed
signal rename_requested(structure_id: int, new_name: String)
signal furnish_requested(structure_id: int, furnishing_type: String)
signal assign_elf_requested(structure_id: int, creature_id_str: String)
signal unassign_elf_requested(structure_id: int, creature_id_str: String)
signal logistics_priority_changed(structure_id: int, priority: int)
signal logistics_wants_changed(structure_id: int, wants_json: String)
signal cooking_config_changed(structure_id: int, cooking_enabled: bool, bread_target: int)
signal workshop_config_changed(structure_id: int, workshop_enabled: bool, recipe_configs: Array)

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
var _logistics_section: VBoxContainer
var _logistics_priority_hbox: HBoxContainer
var _logistics_priority_edit: LineEdit
var _logistics_enabled_check: CheckButton
var _logistics_wants_vbox: VBoxContainer
var _logistics_add_button: Button
var _logistics_item_picker: VBoxContainer
var _cooking_wrapper: VBoxContainer
var _cooking_section: VBoxContainer
var _cooking_enabled_check: CheckButton
var _cooking_bread_target_edit: LineEdit
var _cooking_status_label: Label
var _crafting_wrapper: VBoxContainer
var _crafting_summary_label: Label
var _crafting_details_button: Button
var _crafting_details_panel: PanelContainer
var _crafting_details_enabled_check: CheckButton
var _crafting_details_recipe_vbox: VBoxContainer
var _crafting_details_status_label: Label
## Recipe definitions from the bridge, cached on first load.
var _cached_recipes: Array = []
## Per-recipe widget references, built once when the details panel opens.
## Each entry: { check: CheckButton, target_edit: LineEdit, stock_label: Label, increment: int }
var _recipe_widgets: Array = []
## Whether recipe rows have been built for the current details panel session.
var _recipe_rows_built: bool = false
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

	# Logistics section (visible for furnished buildings).
	vbox.add_child(HSeparator.new())

	var logistics_title := Label.new()
	logistics_title.text = "Logistics"
	logistics_title.add_theme_font_size_override("font_size", 16)
	vbox.add_child(logistics_title)

	_logistics_section = VBoxContainer.new()
	_logistics_section.add_theme_constant_override("separation", 4)
	_logistics_section.visible = false
	vbox.add_child(_logistics_section)

	# Enable/priority row.
	_logistics_priority_hbox = HBoxContainer.new()
	_logistics_section.add_child(_logistics_priority_hbox)

	_logistics_enabled_check = CheckButton.new()
	_logistics_enabled_check.text = "Enabled"
	_logistics_enabled_check.toggled.connect(_on_logistics_enabled_toggled)
	_logistics_priority_hbox.add_child(_logistics_enabled_check)

	var priority_label := Label.new()
	priority_label.text = "Priority:"
	_logistics_priority_hbox.add_child(priority_label)

	var priority_minus := Button.new()
	priority_minus.text = "-1"
	priority_minus.custom_minimum_size.x = 36
	priority_minus.pressed.connect(_on_logistics_priority_button.bind(-1))
	_logistics_priority_hbox.add_child(priority_minus)

	_logistics_priority_edit = LineEdit.new()
	_logistics_priority_edit.text = "5"
	_logistics_priority_edit.custom_minimum_size.x = 36
	_logistics_priority_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_logistics_priority_edit.alignment = HORIZONTAL_ALIGNMENT_CENTER
	_logistics_priority_edit.text_submitted.connect(_on_logistics_priority_submitted)
	_logistics_priority_edit.focus_exited.connect(_on_logistics_priority_focus_exited)
	_logistics_priority_hbox.add_child(_logistics_priority_edit)

	var priority_plus := Button.new()
	priority_plus.text = "+1"
	priority_plus.custom_minimum_size.x = 36
	priority_plus.pressed.connect(_on_logistics_priority_button.bind(1))
	_logistics_priority_hbox.add_child(priority_plus)

	# Wants list.
	_logistics_wants_vbox = VBoxContainer.new()
	_logistics_wants_vbox.add_theme_constant_override("separation", 2)
	_logistics_section.add_child(_logistics_wants_vbox)

	# Add item button + picker.
	_logistics_add_button = Button.new()
	_logistics_add_button.text = "Add Item..."
	_logistics_add_button.pressed.connect(_on_logistics_add_pressed)
	_logistics_section.add_child(_logistics_add_button)

	_logistics_item_picker = VBoxContainer.new()
	_logistics_item_picker.visible = false
	_logistics_section.add_child(_logistics_item_picker)

	for item_name in ["Bread", "Fruit"]:
		var btn := Button.new()
		btn.text = item_name
		btn.pressed.connect(_on_logistics_item_picked.bind(item_name))
		_logistics_item_picker.add_child(btn)

	# Cooking section (visible for kitchen buildings).
	_cooking_wrapper = VBoxContainer.new()
	_cooking_wrapper.visible = false
	vbox.add_child(_cooking_wrapper)

	_cooking_wrapper.add_child(HSeparator.new())

	var cooking_title := Label.new()
	cooking_title.text = "Cooking"
	cooking_title.add_theme_font_size_override("font_size", 16)
	_cooking_wrapper.add_child(cooking_title)

	_cooking_section = VBoxContainer.new()
	_cooking_section.add_theme_constant_override("separation", 4)
	_cooking_wrapper.add_child(_cooking_section)

	_cooking_enabled_check = CheckButton.new()
	_cooking_enabled_check.text = "Cooking Enabled"
	_cooking_enabled_check.toggled.connect(_on_cooking_enabled_toggled)
	_cooking_section.add_child(_cooking_enabled_check)

	var bread_target_hbox := HBoxContainer.new()
	_cooking_section.add_child(bread_target_hbox)

	var bread_target_label := Label.new()
	bread_target_label.text = "Bread Target:"
	bread_target_hbox.add_child(bread_target_label)

	var bread_minus := Button.new()
	bread_minus.text = "-10"
	bread_minus.custom_minimum_size.x = 40
	bread_minus.pressed.connect(_on_cooking_bread_target_button.bind(-10))
	bread_target_hbox.add_child(bread_minus)

	_cooking_bread_target_edit = LineEdit.new()
	_cooking_bread_target_edit.text = "0"
	_cooking_bread_target_edit.custom_minimum_size.x = 48
	_cooking_bread_target_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_cooking_bread_target_edit.alignment = HORIZONTAL_ALIGNMENT_CENTER
	_cooking_bread_target_edit.text_submitted.connect(_on_cooking_bread_target_submitted)
	_cooking_bread_target_edit.focus_exited.connect(_on_cooking_bread_target_focus_exited)
	bread_target_hbox.add_child(_cooking_bread_target_edit)

	var bread_plus := Button.new()
	bread_plus.text = "+10"
	bread_plus.custom_minimum_size.x = 40
	bread_plus.pressed.connect(_on_cooking_bread_target_button.bind(10))
	bread_target_hbox.add_child(bread_plus)

	_cooking_status_label = Label.new()
	_cooking_status_label.text = ""
	_cooking_section.add_child(_cooking_status_label)

	# Crafting section — summary + details button (visible for workshop buildings).
	_crafting_wrapper = VBoxContainer.new()
	_crafting_wrapper.visible = false
	_crafting_wrapper.add_theme_constant_override("separation", 4)
	vbox.add_child(_crafting_wrapper)

	_crafting_wrapper.add_child(HSeparator.new())

	var crafting_title := Label.new()
	crafting_title.text = "Crafting"
	crafting_title.add_theme_font_size_override("font_size", 16)
	_crafting_wrapper.add_child(crafting_title)

	_crafting_summary_label = Label.new()
	_crafting_summary_label.text = ""
	_crafting_wrapper.add_child(_crafting_summary_label)

	_crafting_details_button = Button.new()
	_crafting_details_button.text = "Details..."
	_crafting_details_button.pressed.connect(_on_crafting_details_pressed)
	_crafting_wrapper.add_child(_crafting_details_button)

	# Crafting details panel — created as sibling, positioned left of main panel.
	_crafting_details_panel = PanelContainer.new()
	_crafting_details_panel.visible = false
	_crafting_details_panel.custom_minimum_size.x = 360

	var details_margin := MarginContainer.new()
	details_margin.add_theme_constant_override("margin_left", 12)
	details_margin.add_theme_constant_override("margin_right", 12)
	details_margin.add_theme_constant_override("margin_top", 12)
	details_margin.add_theme_constant_override("margin_bottom", 12)
	_crafting_details_panel.add_child(details_margin)

	var details_vbox := VBoxContainer.new()
	details_vbox.add_theme_constant_override("separation", 8)
	details_margin.add_child(details_vbox)

	var details_header := HBoxContainer.new()
	details_vbox.add_child(details_header)

	var details_title := Label.new()
	details_title.text = "Crafting Details"
	details_title.add_theme_font_size_override("font_size", 20)
	details_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	details_header.add_child(details_title)

	var details_close := Button.new()
	details_close.text = "X"
	details_close.pressed.connect(func(): _crafting_details_panel.visible = false)
	details_header.add_child(details_close)

	details_vbox.add_child(HSeparator.new())

	_crafting_details_enabled_check = CheckButton.new()
	_crafting_details_enabled_check.text = "Crafting Enabled"
	_crafting_details_enabled_check.toggled.connect(_on_crafting_enabled_toggled)
	details_vbox.add_child(_crafting_details_enabled_check)

	details_vbox.add_child(HSeparator.new())

	var recipes_scroll := ScrollContainer.new()
	recipes_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	details_vbox.add_child(recipes_scroll)

	_crafting_details_recipe_vbox = VBoxContainer.new()
	_crafting_details_recipe_vbox.add_theme_constant_override("separation", 8)
	_crafting_details_recipe_vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	recipes_scroll.add_child(_crafting_details_recipe_vbox)

	_crafting_details_status_label = Label.new()
	_crafting_details_status_label.text = ""
	details_vbox.add_child(_crafting_details_status_label)

	# Defer adding the details panel to the parent CanvasLayer so it's a sibling.
	call_deferred("_add_details_panel_to_parent")

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


func _add_details_panel_to_parent() -> void:
	var parent := get_parent()
	if parent:
		parent.add_child(_crafting_details_panel)
		# Position to the left of the main panel.
		_crafting_details_panel.anchor_top = 0.0
		_crafting_details_panel.anchor_bottom = 1.0
		_crafting_details_panel.anchor_left = 1.0
		_crafting_details_panel.anchor_right = 1.0
		_crafting_details_panel.offset_right = -328
		_crafting_details_panel.offset_left = -688
		_crafting_details_panel.offset_top = 0
		_crafting_details_panel.offset_bottom = 0


## Cache recipe definitions from the bridge. Called once by main.gd after
## the sim is initialized.
func set_recipes(recipes: Array) -> void:
	_cached_recipes = recipes


func show_structure(info: Dictionary) -> void:
	_editing_name = false
	_furnish_picker.visible = false
	_elf_picker_scroll.visible = false
	_logistics_item_picker.visible = false
	_crafting_details_panel.visible = false
	_recipe_rows_built = false
	_recipe_widgets.clear()
	_update_info(info)
	visible = true


func update_info(info: Dictionary) -> void:
	_update_info(info)


func hide_panel() -> void:
	_editing_name = false
	if _name_edit.has_focus():
		_name_edit.release_focus()
	_crafting_details_panel.visible = false
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
	_update_logistics(info, furnishing)
	_update_cooking(info, furnishing)
	_update_crafting(info, furnishing)


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


func _update_logistics(info: Dictionary, furnishing: String) -> void:
	# Only show logistics for furnished buildings.
	if furnishing == "":
		_logistics_section.visible = false
		return
	_logistics_section.visible = true

	var priority: int = info.get("logistics_priority", -1)
	var is_enabled := priority >= 0
	_logistics_enabled_check.set_pressed_no_signal(is_enabled)
	_logistics_priority_edit.editable = is_enabled
	if is_enabled and not _logistics_priority_edit.has_focus():
		_logistics_priority_edit.text = str(priority)

	# Rebuild wants list.
	for child in _logistics_wants_vbox.get_children():
		child.queue_free()
	var wants: Array = info.get("logistics_wants", [])
	for want in wants:
		var kind: String = want.get("kind", "?")
		var qty: int = want.get("target_quantity", 0)
		var row := HBoxContainer.new()
		var label := Label.new()
		label.text = "%s: %d" % [kind, qty]
		label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		row.add_child(label)
		var remove_btn := Button.new()
		remove_btn.text = "X"
		remove_btn.pressed.connect(_on_logistics_want_removed.bind(kind))
		row.add_child(remove_btn)
		_logistics_wants_vbox.add_child(row)


func _update_cooking(info: Dictionary, furnishing: String) -> void:
	if furnishing != "Kitchen":
		_cooking_wrapper.visible = false
		return
	_cooking_wrapper.visible = true

	var cooking_enabled: bool = info.get("cooking_enabled", false)
	var bread_target: int = info.get("cooking_bread_target", 0)
	var cook_status: String = info.get("cook_status", "")

	_cooking_enabled_check.set_pressed_no_signal(cooking_enabled)
	_cooking_bread_target_edit.editable = cooking_enabled
	if not _cooking_bread_target_edit.has_focus():
		_cooking_bread_target_edit.text = str(bread_target)

	if cook_status != "":
		_cooking_status_label.text = cook_status
		_cooking_status_label.visible = true
	else:
		_cooking_status_label.visible = false


func _on_cooking_enabled_toggled(_pressed: bool) -> void:
	if _current_structure_id < 0:
		return
	_emit_cooking_config()


func _on_cooking_bread_target_button(delta: int) -> void:
	if _current_structure_id < 0:
		return
	var current := maxi(0, int(_cooking_bread_target_edit.text))
	_cooking_bread_target_edit.text = str(maxi(0, current + delta))
	_emit_cooking_config()


func _on_cooking_bread_target_submitted(_text: String) -> void:
	if _current_structure_id < 0:
		return
	var val := maxi(0, int(_cooking_bread_target_edit.text))
	_cooking_bread_target_edit.text = str(val)
	_cooking_bread_target_edit.release_focus()


func _on_cooking_bread_target_focus_exited() -> void:
	if _current_structure_id < 0:
		return
	var val := maxi(0, int(_cooking_bread_target_edit.text))
	_cooking_bread_target_edit.text = str(val)
	_emit_cooking_config()


func _emit_cooking_config() -> void:
	var bread_target := maxi(0, int(_cooking_bread_target_edit.text))
	cooking_config_changed.emit(
		_current_structure_id, _cooking_enabled_check.button_pressed, bread_target
	)


func _update_crafting(info: Dictionary, furnishing: String) -> void:
	if furnishing != "Workshop":
		_crafting_wrapper.visible = false
		if _crafting_details_panel:
			_crafting_details_panel.visible = false
		return
	_crafting_wrapper.visible = true

	var workshop_enabled: bool = info.get("workshop_enabled", false)
	var active_recipe_ids: Array = info.get("workshop_recipe_ids", [])
	var craft_status: String = info.get("craft_status", "")

	# Update summary in main panel. Count recipes with target > 0 as "active."
	var recipe_targets: Dictionary = info.get("workshop_recipe_targets", {})
	var active_count := 0
	for rid in active_recipe_ids:
		if int(recipe_targets.get(rid, 0)) > 0:
			active_count += 1
	var status_suffix := ""
	if craft_status != "":
		status_suffix = " — %s" % craft_status
	_crafting_summary_label.text = (
		"%d recipe%s active%s" % [active_count, "s" if active_count != 1 else "", status_suffix]
	)

	# Update details panel if visible.
	if not _crafting_details_panel.visible:
		return

	_crafting_details_enabled_check.set_pressed_no_signal(workshop_enabled)

	var recipe_stocks: Dictionary = info.get("workshop_recipe_stocks", {})

	# Build recipe rows once; update in-place on subsequent frames.
	if not _recipe_rows_built:
		_build_recipe_rows(workshop_enabled, active_recipe_ids, recipe_targets, recipe_stocks)
		_recipe_rows_built = true
	else:
		_refresh_recipe_rows(workshop_enabled, active_recipe_ids, recipe_targets, recipe_stocks)

	# Status label in details panel.
	if craft_status != "":
		_crafting_details_status_label.text = "Status: %s" % craft_status
		_crafting_details_status_label.visible = true
	else:
		_crafting_details_status_label.visible = false


## Build recipe row widgets from scratch. Called once when the details panel
## first becomes visible for a given structure.
func _build_recipe_rows(
	workshop_enabled: bool,
	active_recipe_ids: Array,
	recipe_targets: Dictionary,
	recipe_stocks: Dictionary,
) -> void:
	# Clear any leftover rows.
	for child in _crafting_details_recipe_vbox.get_children():
		child.queue_free()
	_recipe_widgets.clear()

	for recipe in _cached_recipes:
		var rid: String = recipe.get("id", "")
		var display: String = recipe.get("display_name", rid)
		var inputs: Array = recipe.get("inputs", [])
		var outputs: Array = recipe.get("outputs", [])

		var row := VBoxContainer.new()
		row.add_theme_constant_override("separation", 2)

		# Row 1: CheckButton with recipe name.
		var check := CheckButton.new()
		check.text = display
		check.set_pressed_no_signal(active_recipe_ids.has(rid))
		check.disabled = not workshop_enabled
		check.toggled.connect(_on_recipe_toggled.bind(rid))
		row.add_child(check)

		# Row 2: Input → Output description (indented).
		var parts: PackedStringArray = []
		for inp in inputs:
			parts.append("%d %s" % [inp.get("quantity", 0), inp.get("item_kind", "?")])
		var input_str := " + ".join(parts) if parts.size() > 0 else "(nothing)"
		parts = []
		for out in outputs:
			parts.append("%d %s" % [out.get("quantity", 0), out.get("item_kind", "?")])
		var output_str := " + ".join(parts)
		var desc := Label.new()
		desc.text = "  %s → %s" % [input_str, output_str]
		row.add_child(desc)

		# Row 3: Target control + Stock count (indented).
		# Increment is the recipe's total output quantity (e.g. 20 for arrows).
		var increment := 1
		for out in outputs:
			increment = maxi(increment, int(out.get("quantity", 1)))

		var target_row := HBoxContainer.new()
		var target_label := Label.new()
		target_label.text = "  Target:"
		target_row.add_child(target_label)

		var minus_btn := Button.new()
		minus_btn.text = "-%d" % increment
		minus_btn.custom_minimum_size.x = 36
		minus_btn.pressed.connect(_on_target_button.bind(rid, -increment))
		target_row.add_child(minus_btn)

		var current_target: int = recipe_targets.get(rid, 0)
		var target_edit := LineEdit.new()
		target_edit.text = str(current_target)
		target_edit.custom_minimum_size.x = 48
		target_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		target_edit.alignment = HORIZONTAL_ALIGNMENT_CENTER
		target_edit.tooltip_text = "0 = don't craft"
		target_edit.text_submitted.connect(_on_target_text_submitted.bind(rid))
		target_edit.focus_exited.connect(_on_target_focus_exited.bind(rid, target_edit))
		target_row.add_child(target_edit)

		var plus_btn := Button.new()
		plus_btn.text = "+%d" % increment
		plus_btn.custom_minimum_size.x = 36
		plus_btn.pressed.connect(_on_target_button.bind(rid, increment))
		target_row.add_child(plus_btn)

		var stock_count: int = recipe_stocks.get(rid, 0)
		var stock_label := Label.new()
		stock_label.text = "Stock: %d" % stock_count
		target_row.add_child(stock_label)

		row.add_child(target_row)
		row.add_child(HSeparator.new())
		_crafting_details_recipe_vbox.add_child(row)

		(
			_recipe_widgets
			. append(
				{
					"check": check,
					"target_edit": target_edit,
					"stock_label": stock_label,
					"increment": increment,
				}
			)
		)


## Update existing recipe row widgets in-place without destroying them.
## This preserves LineEdit focus and pending text edits.
func _refresh_recipe_rows(
	workshop_enabled: bool,
	active_recipe_ids: Array,
	recipe_targets: Dictionary,
	recipe_stocks: Dictionary,
) -> void:
	for i in range(_recipe_widgets.size()):
		if i >= _cached_recipes.size():
			break
		var rid: String = _cached_recipes[i].get("id", "")
		var w: Dictionary = _recipe_widgets[i]
		var check: CheckButton = w["check"]
		var target_edit: LineEdit = w["target_edit"]
		var stock_label: Label = w["stock_label"]

		check.set_pressed_no_signal(active_recipe_ids.has(rid))
		check.disabled = not workshop_enabled

		target_edit.editable = workshop_enabled and active_recipe_ids.has(rid)
		# Only update the text if the user isn't actively editing it.
		if not target_edit.has_focus():
			var current_target: int = recipe_targets.get(rid, 0)
			target_edit.text = str(current_target)

		var stock_count: int = recipe_stocks.get(rid, 0)
		stock_label.text = "Stock: %d" % stock_count


func _on_crafting_details_pressed() -> void:
	_crafting_details_panel.visible = not _crafting_details_panel.visible
	# Reset built flag so rows are rebuilt fresh when reopening.
	if _crafting_details_panel.visible:
		_recipe_rows_built = false
		_recipe_widgets.clear()


func _on_crafting_enabled_toggled(_pressed: bool) -> void:
	if _current_structure_id < 0:
		return
	_emit_workshop_config()


func _on_recipe_toggled(_pressed: bool, _recipe_id: String) -> void:
	if _current_structure_id < 0:
		return
	_emit_workshop_config()


func _on_target_button(recipe_id: String, delta: int) -> void:
	if _current_structure_id < 0:
		return
	# Find the widget for this recipe and adjust the value.
	for i in range(_recipe_widgets.size()):
		if i < _cached_recipes.size() and _cached_recipes[i].get("id", "") == recipe_id:
			var target_edit: LineEdit = _recipe_widgets[i]["target_edit"]
			var current := maxi(0, int(target_edit.text))
			var new_val := maxi(0, current + delta)
			target_edit.text = str(new_val)
			break
	_emit_workshop_config()


func _on_target_text_submitted(_text: String, recipe_id: String) -> void:
	if _current_structure_id < 0:
		return
	# Clamp to >= 0 and update the text to the cleaned value.
	# release_focus() triggers _on_target_focus_exited which calls _emit_workshop_config.
	for i in range(_recipe_widgets.size()):
		if i < _cached_recipes.size() and _cached_recipes[i].get("id", "") == recipe_id:
			var target_edit: LineEdit = _recipe_widgets[i]["target_edit"]
			var val := maxi(0, int(target_edit.text))
			target_edit.text = str(val)
			target_edit.release_focus()
			break


func _on_target_focus_exited(_recipe_id: String, target_edit: LineEdit) -> void:
	if _current_structure_id < 0:
		return
	var val := maxi(0, int(target_edit.text))
	target_edit.text = str(val)
	_emit_workshop_config()


func _emit_workshop_config() -> void:
	var enabled: bool = _crafting_details_enabled_check.button_pressed
	var recipe_configs: Array = []
	for i in range(_recipe_widgets.size()):
		if i >= _cached_recipes.size():
			break
		var w: Dictionary = _recipe_widgets[i]
		var check: CheckButton = w["check"]
		var target_edit: LineEdit = w["target_edit"]
		if check.button_pressed:
			var target_val := maxi(0, int(target_edit.text))
			recipe_configs.append({"id": _cached_recipes[i].get("id", ""), "target": target_val})
	workshop_config_changed.emit(_current_structure_id, enabled, recipe_configs)


func _on_logistics_enabled_toggled(pressed: bool) -> void:
	if _current_structure_id < 0:
		return
	if pressed:
		var p := clampi(int(_logistics_priority_edit.text), 1, 10)
		logistics_priority_changed.emit(_current_structure_id, p)
	else:
		logistics_priority_changed.emit(_current_structure_id, -1)


func _on_logistics_priority_button(delta: int) -> void:
	if _current_structure_id < 0:
		return
	var current := clampi(int(_logistics_priority_edit.text), 1, 10)
	var new_val := clampi(current + delta, 1, 10)
	_logistics_priority_edit.text = str(new_val)
	if _logistics_enabled_check.button_pressed:
		logistics_priority_changed.emit(_current_structure_id, new_val)


func _on_logistics_priority_submitted(_text: String) -> void:
	if _current_structure_id < 0:
		return
	var val := clampi(int(_logistics_priority_edit.text), 1, 10)
	_logistics_priority_edit.text = str(val)
	_logistics_priority_edit.release_focus()


func _on_logistics_priority_focus_exited() -> void:
	if _current_structure_id < 0:
		return
	var val := clampi(int(_logistics_priority_edit.text), 1, 10)
	_logistics_priority_edit.text = str(val)
	if _logistics_enabled_check.button_pressed:
		logistics_priority_changed.emit(_current_structure_id, val)


func _on_logistics_add_pressed() -> void:
	_logistics_item_picker.visible = not _logistics_item_picker.visible


func _on_logistics_item_picked(item_name: String) -> void:
	_logistics_item_picker.visible = false
	if _current_structure_id < 0:
		return
	# Add a want with default quantity 10, or increment existing.
	_emit_wants_with_added(item_name, 10)


func _on_logistics_want_removed(kind: String) -> void:
	if _current_structure_id < 0:
		return
	_emit_wants_without(kind)


func _emit_wants_with_added(kind: String, default_qty: int) -> void:
	# Build JSON from current wants, adding or updating the specified kind.
	var wants: Array = []
	var found := false
	for child in _logistics_wants_vbox.get_children():
		if child is HBoxContainer:
			var label: Label = child.get_child(0)
			var parts := label.text.split(": ")
			if parts.size() == 2:
				var k: String = parts[0]
				var q: int = int(parts[1])
				if k == kind:
					q += default_qty
					found = true
				wants.append({"kind": k, "quantity": q})
	if not found:
		wants.append({"kind": kind, "quantity": default_qty})
	logistics_wants_changed.emit(_current_structure_id, JSON.stringify(wants))


func _emit_wants_without(kind: String) -> void:
	var wants: Array = []
	for child in _logistics_wants_vbox.get_children():
		if child is HBoxContainer:
			var label: Label = child.get_child(0)
			var parts := label.text.split(": ")
			if parts.size() == 2:
				var k: String = parts[0]
				var q: int = int(parts[1])
				if k != kind:
					wants.append({"kind": k, "quantity": q})
	logistics_wants_changed.emit(_current_structure_id, JSON.stringify(wants))


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
