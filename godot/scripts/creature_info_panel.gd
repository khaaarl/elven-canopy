## Creature info panel displayed on the right side of the screen.
##
## Shows information about the currently selected creature, organized into
## three tabs: Status (vitals, position, task, needs, mood, ability scores),
## Inventory (scrollable item list), and Thoughts (scrollable recent thoughts).
##
## The panel is ~25% screen width, full height, anchored to the right edge.
## A fixed header (species, name, status, military group) and Follow button
## sit outside the tabs so they're always visible. Updated every frame by
## main.gd.
##
## See also: selection_controller.gd which triggers show/hide,
## orbital_camera.gd which responds to follow/unfollow,
## main.gd which wires everything together.

extends PanelContainer

signal follow_requested
signal unfollow_requested
signal panel_closed
signal zoom_to_task_location(x: float, y: float, z: float)
signal military_group_clicked(group_id: int)

const MAX_DISPLAYED_THOUGHTS := 10

## Index constants for the three tabs.
const TAB_STATUS := 0
const TAB_INVENTORY := 1
const TAB_THOUGHTS := 2

var _species_label: Label
var _name_label: Label
var _position_label: Label
var _task_row: HBoxContainer
var _task_label: Label
var _task_zoom_btn: Button
var _hp_bar: ProgressBar
var _hp_label: Label
var _food_bar: ProgressBar
var _food_label: Label
var _rest_bar: ProgressBar
var _rest_label: Label
var _mp_bar: ProgressBar
var _mp_label: Label
var _mp_row: HBoxContainer
var _status_label: Label
var _military_group_btn: Button
var _military_group_id: int = -1
var _mood_label: Label
var _stat_labels: Dictionary = {}
var _thoughts_container: VBoxContainer
var _inventory_label: Label
var _follow_button: Button
var _is_following: bool = false
var _selected_creature_id: String = ""

## Tab switching state.
var _tab_buttons: Array[Button] = []
var _tab_contents: Array[Control] = []
var _active_tab: int = TAB_STATUS


func _ready() -> void:
	# Anchor to the right edge, full height.
	set_anchors_preset(PRESET_RIGHT_WIDE)
	custom_minimum_size.x = 320
	# PanelContainer shrinks to content minimum, and ScrollContainer has zero
	# minimum height — force full viewport height so the scroll area is visible.
	_match_viewport_height()
	get_viewport().size_changed.connect(_match_viewport_height)

	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	margin.add_theme_constant_override("margin_right", 12)
	margin.add_theme_constant_override("margin_top", 12)
	margin.add_theme_constant_override("margin_bottom", 12)
	add_child(margin)

	var root_vbox := VBoxContainer.new()
	root_vbox.add_theme_constant_override("separation", 8)
	margin.add_child(root_vbox)

	# -- Fixed header (always visible) --
	_build_header(root_vbox)

	# -- Tab bar --
	var tab_bar := HBoxContainer.new()
	tab_bar.add_theme_constant_override("separation", 4)
	root_vbox.add_child(tab_bar)

	for tab_name in ["Status", "Inventory", "Thoughts"]:
		var btn := Button.new()
		btn.text = tab_name
		btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		btn.pressed.connect(_on_tab_pressed.bind(_tab_buttons.size()))
		tab_bar.add_child(btn)
		_tab_buttons.append(btn)

	root_vbox.add_child(HSeparator.new())

	# -- Tab contents (direct children of root_vbox, only one visible) --
	_tab_contents.append(_build_status_tab(root_vbox))
	_tab_contents.append(_build_inventory_tab(root_vbox))
	_tab_contents.append(_build_thoughts_tab(root_vbox))

	# -- Follow button (always visible) --
	_follow_button = Button.new()
	_follow_button.text = "Follow"
	_follow_button.pressed.connect(_on_follow_pressed)
	root_vbox.add_child(_follow_button)

	_switch_tab(TAB_STATUS)
	visible = false


