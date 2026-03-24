## Persistent status bar showing at-a-glance game statistics.
##
## Displays population count, idle elf count, active task count, current
## sim speed, and FPS in a semi-transparent bar at the bottom-left of the screen.
## Updates are throttled to every 0.25 seconds to avoid per-frame overhead
## from bridge queries.
##
## Sits on the base CanvasLayer (layer 1) alongside the action toolbar and
## notification display. Positioned at bottom-left, below the minimap. The
## minimap clears 44px above the bottom edge to avoid overlapping this bar.
##
## See also: action_toolbar.gd for speed controls, main.gd for wiring,
## sim_bridge.rs for the bridge query methods used here.

extends PanelContainer

## Throttle: only query the bridge every UPDATE_INTERVAL seconds.
const UPDATE_INTERVAL := 0.25

var bridge: SimBridge

var _pop_label: Label
var _idle_label: Label
var _tasks_label: Label
var _speed_label: Label
var _fps_label: Label
var _update_timer: float = 0.0

## Cached speed name from toolbar signal (avoids extra bridge call).
var _current_speed: String = "Normal"


func _ready() -> void:
	# Anchor to bottom-left corner.
	set_anchors_preset(Control.PRESET_BOTTOM_LEFT)
	anchor_left = 0.0
	anchor_top = 1.0
	anchor_right = 0.0
	anchor_bottom = 1.0
	offset_left = 10
	offset_top = -10
	offset_right = 10
	offset_bottom = -10
	grow_horizontal = Control.GROW_DIRECTION_END
	grow_vertical = Control.GROW_DIRECTION_BEGIN
	mouse_filter = Control.MOUSE_FILTER_IGNORE

	# Semi-transparent dark background.
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.08, 0.08, 0.08, 0.75)
	style.corner_radius_top_left = 4
	style.corner_radius_top_right = 4
	style.corner_radius_bottom_left = 4
	style.corner_radius_bottom_right = 4
	style.content_margin_left = 12
	style.content_margin_right = 12
	style.content_margin_top = 6
	style.content_margin_bottom = 6
	add_theme_stylebox_override("panel", style)

	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 16)
	hbox.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(hbox)

	_pop_label = _make_label()
	hbox.add_child(_pop_label)

	hbox.add_child(_make_separator())

	_idle_label = _make_label()
	hbox.add_child(_idle_label)

	hbox.add_child(_make_separator())

	_tasks_label = _make_label()
	hbox.add_child(_tasks_label)

	hbox.add_child(_make_separator())

	_speed_label = _make_label()
	hbox.add_child(_speed_label)

	hbox.add_child(_make_separator())

	_fps_label = _make_label()
	hbox.add_child(_fps_label)

	# Set initial text so bar is visible immediately.
	_pop_label.text = "0 Elves"
	_idle_label.text = "0 Idle"
	_tasks_label.text = "0 Tasks"
	_speed_label.text = "Speed: Normal"
	_fps_label.text = "FPS: --"


func _process(delta: float) -> void:
	if not bridge:
		return
	# FPS updates every frame (cheap, no bridge call).
	_fps_label.text = "FPS: " + str(Engine.get_frames_per_second())
	_update_timer += delta
	if _update_timer < UPDATE_INTERVAL:
		return
	_update_timer = 0.0
	_refresh()


func set_speed(speed_name: String) -> void:
	_current_speed = speed_name
	_update_speed_display()


func _refresh() -> void:
	# Population (elves only).
	var elf_count: int = bridge.elf_count()
	if elf_count == 1:
		_pop_label.text = "1 Elf"
	else:
		_pop_label.text = str(elf_count) + " Elves"

	# Idle elves (no current task).
	var summary: Array = bridge.get_all_creatures_summary()
	var idle_count := 0
	for entry in summary:
		if entry is Dictionary and entry.get("species") == "Elf" and not entry.get("has_task"):
			idle_count += 1
	_idle_label.text = str(idle_count) + " Idle"

	# Active tasks.
	var tasks: Array = bridge.get_active_tasks()
	var task_count: int = tasks.size()
	if task_count == 1:
		_tasks_label.text = "1 Task"
	else:
		_tasks_label.text = str(task_count) + " Tasks"

	_update_speed_display()


func _update_speed_display() -> void:
	var display_name: String = _current_speed
	if display_name == "VeryFast":
		display_name = "Very Fast"
	_speed_label.text = "Speed: " + display_name


func _make_label() -> Label:
	var label := Label.new()
	label.add_theme_font_size_override("font_size", 14)
	label.add_theme_color_override("font_color", Color(0.85, 0.85, 0.85))
	label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return label


func _make_separator() -> VSeparator:
	var sep := VSeparator.new()
	sep.custom_minimum_size.x = 1
	sep.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return sep
