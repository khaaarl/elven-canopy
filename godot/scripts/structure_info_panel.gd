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
## For furnished buildings, a Logistics section is shown with a summary label
## and "Details..." button. The details open a secondary panel (sibling
## PanelContainer positioned left of the main panel, same pattern as crafting)
## containing:
## - An enable checkbox + priority controls (1-10)
## - A scrollable wants list (kind + material filter + quantity controls + remove)
## - An "Add Item..." two-step picker (item kind -> material filter)
## Emits logistics_priority_changed and logistics_wants_changed.
##
## For Kitchen and Workshop buildings (any building with crafting recipes),
## a unified Crafting section is shown with a summary and "Details..." button.
## The detail panel (sibling PanelContainer) contains:
## - A "Crafting Enabled" toggle
## - An "Add Recipe..." button that expands a hierarchical recipe picker
##   organized as a collapsible tree by category (e.g. Processing > Milling),
##   with a search/filter field for case-insensitive substring matching against
##   recipe names and input/output item kinds
## - Active recipe list with per-output target controls, auto-logistics toggle,
##   reorder/remove buttons, and per-recipe enable toggles
## Emits crafting_enabled_changed, add_recipe_requested, remove_recipe_requested,
## recipe_output_target_changed, recipe_auto_logistics_changed,
## recipe_enabled_changed, recipe_move_up_requested, recipe_move_down_requested.
##
## The panel is ~25% screen width, full height, anchored to the right edge.
## Updated every frame by main.gd while visible.
##
## See also: selection_controller.gd which triggers show/hide,
## creature_info_panel.gd for the creature equivalent,
## main.gd which wires everything together,
## sim_bridge.rs for rename_structure(), furnish_structure(), assign_home(),
## set_logistics_priority(), set_logistics_wants(), set_crafting_enabled(),
## get_available_recipes(), add_active_recipe(), etc.

extends PanelContainer