## Build the fixed header: title row, species, name, status, military group.
func _build_header(parent: VBoxContainer) -> void:
	var header := HBoxContainer.new()
	parent.add_child(header)

	var title := Label.new()
	title.text = "Creature Info"
	title.add_theme_font_size_override("font_size", 20)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close_pressed)
	header.add_child(close_btn)

	parent.add_child(HSeparator.new())

	_species_label = Label.new()
	parent.add_child(_species_label)

	_name_label = Label.new()
	parent.add_child(_name_label)

	_status_label = Label.new()
	_status_label.text = "INCAPACITATED"
	_status_label.add_theme_color_override("font_color", Color(0.9, 0.3, 0.3))
	_status_label.add_theme_font_size_override("font_size", 16)
	_status_label.visible = false
	parent.add_child(_status_label)

	_military_group_btn = Button.new()
	_military_group_btn.alignment = HORIZONTAL_ALIGNMENT_LEFT
	_military_group_btn.flat = true
	_military_group_btn.visible = false
	_military_group_btn.pressed.connect(_on_military_group_clicked)
	parent.add_child(_military_group_btn)


## Build the Status tab: HP, MP, position, task, food, rest, mood, stats grid.
func _build_status_tab(parent: VBoxContainer) -> ScrollContainer:
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	parent.add_child(scroll)

	var vbox := VBoxContainer.new()
	vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.add_theme_constant_override("separation", 8)
	scroll.add_child(vbox)

	# HP gauge.
	var hp_row := HBoxContainer.new()
	hp_row.add_theme_constant_override("separation", 6)
	vbox.add_child(hp_row)

	var hp_title := Label.new()
	hp_title.text = "HP:"
	hp_row.add_child(hp_title)

	_hp_bar = ProgressBar.new()
	_hp_bar.min_value = 0.0
	_hp_bar.max_value = 100.0
	_hp_bar.value = 100.0
	_hp_bar.show_percentage = false
	_hp_bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_hp_bar.custom_minimum_size.y = 20
	var hp_style := StyleBoxFlat.new()
	hp_style.bg_color = Color(0.6, 0.15, 0.1)
	_hp_bar.add_theme_stylebox_override("fill", hp_style)
	hp_row.add_child(_hp_bar)

	_hp_label = Label.new()
	_hp_label.text = "0 / 0"
	_hp_label.custom_minimum_size.x = 70
	_hp_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	hp_row.add_child(_hp_label)

	# MP gauge — hidden for nonmagical creatures.
	_mp_row = HBoxContainer.new()
	_mp_row.add_theme_constant_override("separation", 6)
	_mp_row.visible = false
	vbox.add_child(_mp_row)

	var mp_title := Label.new()
	mp_title.text = "MP:"
	_mp_row.add_child(mp_title)

	_mp_bar = ProgressBar.new()
	_mp_bar.min_value = 0.0
	_mp_bar.max_value = 100.0
	_mp_bar.value = 100.0
	_mp_bar.show_percentage = false
	_mp_bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_mp_bar.custom_minimum_size.y = 20
	var mp_style := StyleBoxFlat.new()
	mp_style.bg_color = Color(0.15, 0.25, 0.7)
	_mp_bar.add_theme_stylebox_override("fill", mp_style)
	_mp_row.add_child(_mp_bar)

	_mp_label = Label.new()
	_mp_label.text = "0 / 0"
	_mp_label.custom_minimum_size.x = 70
	_mp_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	_mp_row.add_child(_mp_label)

	# Position.
	_position_label = Label.new()
	vbox.add_child(_position_label)

	# Task status row.
	_task_row = HBoxContainer.new()
	_task_row.add_theme_constant_override("separation", 6)
	vbox.add_child(_task_row)

	_task_label = Label.new()
	_task_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_task_row.add_child(_task_label)

	_task_zoom_btn = Button.new()
	_task_zoom_btn.text = "Zoom"
	_task_zoom_btn.visible = false
	_task_zoom_btn.pressed.connect(_on_task_zoom_pressed)
	_task_row.add_child(_task_zoom_btn)

	# Food gauge.
	var food_row := HBoxContainer.new()
	food_row.add_theme_constant_override("separation", 6)
	vbox.add_child(food_row)

	var food_title := Label.new()
	food_title.text = "Food:"
	food_row.add_child(food_title)

	_food_bar = ProgressBar.new()
	_food_bar.min_value = 0.0
	_food_bar.max_value = 100.0
	_food_bar.value = 100.0
	_food_bar.show_percentage = false
	_food_bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_food_bar.custom_minimum_size.y = 20
	food_row.add_child(_food_bar)

	_food_label = Label.new()
	_food_label.text = "100%"
	_food_label.custom_minimum_size.x = 45
	_food_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	food_row.add_child(_food_label)

	# Rest gauge.
	var rest_row := HBoxContainer.new()
	rest_row.add_theme_constant_override("separation", 6)
	vbox.add_child(rest_row)

	var rest_title := Label.new()
	rest_title.text = "Rest:"
	rest_row.add_child(rest_title)

	_rest_bar = ProgressBar.new()
	_rest_bar.min_value = 0.0
	_rest_bar.max_value = 100.0
	_rest_bar.value = 100.0
	_rest_bar.show_percentage = false
	_rest_bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_rest_bar.custom_minimum_size.y = 20
	rest_row.add_child(_rest_bar)

	_rest_label = Label.new()
	_rest_label.text = "100%"
	_rest_label.custom_minimum_size.x = 45
	_rest_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	rest_row.add_child(_rest_label)

	# Mood label.
	_mood_label = Label.new()
	_mood_label.text = "Mood: Neutral (0)"
	vbox.add_child(_mood_label)

	# Ability scores grid (4 rows × 2 columns).
	vbox.add_child(HSeparator.new())
	_build_stats_grid(vbox)

	return scroll


