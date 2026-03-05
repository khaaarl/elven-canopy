## Creature info panel displayed on the right side of the screen.
##
## Shows information about the currently selected creature. Built
## programmatically as a PanelContainer with labels and a Follow button.
## Sits on the CanvasLayer alongside the spawn toolbar.
##
## The panel is ~25% screen width, full height, anchored to the right edge.
## Shows species, name (Vaelith name for elves, fallback "Species #N" for
## unnamed creatures), position, task kind with a "Zoom" button to jump to
## the task's target location (when available), a food gauge, a rest gauge
## (both as progress bar + percentage), a mood label showing the derived
## mood tier and numeric score, a "Recent Thoughts" section listing the
## creature's accumulated thoughts (most recent first), and an inventory
## section listing carried items. Updated every frame by main.gd.
##
## See also: selection_controller.gd which triggers show/hide,
## orbital_camera.gd which responds to follow/unfollow,
## main.gd which wires everything together.

extends PanelContainer

signal follow_requested
signal unfollow_requested
signal panel_closed
signal zoom_to_task_location(x: float, y: float, z: float)

const MAX_DISPLAYED_THOUGHTS := 10

var _species_label: Label
var _name_label: Label
var _position_label: Label
var _task_row: HBoxContainer
var _task_label: Label
var _task_zoom_btn: Button
var _food_bar: ProgressBar
var _food_label: Label
var _rest_bar: ProgressBar
var _rest_label: Label
var _mood_label: Label
var _thoughts_container: VBoxContainer
var _thoughts_header: Label
var _inventory_label: Label
var _follow_button: Button
var _is_following: bool = false
var _selected_species: String = ""
var _selected_index: int = -1


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
	title.text = "Creature Info"
	title.add_theme_font_size_override("font_size", 20)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close_pressed)
	header.add_child(close_btn)

	# Separator.
	vbox.add_child(HSeparator.new())

	# Species.
	_species_label = Label.new()
	vbox.add_child(_species_label)

	# Name (Vaelith name for elves, fallback "Species #N" for unnamed creatures).
	_name_label = Label.new()
	vbox.add_child(_name_label)

	# Position.
	_position_label = Label.new()
	vbox.add_child(_position_label)

	# Task status row (label + zoom button).
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

	# Recent Thoughts section.
	vbox.add_child(HSeparator.new())

	_thoughts_header = Label.new()
	_thoughts_header.text = "Recent Thoughts"
	_thoughts_header.add_theme_font_size_override("font_size", 16)
	vbox.add_child(_thoughts_header)

	_thoughts_container = VBoxContainer.new()
	_thoughts_container.add_theme_constant_override("separation", 2)
	vbox.add_child(_thoughts_container)

	# Inventory section.
	vbox.add_child(HSeparator.new())

	var inv_title := Label.new()
	inv_title.text = "Inventory"
	inv_title.add_theme_font_size_override("font_size", 16)
	vbox.add_child(inv_title)

	_inventory_label = Label.new()
	_inventory_label.text = "(empty)"
	vbox.add_child(_inventory_label)

	# Spacer to push the follow button toward the bottom-ish area.
	var spacer := Control.new()
	spacer.size_flags_vertical = Control.SIZE_EXPAND_FILL
	vbox.add_child(spacer)

	# Follow / Unfollow button.
	_follow_button = Button.new()
	_follow_button.text = "Follow"
	_follow_button.pressed.connect(_on_follow_pressed)
	vbox.add_child(_follow_button)

	visible = false


func show_creature(species: String, index: int, info: Dictionary) -> void:
	_selected_species = species
	_selected_index = index
	_species_label.text = "Species: %s" % species
	var creature_name: String = info.get("name", "")
	if creature_name.is_empty():
		_name_label.text = "Name: %s #%d" % [species, index + 1]
	else:
		var meaning: String = info.get("name_meaning", "")
		if meaning.is_empty():
			_name_label.text = "Name: %s" % creature_name
		else:
			_name_label.text = "Name: %s (%s)" % [creature_name, meaning]
	_update_position(info)
	_update_task(info)
	_update_food(info)
	_update_rest(info)
	_update_mood(info)
	_update_thoughts(info)
	_update_inventory(info)
	_is_following = false
	_follow_button.text = "Follow"
	visible = true


func update_info(info: Dictionary) -> void:
	_update_position(info)
	_update_task(info)
	_update_food(info)
	_update_rest(info)
	_update_mood(info)
	_update_thoughts(info)
	_update_inventory(info)


func hide_panel() -> void:
	if _is_following:
		unfollow_requested.emit()
	_is_following = false
	_follow_button.text = "Follow"
	visible = false


func set_follow_state(following: bool) -> void:
	_is_following = following
	_follow_button.text = "Unfollow" if following else "Follow"


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
	# Show/hide the header based on whether there are any thoughts.
	_thoughts_header.visible = display_count > 0


func _update_position(info: Dictionary) -> void:
	_position_label.text = (
		"Position: (%d, %d, %d)" % [info.get("x", 0), info.get("y", 0), info.get("z", 0)]
	)


func _update_task(info: Dictionary) -> void:
	var has_task: bool = info.get("has_task", false)
	if has_task:
		var kind: String = info.get("task_kind", "")
		_task_label.text = "Task: %s" % kind if not kind.is_empty() else "Task: (unknown)"
		# Show zoom button only when location data is present.
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


func _on_close_pressed() -> void:
	hide_panel()
	panel_closed.emit()