signal zoom_requested(x: float, y: float, z: float)
signal panel_closed
signal rename_requested(structure_id: int, new_name: String)
signal furnish_requested(structure_id: int, furnishing_type: String, species_id: int)
signal assign_elf_requested(structure_id: int, creature_id_str: String)
signal unassign_elf_requested(structure_id: int, creature_id_str: String)
signal logistics_priority_changed(structure_id: int, priority: int)
signal logistics_wants_changed(structure_id: int, wants_json: String)
signal crafting_enabled_changed(structure_id: int, enabled: bool)
signal add_recipe_requested(structure_id: int, recipe_variant: int, material_json: String)
signal remove_recipe_requested(active_recipe_id: int)
signal recipe_output_target_changed(active_recipe_target_id: int, target_quantity: int)
signal recipe_auto_logistics_changed(
	active_recipe_id: int, auto_logistics: bool, spare_iterations: int
)
signal recipe_enabled_changed(active_recipe_id: int, enabled: bool)
signal recipe_move_up_requested(active_recipe_id: int)
signal recipe_move_down_requested(active_recipe_id: int)
signal item_clicked(item_stack_id: int)

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
var _inventory_container: VBoxContainer
var _inventory_empty_label: Label
## Cache of the last inventory array passed to _update_inventory(), used to skip
## redundant button rebuilds on per-frame refreshes. Null means "no cached
## value" — the cache is invalidated (set to null) in show_structure() so that
## _update_inventory() always rebuilds when switching to a new structure. This
## is distinct from [] (cached, genuinely empty inventory) which correctly
## skips rebuild when the structure simply has no items.
var _last_inventory = null
var _logistics_wrapper: VBoxContainer
var _logistics_summary_label: Label
var _logistics_details_button: Button
var _logistics_details_panel: PanelContainer
var _logistics_priority_hbox: HBoxContainer
var _logistics_priority_edit: LineEdit
var _logistics_enabled_check: CheckButton
var _logistics_wants_editor: Control  # WantsEditor instance
var _cached_logistics_item_kinds: Array = []
var _cached_logistics_material_options: Dictionary = {}
var _crafting_wrapper: VBoxContainer
var _crafting_summary_label: Label
var _crafting_details_button: Button
var _crafting_details_panel: PanelContainer
var _crafting_details_scroll: ScrollContainer
var _crafting_details_vbox: VBoxContainer
var _crafting_details_enabled_check: CheckButton
var _crafting_add_recipe_button: Button
var _crafting_recipe_picker: VBoxContainer
var _crafting_active_recipes_vbox: VBoxContainer
var _crafting_details_status_label: Label
## Whether the recipe picker is currently expanded.
var _recipe_picker_visible: bool = false
var _recipe_search_edit: LineEdit
## Cached recipe catalog for the current building (from bridge).
var _cached_building_recipes: Array = []
## Collapsed state per top-level category in the recipe picker tree.
## Keys are category name strings, values are bools (true = collapsed).
var _recipe_category_collapsed: Dictionary = {}
## Cached active recipe key JSONs for re-applying availability after tree rebuild.
var _cached_active_recipe_keys: Array = []
## Per-active-recipe widget references, rebuilt each frame when details visible.
## Stored to preserve focus state on LineEdits.
var _active_recipe_widgets: Dictionary = {}
## Whether active recipe rows have been built for the current details panel session.
var _recipe_rows_built: bool = false
var _greenhouse_picker_scroll: ScrollContainer
var _greenhouse_picker_vbox: VBoxContainer
var _cached_cultivable_fruits: Array = []
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
		["Dance Hall", "DanceHall"],
		["Dining Hall", "DiningHall"],
		["Dormitory", "Dormitory"],
		["Greenhouse", "Greenhouse"],
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

	# Greenhouse species picker (hidden; shown when "Greenhouse" is selected).
	_greenhouse_picker_scroll = ScrollContainer.new()
	_greenhouse_picker_scroll.custom_minimum_size.y = 150
	_greenhouse_picker_scroll.visible = false
	vbox.add_child(_greenhouse_picker_scroll)

	_greenhouse_picker_vbox = VBoxContainer.new()
	_greenhouse_picker_vbox.add_theme_constant_override("separation", 2)
	_greenhouse_picker_scroll.add_child(_greenhouse_picker_vbox)

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

	_inventory_container = VBoxContainer.new()
	_inventory_container.add_theme_constant_override("separation", 2)
	vbox.add_child(_inventory_container)

	_inventory_empty_label = Label.new()
	_inventory_empty_label.text = "(empty)"
	_inventory_container.add_child(_inventory_empty_label)

	# Logistics section — summary + details button (visible for furnished buildings).
	_logistics_wrapper = VBoxContainer.new()
	_logistics_wrapper.visible = false
	_logistics_wrapper.add_theme_constant_override("separation", 4)
	vbox.add_child(_logistics_wrapper)

	_logistics_wrapper.add_child(HSeparator.new())

	var logistics_title := Label.new()
	logistics_title.text = "Logistics"
	logistics_title.add_theme_font_size_override("font_size", 16)
	_logistics_wrapper.add_child(logistics_title)

	_logistics_summary_label = Label.new()
	_logistics_summary_label.text = ""
	_logistics_wrapper.add_child(_logistics_summary_label)

	_logistics_details_button = Button.new()
	_logistics_details_button.text = "Details..."
	_logistics_details_button.pressed.connect(_on_logistics_details_pressed)
	_logistics_wrapper.add_child(_logistics_details_button)

	# Logistics details panel — created as sibling, positioned left of main panel.
	_logistics_details_panel = PanelContainer.new()
	_logistics_details_panel.visible = false
	_logistics_details_panel.custom_minimum_size.x = 360

	var logistics_margin := MarginContainer.new()
	logistics_margin.add_theme_constant_override("margin_left", 12)
	logistics_margin.add_theme_constant_override("margin_right", 12)
	logistics_margin.add_theme_constant_override("margin_top", 12)
	logistics_margin.add_theme_constant_override("margin_bottom", 12)
	_logistics_details_panel.add_child(logistics_margin)

	var logistics_vbox := VBoxContainer.new()
	logistics_vbox.add_theme_constant_override("separation", 8)
	logistics_margin.add_child(logistics_vbox)

	var logistics_header := HBoxContainer.new()
	logistics_vbox.add_child(logistics_header)

	var logistics_header_title := Label.new()
	logistics_header_title.text = "Logistics Details"
	logistics_header_title.add_theme_font_size_override("font_size", 20)
	logistics_header_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	logistics_header.add_child(logistics_header_title)

	var logistics_close := Button.new()
	logistics_close.text = "X"
	logistics_close.pressed.connect(func(): _logistics_details_panel.visible = false)
	logistics_header.add_child(logistics_close)

	logistics_vbox.add_child(HSeparator.new())

	# Enable/priority row inside details panel.
	_logistics_priority_hbox = HBoxContainer.new()
	logistics_vbox.add_child(_logistics_priority_hbox)

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

	logistics_vbox.add_child(HSeparator.new())

	# Wants list editor (reusable widget).
	var editor_script = load("res://scripts/wants_editor.gd")
	_logistics_wants_editor = VBoxContainer.new()
	_logistics_wants_editor.set_script(editor_script)
	_logistics_wants_editor.default_add_quantity = 10
	_logistics_wants_editor.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_logistics_wants_editor.wants_changed.connect(_on_logistics_wants_editor_changed)
	logistics_vbox.add_child(_logistics_wants_editor)

	# Crafting section — summary + details button (visible for crafting buildings).
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
	_crafting_details_panel.custom_minimum_size.x = 400

	var details_margin := MarginContainer.new()
	details_margin.add_theme_constant_override("margin_left", 12)
	details_margin.add_theme_constant_override("margin_right", 12)
	details_margin.add_theme_constant_override("margin_top", 12)
	details_margin.add_theme_constant_override("margin_bottom", 12)
	_crafting_details_panel.add_child(details_margin)

	# The entire detail panel is one scroll container.
	_crafting_details_scroll = ScrollContainer.new()
	_crafting_details_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_crafting_details_scroll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	details_margin.add_child(_crafting_details_scroll)

	_crafting_details_vbox = VBoxContainer.new()
	_crafting_details_vbox.add_theme_constant_override("separation", 8)
	_crafting_details_vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_crafting_details_scroll.add_child(_crafting_details_vbox)

	var details_header := HBoxContainer.new()
	_crafting_details_vbox.add_child(details_header)

	var details_title := Label.new()
	details_title.text = "Crafting Details"
	details_title.add_theme_font_size_override("font_size", 20)
	details_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	details_header.add_child(details_title)

	var details_close := Button.new()
	details_close.text = "X"
	details_close.pressed.connect(
		func():
			_crafting_details_panel.visible = false
			_recipe_picker_visible = false
			_recipe_search_edit.visible = false
			_recipe_search_edit.text = ""
	)
	details_header.add_child(details_close)

	_crafting_details_vbox.add_child(HSeparator.new())

	_crafting_details_enabled_check = CheckButton.new()
	_crafting_details_enabled_check.text = "Crafting Enabled"
	_crafting_details_enabled_check.toggled.connect(_on_crafting_enabled_toggled)
	_crafting_details_vbox.add_child(_crafting_details_enabled_check)

	_crafting_details_vbox.add_child(HSeparator.new())

	# Add Recipe button and picker.
	_crafting_add_recipe_button = Button.new()
	_crafting_add_recipe_button.text = "Add Recipe..."
	_crafting_add_recipe_button.pressed.connect(_on_add_recipe_pressed)
	_crafting_details_vbox.add_child(_crafting_add_recipe_button)

	_recipe_search_edit = LineEdit.new()
	_recipe_search_edit.placeholder_text = "Filter recipes..."
	_recipe_search_edit.clear_button_enabled = true
	_recipe_search_edit.visible = false
	_recipe_search_edit.text_changed.connect(_on_recipe_search_changed)
	_crafting_details_vbox.add_child(_recipe_search_edit)

	_crafting_recipe_picker = VBoxContainer.new()
	_crafting_recipe_picker.visible = false
	_crafting_recipe_picker.add_theme_constant_override("separation", 2)
	_crafting_details_vbox.add_child(_crafting_recipe_picker)

	_crafting_details_vbox.add_child(HSeparator.new())

	# Active recipes list.
	_crafting_active_recipes_vbox = VBoxContainer.new()
	_crafting_active_recipes_vbox.add_theme_constant_override("separation", 4)
	_crafting_active_recipes_vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_crafting_details_vbox.add_child(_crafting_active_recipes_vbox)

	_crafting_details_status_label = Label.new()
	_crafting_details_status_label.text = ""
	_crafting_details_vbox.add_child(_crafting_details_status_label)

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