## Build the 4×2 ability scores grid: DEX/AGI, STR/CON, WIL/INT, PER/CHA.
func _build_stats_grid(parent: VBoxContainer) -> void:
	var grid := GridContainer.new()
	grid.columns = 4
	grid.add_theme_constant_override("h_separation", 4)
	grid.add_theme_constant_override("v_separation", 2)
	parent.add_child(grid)

	# Each stat is two cells: a fixed-width abbreviation label + a value label.
	var stat_order: Array[String] = [
		"stat_dex",
		"stat_agi",
		"stat_str",
		"stat_con",
		"stat_wil",
		"stat_int",
		"stat_per",
		"stat_cha",
	]
	var abbrevs: Dictionary = {
		"stat_dex": "DEX",
		"stat_agi": "AGI",
		"stat_str": "STR",
		"stat_con": "CON",
		"stat_wil": "WIL",
		"stat_int": "INT",
		"stat_per": "PER",
		"stat_cha": "CHA",
	}

	for key in stat_order:
		var abbr_lbl := Label.new()
		abbr_lbl.text = abbrevs[key]
		abbr_lbl.add_theme_font_size_override("font_size", 13)
		abbr_lbl.add_theme_color_override("font_color", Color(0.7, 0.7, 0.7))
		abbr_lbl.custom_minimum_size.x = 36
		grid.add_child(abbr_lbl)

		var val_lbl := Label.new()
		val_lbl.text = "0"
		val_lbl.add_theme_font_size_override("font_size", 13)
		val_lbl.custom_minimum_size.x = 40
		val_lbl.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
		val_lbl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		grid.add_child(val_lbl)

		_stat_labels[key] = val_lbl


## Build the Inventory tab: scrollable item list.
func _build_inventory_tab(parent: VBoxContainer) -> ScrollContainer:
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	parent.add_child(scroll)

	var vbox := VBoxContainer.new()
	vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.add_theme_constant_override("separation", 4)
	scroll.add_child(vbox)

	_inventory_label = Label.new()
	_inventory_label.text = "(empty)"
	vbox.add_child(_inventory_label)

	return scroll


