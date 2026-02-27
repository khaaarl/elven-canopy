## Creature info panel displayed on the right side of the screen.
##
## Shows information about the currently selected creature. Built
## programmatically as a PanelContainer with labels and a Follow button.
## Sits on the CanvasLayer alongside the spawn toolbar.
##
## The panel is ~25% screen width, full height, anchored to the right edge.
## Shows species, name (placeholder), position, task status, and a food
## gauge (progress bar + percentage). Updated every frame by main.gd.
##
## See also: selection_controller.gd which triggers show/hide,
## orbital_camera.gd which responds to follow/unfollow,
## main.gd which wires everything together.

extends PanelContainer

signal follow_requested
signal unfollow_requested
signal panel_closed

var _species_label: Label
var _name_label: Label
var _position_label: Label
var _task_label: Label
var _food_bar: ProgressBar
var _food_label: Label
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

	# Name (placeholder).
	_name_label = Label.new()
	vbox.add_child(_name_label)

	# Position.
	_position_label = Label.new()
	vbox.add_child(_position_label)

	# Task status.
	_task_label = Label.new()
	vbox.add_child(_task_label)

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
	_name_label.text = "Name: %s #%d" % [species, index + 1]
	_update_position(info)
	_task_label.text = "Has task: %s" % str(info.get("has_task", false))
	_update_food(info)
	_is_following = false
	_follow_button.text = "Follow"
	visible = true


func update_info(info: Dictionary) -> void:
	_update_position(info)
	_task_label.text = "Has task: %s" % str(info.get("has_task", false))
	_update_food(info)


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


func _update_position(info: Dictionary) -> void:
	_position_label.text = "Position: (%d, %d, %d)" % [
		info.get("x", 0), info.get("y", 0), info.get("z", 0)]


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