func _notification(what: int) -> void:
	# Add the details panels to the parent CanvasLayer when this node
	# gets a parent. NOTIFICATION_PARENTED fires reliably during add_child(),
	# unlike _ready which may fire during set_script() before the node is
	# in the tree.
	if what == NOTIFICATION_PARENTED:
		call_deferred("_add_details_panel_to_parent")


func _add_details_panel_to_parent() -> void:
	var parent := get_parent()
	if not parent:
		return
	# Guard against double-add (e.g., if _enter_tree fires multiple times).
	if _crafting_details_panel.get_parent() == parent:
		return
	# Both details panels share the same screen position (left of main panel).
	# The toggle handlers ensure only one is visible at a time.
	for panel in [_crafting_details_panel, _logistics_details_panel]:
		parent.add_child(panel)
		panel.anchor_top = 0.0
		panel.anchor_bottom = 1.0
		panel.anchor_left = 1.0
		panel.anchor_right = 1.0
		panel.offset_right = -328
		panel.offset_left = -688
		panel.offset_top = 0
		panel.offset_bottom = 0
	# PanelContainer shrinks to content minimum, and ScrollContainer has zero
	# minimum height — force full viewport height so the scroll area is visible.
	# See docs/godot_scroll_sizing.md.
	_match_details_viewport_height()
	get_viewport().size_changed.connect(_match_details_viewport_height)


func _match_details_viewport_height() -> void:
	var h: float = get_viewport().get_visible_rect().size.y
	_logistics_details_panel.custom_minimum_size.y = h
	_crafting_details_panel.custom_minimum_size.y = h


## Cache the recipe catalog for the current building. Called by main.gd
## when the details panel opens or the structure changes.
func set_building_recipes(recipes: Array) -> void:
	_cached_building_recipes = recipes


func set_cultivable_fruits(fruits: Array) -> void:
	_cached_cultivable_fruits = fruits


## Cache logistics item kinds from the bridge. Called by main.gd.
func set_logistics_item_kinds(kinds: Array) -> void:
	_cached_logistics_item_kinds = kinds
	_logistics_wants_editor.set_picker_data(
		_cached_logistics_item_kinds, _cached_logistics_material_options
	)


## Cache logistics material options from the bridge. Called by main.gd.
func set_logistics_material_options(options: Dictionary) -> void:
	_cached_logistics_material_options = options
	_logistics_wants_editor.set_picker_data(
		_cached_logistics_item_kinds, _cached_logistics_material_options
	)


func show_structure(info: Dictionary) -> void:
	_last_inventory = null  # Invalidate cache so _update_inventory rebuilds.
	_editing_name = false
	_furnish_picker.visible = false
	_greenhouse_picker_scroll.visible = false
	_elf_picker_scroll.visible = false
	_logistics_details_panel.visible = false
	_crafting_details_panel.visible = false
	_recipe_rows_built = false
	_active_recipe_widgets.clear()
	_recipe_picker_visible = false
	_cached_building_recipes = []
	_update_info(info)
	visible = true


func update_info(info: Dictionary) -> void:
	_update_info(info)


func hide_panel() -> void:
	_editing_name = false
	if _name_edit.has_focus():
		_name_edit.release_focus()
	_logistics_details_panel.visible = false
	_crafting_details_panel.visible = false
	_recipe_picker_visible = false
	_recipe_search_edit.visible = false
	_recipe_search_edit.text = ""
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