## Build the Thoughts tab: scrollable thought list.
func _build_thoughts_tab(parent: VBoxContainer) -> ScrollContainer:
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	parent.add_child(scroll)

	_thoughts_container = VBoxContainer.new()
	_thoughts_container.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_thoughts_container.add_theme_constant_override("separation", 2)
	scroll.add_child(_thoughts_container)

	return scroll


func _switch_tab(index: int) -> void:
	_active_tab = index
	for i in range(_tab_contents.size()):
		_tab_contents[i].visible = (i == index)
	for i in range(_tab_buttons.size()):
		_tab_buttons[i].disabled = (i == index)


func _on_tab_pressed(index: int) -> void:
	_switch_tab(index)


func show_creature(creature_id: String, info: Dictionary) -> void:
	_selected_creature_id = creature_id
	var species: String = info.get("species", "")
	_species_label.text = "Species: %s" % species
	var creature_name: String = info.get("name", "")
	if creature_name.is_empty():
		_name_label.text = "Name: %s" % species
	else:
		var meaning: String = info.get("name_meaning", "")
		if meaning.is_empty():
			_name_label.text = "Name: %s" % creature_name
		else:
			_name_label.text = "Name: %s (%s)" % [creature_name, meaning]
	_update_status(info)
	_update_hp(info)
	_update_mp(info)
	_update_position(info)
	_update_task(info)
	_update_food(info)
	_update_rest(info)
	_update_mood(info)
	_update_stats(info)
	_update_thoughts(info)
	_update_inventory(info)
	_update_military_group(info)
	if _is_following:
		unfollow_requested.emit()
	_is_following = false
	_follow_button.text = "Follow"
	visible = true


func update_info(info: Dictionary) -> void:
	_update_status(info)
	_update_hp(info)
	_update_mp(info)
	_update_position(info)
	_update_task(info)
	_update_food(info)
	_update_rest(info)
	_update_mood(info)
	_update_stats(info)
	_update_thoughts(info)
	_update_inventory(info)
	_update_military_group(info)


func hide_panel() -> void:
	if _is_following:
		unfollow_requested.emit()
	_is_following = false
	_follow_button.text = "Follow"
	_switch_tab(TAB_STATUS)
	visible = false


func set_follow_state(following: bool) -> void:
	_is_following = following
	_follow_button.text = "Unfollow" if following else "Follow"


func _update_hp(info: Dictionary) -> void:
	var hp: int = info.get("hp", 0)
	var hp_max: int = info.get("hp_max", 1)
	if hp_max <= 0:
		hp_max = 1
	var is_incap: bool = info.get("incapacitated", false)
	if is_incap:
		var hp_style := StyleBoxFlat.new()
		hp_style.bg_color = Color(0.35, 0.35, 0.35)
		_hp_bar.add_theme_stylebox_override("fill", hp_style)
		_hp_bar.value = 50.0
	else:
		var hp_style := StyleBoxFlat.new()
		hp_style.bg_color = Color(0.6, 0.15, 0.1)
		_hp_bar.add_theme_stylebox_override("fill", hp_style)
		var pct: float = 100.0 * float(hp) / float(hp_max)
		_hp_bar.value = pct
	_hp_label.text = "%d / %d" % [hp, hp_max]


func _update_status(info: Dictionary) -> void:
	var is_incap: bool = info.get("incapacitated", false)
	_status_label.visible = is_incap


func _update_mp(info: Dictionary) -> void:
	var mp_max: int = info.get("mp_max", 0)
	if mp_max <= 0:
		_mp_row.visible = false
		return
	_mp_row.visible = true
	var mp: int = info.get("mp", 0)
	var pct: float = 100.0 * float(mp) / float(mp_max)
	_mp_bar.value = pct
	_mp_label.text = "%d%%" % int(pct)


func _update_food(info: Dictionary) -> void:
	var food_max: int = info.get("food_max", 1)
	if food_max <= 0:
		food_max = 1
	var pct: float = 100.0 * float(info.get("food", 0)) / float(food_max)
	_food_bar.value = pct
	_food_label.text = "%d%%" % int(pct)