## Returns true if the crafting details panel is visible (for main.gd to
## know when to fetch and provide the recipe catalog).
func is_crafting_details_visible() -> bool:
	return _crafting_details_panel.visible


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
		elif planned_furniture_count == 0:
			# Furniture-less furnishings (e.g. Dance Hall) — show name only.
			_furnish_label.text = furnishing
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
		_greenhouse_picker_scroll.visible = false

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
	_update_crafting(info, furnishing)


func _update_inventory(info: Dictionary) -> void:
	var inv: Array = info.get("inventory", [])
	# Skip rebuild if inventory hasn't changed — newly-created buttons don't
	# have a valid layout rect until the next frame, so clicks fall through.
	if _last_inventory != null and inv == _last_inventory:
		return
	_last_inventory = inv.duplicate(true)

	# Remove old item buttons (keep the empty label at index 0).
	while _inventory_container.get_child_count() > 1:
		var child := _inventory_container.get_child(1)
		_inventory_container.remove_child(child)
		child.queue_free()

	if inv.is_empty():
		_inventory_empty_label.visible = true
		return

	_inventory_empty_label.visible = false
	for entry in inv:
		var kind: String = entry.get("kind", "?")
		var qty: int = entry.get("quantity", 0)
		var stack_id: int = entry.get("item_stack_id", -1)
		var btn := Button.new()
		btn.text = "%s: %d" % [kind, qty]
		btn.alignment = HORIZONTAL_ALIGNMENT_LEFT
		btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		btn.set_meta("item_stack_id", stack_id)
		btn.pressed.connect(_on_item_pressed.bind(stack_id))
		_inventory_container.add_child(btn)


func _on_item_pressed(stack_id: int) -> void:
	item_clicked.emit(stack_id)


func _update_logistics(info: Dictionary, furnishing: String) -> void:
	# Only show logistics for furnished buildings.
	if furnishing == "":
		_logistics_wrapper.visible = false
		if _logistics_details_panel:
			_logistics_details_panel.visible = false
		return
	_logistics_wrapper.visible = true

	var priority: int = info.get("logistics_priority", -1)
	var is_enabled := priority >= 0
	var wants: Array = info.get("logistics_wants", [])

	# Update summary in main panel.
	if is_enabled:
		_logistics_summary_label.text = (
			"Enabled (priority %d), %d want%s"
			% [priority, wants.size(), "s" if wants.size() != 1 else ""]
		)
	else:
		_logistics_summary_label.text = "Disabled"

	# Update details panel if visible.
	if not _logistics_details_panel.visible:
		return

	_logistics_enabled_check.set_pressed_no_signal(is_enabled)
	_logistics_priority_edit.editable = is_enabled
	if is_enabled and not _logistics_priority_edit.has_focus():
		_logistics_priority_edit.text = str(priority)

	# Update wants list via the reusable editor widget.
	_logistics_wants_editor.update_wants(wants)


func _update_crafting(info: Dictionary, furnishing: String) -> void:
	# Show crafting section for any building with a furnishing type that has recipes.
	# Kitchen and Workshop are the current crafting building types.
	if furnishing != "Kitchen" and furnishing != "Workshop":
		_crafting_wrapper.visible = false
		if _crafting_details_panel:
			_crafting_details_panel.visible = false
		return
	_crafting_wrapper.visible = true

	var crafting_enabled: bool = info.get("crafting_enabled", false)
	var active_count: int = info.get("active_recipe_count", 0)
	var satisfied_count: int = info.get("satisfied_recipe_count", 0)
	var craft_status: String = info.get("craft_status", "")

	# Update summary in main panel.
	var status_suffix := ""
	if craft_status != "":
		status_suffix = " — %s" % craft_status
	if active_count > 0:
		_crafting_summary_label.text = (
			"%d active (%d satisfied)%s" % [active_count, satisfied_count, status_suffix]
		)
	elif crafting_enabled:
		_crafting_summary_label.text = "No active recipes" + status_suffix
	else:
		_crafting_summary_label.text = "Disabled"

	# Update details panel if visible.
	if not _crafting_details_panel.visible:
		return

	_crafting_details_enabled_check.set_pressed_no_signal(crafting_enabled)

	var active_recipes: Array = info.get("active_recipes", [])

	# Build active recipe rows once; update in-place on subsequent frames.
	if not _recipe_rows_built:
		_build_active_recipe_rows(active_recipes)
		_recipe_rows_built = true
	else:
		_refresh_active_recipe_rows(active_recipes)

	# Update recipe picker availability (gray out already-active recipes).
	_update_recipe_picker_availability(active_recipes)

	# Status label in details panel.
	if craft_status != "":
		_crafting_details_status_label.text = "Status: %s" % craft_status
		_crafting_details_status_label.visible = true
	else:
		_crafting_details_status_label.visible = false


## Build active recipe row widgets from scratch.
func _build_active_recipe_rows(active_recipes: Array) -> void:
	for child in _crafting_active_recipes_vbox.get_children():
		child.queue_free()
	_active_recipe_widgets.clear()

	for ar in active_recipes:
		var ar_id: int = ar.get("active_recipe_id", 0)
		var display_name: String = ar.get("recipe_display_name", "?")
		var enabled: bool = ar.get("enabled", false)
		var auto_logistics: bool = ar.get("auto_logistics", true)
		var spare_iterations: int = ar.get("spare_iterations", 0)
		var targets: Array = ar.get("targets", [])

		var section := VBoxContainer.new()
		section.add_theme_constant_override("separation", 4)

		# Header row: name (bold) + reorder + remove buttons.
		var header := HBoxContainer.new()
		var name_label := Label.new()
		name_label.text = display_name
		name_label.add_theme_font_size_override("font_size", 15)
		name_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		header.add_child(name_label)

		var up_btn := Button.new()
		up_btn.text = "▲"
		up_btn.custom_minimum_size.x = 28
		up_btn.pressed.connect(_on_recipe_move_up.bind(ar_id))
		header.add_child(up_btn)

		var down_btn := Button.new()
		down_btn.text = "▼"
		down_btn.custom_minimum_size.x = 28
		down_btn.pressed.connect(_on_recipe_move_down.bind(ar_id))
		header.add_child(down_btn)

		var remove_btn := Button.new()
		remove_btn.text = "X"
		remove_btn.custom_minimum_size.x = 28
		remove_btn.pressed.connect(_on_recipe_remove.bind(ar_id))
		header.add_child(remove_btn)

		section.add_child(header)

		# Enabled toggle.
		var enabled_check := CheckButton.new()
		enabled_check.text = "Enabled"
		enabled_check.set_pressed_no_signal(enabled)
		enabled_check.toggled.connect(_on_recipe_enabled_toggled.bind(ar_id))
		section.add_child(enabled_check)

		# Per-output target rows.
		var target_widgets: Array = []
		for target in targets:
			var target_id: int = target.get("target_id", 0)
			var item_kind: String = target.get("item_kind", "?")
			var material: String = target.get("material", "")
			var target_qty: int = target.get("target_quantity", 0)
			var stock: int = target.get("stock", 0)

			var output_label_text := item_kind
			if material != "":
				output_label_text = material

			var target_row := HBoxContainer.new()
			var output_label := Label.new()
			output_label.text = "  %s:" % output_label_text
			output_label.custom_minimum_size.x = 80
			target_row.add_child(output_label)

			# Increment is the output quantity from the recipe def.
			# Use target quantity or a sensible default.
			var increment := maxi(1, int(target.get("target_quantity", 10)))
			# Use output quantity from catalog as increment if available.
			var recipe_output_qty := _get_output_quantity_for_target(
				ar.get("recipe_variant", 0), ar.get("material_json", ""), item_kind
			)
			if recipe_output_qty > 0:
				increment = recipe_output_qty

			var minus_btn := Button.new()
			minus_btn.text = "-%d" % increment
			minus_btn.custom_minimum_size.x = 40
			minus_btn.pressed.connect(_on_output_target_button.bind(target_id, -increment))
			target_row.add_child(minus_btn)

			var target_edit := LineEdit.new()
			target_edit.text = str(target_qty)
			target_edit.custom_minimum_size.x = 48
			target_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
			target_edit.alignment = HORIZONTAL_ALIGNMENT_CENTER
			target_edit.tooltip_text = "0 = don't craft"
			target_edit.text_submitted.connect(_on_output_target_submitted.bind(target_id))
			target_edit.focus_exited.connect(
				_on_output_target_focus_exited.bind(target_id, target_edit)
			)
			target_row.add_child(target_edit)

			var plus_btn := Button.new()
			plus_btn.text = "+%d" % increment
			plus_btn.custom_minimum_size.x = 40
			plus_btn.pressed.connect(_on_output_target_button.bind(target_id, increment))
			target_row.add_child(plus_btn)

			var stock_label := Label.new()
			stock_label.text = "(%d)" % stock
			target_row.add_child(stock_label)

			section.add_child(target_row)
			(
				target_widgets
				. append(
					{
						"target_id": target_id,
						"target_edit": target_edit,
						"stock_label": stock_label,
					}
				)
			)

		# Auto-logistics section.
		var auto_hbox := HBoxContainer.new()
		var auto_check := CheckButton.new()
		auto_check.text = "Auto-logistics"
		auto_check.set_pressed_no_signal(auto_logistics)
		auto_check.toggled.connect(_on_auto_logistics_toggled.bind(ar_id))
		auto_hbox.add_child(auto_check)

		var spare_label := Label.new()
		spare_label.text = "  Spare:"
		auto_hbox.add_child(spare_label)

		var spare_edit := LineEdit.new()
		spare_edit.text = str(spare_iterations)
		spare_edit.custom_minimum_size.x = 36
		spare_edit.alignment = HORIZONTAL_ALIGNMENT_CENTER
		spare_edit.editable = auto_logistics
		spare_edit.text_submitted.connect(_on_spare_iterations_submitted.bind(ar_id))
		spare_edit.focus_exited.connect(_on_spare_iterations_focus_exited.bind(ar_id, spare_edit))
		auto_hbox.add_child(spare_edit)

		section.add_child(auto_hbox)
		section.add_child(HSeparator.new())
		_crafting_active_recipes_vbox.add_child(section)

		_active_recipe_widgets[ar_id] = {
			"enabled_check": enabled_check,
			"auto_check": auto_check,
			"spare_edit": spare_edit,
			"target_widgets": target_widgets,
		}