func _update_rest(info: Dictionary) -> void:
	var rest_max: int = info.get("rest_max", 1)
	if rest_max <= 0:
		rest_max = 1
	var pct: float = 100.0 * float(info.get("rest", 0)) / float(rest_max)
	_rest_bar.value = pct
	_rest_label.text = "%d%%" % int(pct)


func _update_mood(info: Dictionary) -> void:
	var tier: String = info.get("mood_tier", "Neutral")
	var score: int = info.get("mood_score", 0)
	var sign: String = "+" if score >= 0 else ""
	_mood_label.text = "Mood: %s (%s%d)" % [tier, sign, score]


func _update_stats(info: Dictionary) -> void:
	for key in _stat_labels:
		var val: int = info.get(key, 0)
		_stat_labels[key].text = "%d" % val


func _update_thoughts(info: Dictionary) -> void:
	var thoughts: Array = info.get("thoughts", [])
	var display_count := mini(thoughts.size(), MAX_DISPLAYED_THOUGHTS)
	# Reuse existing labels where possible, add new ones if needed.
	while _thoughts_container.get_child_count() < display_count:
		var lbl := Label.new()
		lbl.add_theme_font_size_override("font_size", 13)
		lbl.add_theme_color_override("font_color", Color(0.8, 0.8, 0.8))
		_thoughts_container.add_child(lbl)
	# Update visible labels.
	for i in range(display_count):
		var lbl: Label = _thoughts_container.get_child(i)
		var thought: Dictionary = thoughts[i]
		lbl.text = "- %s" % thought.get("text", "")
		lbl.visible = true
	# Hide excess labels.
	for i in range(display_count, _thoughts_container.get_child_count()):
		_thoughts_container.get_child(i).visible = false


func _update_position(info: Dictionary) -> void:
	_position_label.text = (
		"Position: (%d, %d, %d)" % [info.get("x", 0), info.get("y", 0), info.get("z", 0)]
	)


func _update_task(info: Dictionary) -> void:
	var has_task: bool = info.get("has_task", false)
	if has_task:
		var kind: String = info.get("task_kind", "")
		_task_label.text = "Task: %s" % kind if not kind.is_empty() else "Task: (unknown)"
		var has_loc: bool = info.has("task_location_x")
		_task_zoom_btn.visible = has_loc
		if has_loc:
			_task_zoom_btn.set_meta("tx", float(info.get("task_location_x", 0)))
			_task_zoom_btn.set_meta("ty", float(info.get("task_location_y", 0)))
			_task_zoom_btn.set_meta("tz", float(info.get("task_location_z", 0)))
	else:
		_task_label.text = "Task: none"
		_task_zoom_btn.visible = false


func _on_task_zoom_pressed() -> void:
	var tx: float = _task_zoom_btn.get_meta("tx", 0.0)
	var ty: float = _task_zoom_btn.get_meta("ty", 0.0)
	var tz: float = _task_zoom_btn.get_meta("tz", 0.0)
	zoom_to_task_location.emit(tx, ty, tz)


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


func _on_follow_pressed() -> void:
	if _is_following:
		_is_following = false
		_follow_button.text = "Follow"
		unfollow_requested.emit()
	else:
		_is_following = true
		_follow_button.text = "Unfollow"
		follow_requested.emit()


func _update_military_group(info: Dictionary) -> void:
	var group_name: String = info.get("military_group_name", "")
	var group_id: int = info.get("military_group_id", -1)
	if group_name.is_empty() or group_id < 0:
		_military_group_btn.visible = false
		_military_group_id = -1
	else:
		_military_group_btn.text = "Group: %s" % group_name
		_military_group_btn.visible = true
		_military_group_id = group_id


func _on_military_group_clicked() -> void:
	if _military_group_id >= 0:
		military_group_clicked.emit(_military_group_id)


func _match_viewport_height() -> void:
	custom_minimum_size.y = get_viewport().get_visible_rect().size.y


func _on_close_pressed() -> void:
	hide_panel()
	panel_closed.emit()