## Update existing active recipe row widgets in-place.
func _refresh_active_recipe_rows(active_recipes: Array) -> void:
	# Check if the recipe list structure changed (added/removed/reordered).
	var current_ids: Array = []
	for ar in active_recipes:
		current_ids.append(ar.get("active_recipe_id", 0))
	var widget_ids: Array = _active_recipe_widgets.keys()
	if current_ids != widget_ids:
		# Structure changed — rebuild from scratch.
		_build_active_recipe_rows(active_recipes)
		return

	# In-place update of existing widgets.
	for ar in active_recipes:
		var ar_id: int = ar.get("active_recipe_id", 0)
		if not _active_recipe_widgets.has(ar_id):
			continue
		var w: Dictionary = _active_recipe_widgets[ar_id]

		var enabled_check: CheckButton = w["enabled_check"]
		enabled_check.set_pressed_no_signal(ar.get("enabled", false))

		var auto_check: CheckButton = w["auto_check"]
		auto_check.set_pressed_no_signal(ar.get("auto_logistics", true))

		var spare_edit: LineEdit = w["spare_edit"]
		spare_edit.editable = ar.get("auto_logistics", true)
		if not spare_edit.has_focus():
			spare_edit.text = str(ar.get("spare_iterations", 0))

		var targets: Array = ar.get("targets", [])
		var target_widgets: Array = w["target_widgets"]
		for i in range(mini(targets.size(), target_widgets.size())):
			var tw: Dictionary = target_widgets[i]
			var target_edit: LineEdit = tw["target_edit"]
			var stock_label: Label = tw["stock_label"]
			if not target_edit.has_focus():
				target_edit.text = str(targets[i].get("target_quantity", 0))
			stock_label.text = "(%d)" % targets[i].get("stock", 0)


## Get the output quantity for a target's item kind from the cached catalog.
## With the new Recipe enum system, we look up by variant + material.
func _get_output_quantity_for_target(
	_recipe_variant: int, _material_json: String, _item_kind: String
) -> int:
	# Output quantity is not cached in the available recipes list anymore
	# (it requires resolve() which happens server-side). Use 1 as default.
	return 1


func _on_crafting_details_pressed() -> void:
	_crafting_details_panel.visible = not _crafting_details_panel.visible
	# Hide the logistics panel — they share the same screen position.
	if _crafting_details_panel.visible:
		_logistics_details_panel.visible = false
	# Reset built flag so rows are rebuilt fresh when reopening.
	if _crafting_details_panel.visible:
		_recipe_rows_built = false
		_active_recipe_widgets.clear()
		_recipe_picker_visible = false
		_recipe_search_edit.visible = false
		_recipe_search_edit.text = ""


func _on_crafting_enabled_toggled(pressed: bool) -> void:
	if _current_structure_id < 0:
		return
	crafting_enabled_changed.emit(_current_structure_id, pressed)


func _on_add_recipe_pressed() -> void:
	_recipe_picker_visible = not _recipe_picker_visible
	_crafting_recipe_picker.visible = _recipe_picker_visible
	_recipe_search_edit.visible = _recipe_picker_visible
	if _recipe_picker_visible:
		_recipe_search_edit.text = ""
		_populate_recipe_picker()
		_recipe_search_edit.grab_focus()
	else:
		_recipe_search_edit.text = ""


func _on_recipe_search_changed(_new_text: String) -> void:
	_populate_recipe_picker()
	if _crafting_recipe_picker.visible:
		_refresh_recipe_picker_availability()


## Return true if `recipe` matches the current search filter (case-insensitive
## substring match against display name, input item kinds, and output item kinds).
func _recipe_matches_filter(recipe: Dictionary, filter_lower: String) -> bool:
	if filter_lower.is_empty():
		return true
	var display_name: String = recipe.get("display_name", "")
	if display_name.to_lower().contains(filter_lower):
		return true
	for inp in recipe.get("inputs", []):
		if inp.get("item_kind", "").to_lower().contains(filter_lower):
			return true
	for out in recipe.get("outputs", []):
		if out.get("item_kind", "").to_lower().contains(filter_lower):
			return true
	return false


func _populate_recipe_picker() -> void:
	for child in _crafting_recipe_picker.get_children():
		child.queue_free()

	var filter_lower: String = _recipe_search_edit.text.strip_edges().to_lower()

	# Group recipes by top-level category.
	var categorized: Dictionary = {}  # category_name -> Array of recipes
	var root_recipes: Array = []  # recipes with empty category
	for recipe in _cached_building_recipes:
		if not _recipe_matches_filter(recipe, filter_lower):
			continue
		var category: Array = recipe.get("category", [])
		if category.size() == 0:
			root_recipes.append(recipe)
		else:
			var top_cat: String = category[0]
			if not categorized.has(top_cat):
				categorized[top_cat] = []
			categorized[top_cat].append(recipe)

	# Auto-expand if all recipes share a single top-level category,
	# or if a search filter is active (so matches aren't hidden).
	var auto_expand := root_recipes.size() == 0 and categorized.size() == 1
	var filter_active := not filter_lower.is_empty()

	# Initialize collapsed state for any new categories (default collapsed).
	for cat_name: String in categorized:
		if not _recipe_category_collapsed.has(cat_name):
			_recipe_category_collapsed[cat_name] = not auto_expand

	# Add root-level recipes (no category) first.
	for recipe in root_recipes:
		_crafting_recipe_picker.add_child(_make_recipe_button(recipe))

	# Add categorized recipes as collapsible groups.
	var sorted_cats: Array = categorized.keys()
	sorted_cats.sort()
	for cat_name: String in sorted_cats:
		var recipes: Array = categorized[cat_name]
		var is_collapsed: bool = (
			false if filter_active else _recipe_category_collapsed.get(cat_name, true)
		)

		# Subcategorize within this top-level category.
		var subcategorized: Dictionary = {}  # subcat_name -> Array of recipes
		var direct_recipes: Array = []  # recipes with only the top-level category
		for recipe in recipes:
			var category: Array = recipe.get("category", [])
			if category.size() >= 2:
				var sub_cat: String = category[1]
				if not subcategorized.has(sub_cat):
					subcategorized[sub_cat] = []
				subcategorized[sub_cat].append(recipe)
			else:
				direct_recipes.append(recipe)

		# Category header button with expand/collapse indicator.
		var header := Button.new()
		var arrow := "▶" if is_collapsed else "▼"
		header.text = "%s %s (%d)" % [arrow, cat_name, recipes.size()]
		header.alignment = HORIZONTAL_ALIGNMENT_LEFT
		header.pressed.connect(_on_recipe_category_toggled.bind(cat_name))
		_crafting_recipe_picker.add_child(header)

		# Container for this category's recipes, hidden when collapsed.
		var cat_vbox := VBoxContainer.new()
		cat_vbox.visible = not is_collapsed
		cat_vbox.add_theme_constant_override("separation", 2)
		cat_vbox.set_meta("category", cat_name)
		_crafting_recipe_picker.add_child(cat_vbox)

		# Indent the contents.
		var margin := MarginContainer.new()
		margin.add_theme_constant_override("margin_left", 16)
		cat_vbox.add_child(margin)

		var inner_vbox := VBoxContainer.new()
		inner_vbox.add_theme_constant_override("separation", 2)
		margin.add_child(inner_vbox)

		# Direct recipes (single-level category) first.
		for recipe in direct_recipes:
			inner_vbox.add_child(_make_recipe_button(recipe))

		# Subcategory groups.
		var sorted_subcats: Array = subcategorized.keys()
		sorted_subcats.sort()
		for sub_name: String in sorted_subcats:
			var sub_recipes: Array = subcategorized[sub_name]
			var sub_label := Label.new()
			sub_label.text = "%s (%d)" % [sub_name, sub_recipes.size()]
			sub_label.add_theme_font_size_override("font_size", 13)
			inner_vbox.add_child(sub_label)
			var sub_margin := MarginContainer.new()
			sub_margin.add_theme_constant_override("margin_left", 12)
			inner_vbox.add_child(sub_margin)
			var sub_vbox := VBoxContainer.new()
			sub_vbox.add_theme_constant_override("separation", 2)
			sub_margin.add_child(sub_vbox)
			for recipe in sub_recipes:
				sub_vbox.add_child(_make_recipe_button(recipe))


## Build recipe buttons for a template — one per valid material.
func _make_recipe_button(recipe: Dictionary) -> VBoxContainer:
	var container := VBoxContainer.new()
	container.add_theme_constant_override("separation", 1)
	var materials: Array = recipe.get("materials", [])
	var recipe_variant: int = recipe.get("recipe", 0)
	for mat_entry in materials:
		var display_name: String = mat_entry.get("display_name", "?")
		var material_json: String = mat_entry.get("material_json", "")
		var btn := Button.new()
		btn.text = display_name
		var unique_key := "%d|%s" % [recipe_variant, material_json]
		btn.set_meta("recipe_key", unique_key)
		btn.pressed.connect(_on_recipe_picker_selected.bind(recipe_variant, material_json))
		container.add_child(btn)
	return container


## Toggle a recipe category's collapsed state and rebuild the picker.
func _on_recipe_category_toggled(cat_name: String) -> void:
	_recipe_category_collapsed[cat_name] = not _recipe_category_collapsed.get(cat_name, true)
	_populate_recipe_picker()
	# Re-apply availability graying after rebuild.
	if _crafting_recipe_picker.visible:
		_refresh_recipe_picker_availability()


func _update_recipe_picker_availability(active_recipes: Array) -> void:
	if not _crafting_recipe_picker.visible:
		return
	# Collect and cache active recipe keys as "variant|material_json".
	_cached_active_recipe_keys = []
	for ar in active_recipes:
		var variant: int = ar.get("recipe_variant", 0)
		var mat_json: String = ar.get("material_json", "")
		_cached_active_recipe_keys.append("%d|%s" % [variant, mat_json])
	# Gray out recipes already active (recurse into category containers).
	_disable_active_recipe_buttons(_crafting_recipe_picker, _cached_active_recipe_keys)


## Re-apply recipe availability using cached active keys (after tree rebuild).
func _refresh_recipe_picker_availability() -> void:
	_disable_active_recipe_buttons(_crafting_recipe_picker, _cached_active_recipe_keys)


## Recursively find recipe buttons in the picker tree and disable active ones.
func _disable_active_recipe_buttons(node: Node, active_keys: Array) -> void:
	for child in node.get_children():
		if child is Button and child.has_meta("recipe_key"):
			var key: String = child.get_meta("recipe_key")
			child.disabled = active_keys.has(key)
		elif child is Container:
			_disable_active_recipe_buttons(child, active_keys)


func _on_recipe_picker_selected(recipe_variant: int, material_json: String) -> void:
	if _current_structure_id < 0:
		return
	add_recipe_requested.emit(_current_structure_id, recipe_variant, material_json)
	_recipe_picker_visible = false
	_crafting_recipe_picker.visible = false
	_recipe_search_edit.visible = false
	_recipe_search_edit.text = ""


func _on_recipe_remove(active_recipe_id: int) -> void:
	remove_recipe_requested.emit(active_recipe_id)


func _on_recipe_move_up(active_recipe_id: int) -> void:
	recipe_move_up_requested.emit(active_recipe_id)


func _on_recipe_move_down(active_recipe_id: int) -> void:
	recipe_move_down_requested.emit(active_recipe_id)


func _on_recipe_enabled_toggled(pressed: bool, active_recipe_id: int) -> void:
	recipe_enabled_changed.emit(active_recipe_id, pressed)


func _on_output_target_button(target_id: int, delta: int) -> void:
	# Find the widget and adjust value.
	for ar_id in _active_recipe_widgets:
		var w: Dictionary = _active_recipe_widgets[ar_id]
		for tw in w["target_widgets"]:
			if tw["target_id"] == target_id:
				var target_edit: LineEdit = tw["target_edit"]
				var current := maxi(0, int(target_edit.text))
				var new_val := maxi(0, current + delta)
				target_edit.text = str(new_val)
				recipe_output_target_changed.emit(target_id, new_val)
				return


func _on_output_target_submitted(_text: String, target_id: int) -> void:
	for ar_id in _active_recipe_widgets:
		var w: Dictionary = _active_recipe_widgets[ar_id]
		for tw in w["target_widgets"]:
			if tw["target_id"] == target_id:
				var target_edit: LineEdit = tw["target_edit"]
				var val := maxi(0, int(target_edit.text))
				target_edit.text = str(val)
				target_edit.release_focus()
				return


func _on_output_target_focus_exited(target_id: int, target_edit: LineEdit) -> void:
	var val := maxi(0, int(target_edit.text))
	target_edit.text = str(val)
	recipe_output_target_changed.emit(target_id, val)


func _on_auto_logistics_toggled(pressed: bool, active_recipe_id: int) -> void:
	# Get current spare iterations from widget.
	if not _active_recipe_widgets.has(active_recipe_id):
		return
	var w: Dictionary = _active_recipe_widgets[active_recipe_id]
	var spare_edit: LineEdit = w["spare_edit"]
	spare_edit.editable = pressed
	var spare := maxi(0, int(spare_edit.text))
	recipe_auto_logistics_changed.emit(active_recipe_id, pressed, spare)


func _on_spare_iterations_submitted(_text: String, active_recipe_id: int) -> void:
	if not _active_recipe_widgets.has(active_recipe_id):
		return
	var w: Dictionary = _active_recipe_widgets[active_recipe_id]
	var spare_edit: LineEdit = w["spare_edit"]
	var val := maxi(0, int(spare_edit.text))
	spare_edit.text = str(val)
	spare_edit.release_focus()


func _on_spare_iterations_focus_exited(active_recipe_id: int, spare_edit: LineEdit) -> void:
	if not _active_recipe_widgets.has(active_recipe_id):
		return
	var val := maxi(0, int(spare_edit.text))
	spare_edit.text = str(val)
	var auto_check: CheckButton = _active_recipe_widgets[active_recipe_id]["auto_check"]
	recipe_auto_logistics_changed.emit(active_recipe_id, auto_check.button_pressed, val)


func _on_logistics_details_pressed() -> void:
	_logistics_details_panel.visible = not _logistics_details_panel.visible
	# Hide the crafting panel — they share the same screen position.
	if _logistics_details_panel.visible:
		_crafting_details_panel.visible = false


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


func _on_logistics_wants_editor_changed(wants_json: String) -> void:
	if _current_structure_id < 0:
		return
	logistics_wants_changed.emit(_current_structure_id, wants_json)


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
	_greenhouse_picker_scroll.visible = false


func _on_furnishing_type_pressed(type_id: String) -> void:
	_furnish_picker.visible = false
	if _current_structure_id < 0:
		return
	if type_id == "Greenhouse":
		_show_greenhouse_species_picker()
	else:
		furnish_requested.emit(_current_structure_id, type_id, -1)


func _show_greenhouse_species_picker() -> void:
	# Populate the species picker from cached cultivable fruits.
	for child in _greenhouse_picker_vbox.get_children():
		child.queue_free()

	if _cached_cultivable_fruits.is_empty():
		var lbl := Label.new()
		lbl.text = "No cultivable fruit species available."
		lbl.add_theme_font_size_override("font_size", 12)
		_greenhouse_picker_vbox.add_child(lbl)
	else:
		var header := Label.new()
		header.text = "Select fruit species:"
		header.add_theme_font_size_override("font_size", 12)
		_greenhouse_picker_vbox.add_child(header)
		for fruit in _cached_cultivable_fruits:
			var btn := Button.new()
			btn.text = fruit["name"]
			var fid: int = fruit["id"]
			btn.pressed.connect(_on_greenhouse_species_selected.bind(fid))
			_greenhouse_picker_vbox.add_child(btn)

	_greenhouse_picker_scroll.visible = true


func _on_greenhouse_species_selected(species_id: int) -> void:
	_greenhouse_picker_scroll.visible = false
	if _current_structure_id >= 0:
		furnish_requested.emit(_current_structure_id, "Greenhouse", species_id)


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
