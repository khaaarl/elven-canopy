## Top toolbar for gameplay actions, speed controls, and a toggleable debug panel.
##
## The main toolbar row contains gameplay buttons: speed controls (pause/play/
## fast/very fast), Build, Tasks, Structures, Units, Tree Info, and Debug toggle.
## A "Debug" toggle button (or F12) reveals a second row with dev/test tools:
## creature spawn buttons and Summon Elf. When the debug row is hidden, its
## keyboard shortcuts (1–7) are inactive.
##
## Keyboard shortcuts:
## [Space] Toggle pause/resume, [+/=] Speed up, [-] Slow down
## [B] Build, [T] Tasks, [U] Units, [I] Tree Info, [F12] Toggle debug panel
## Debug-only (visible when debug panel is open):
## [1] Spawn Elf, [2] Spawn Capybara, [3] Spawn Boar, [4] Spawn Deer,
## [5] Spawn Monkey, [6] Spawn Squirrel, [7] Spawn Elephant, [8] Summon Elf
##
## Emits three signals:
## - spawn_requested(species_name: String) — for creature spawns. Picked up
##   by placement_controller.gd to enter placement mode.
## - action_requested(action_name: String) — for task actions ("Summon") and
##   mode toggles ("Build", "Structures"). "Summon" creates a GoTo task at
##   the clicked location via SimBridge. "Build" toggles construction mode,
##   handled by construction_controller.gd. "Structures" toggles the
##   structure list panel.
## - speed_changed(speed_name: String) — emitted when the user changes sim
##   speed via buttons or keyboard. Picked up by main.gd to call
##   bridge.set_sim_speed(). Values: "Paused", "Normal", "Fast", "VeryFast".
##
## See also: placement_controller.gd which listens for spawn/action signals,
## construction_controller.gd which listens for the "Build" action,
## task_panel.gd which listens for the "Tasks" action,
## structure_list_panel.gd which listens for the "Structures" action,
## main.gd which wires toolbar to controllers and speed signal,
## sim_bridge.rs for the spawn_creature/create_goto_task/set_sim_speed commands.

extends MarginContainer

signal spawn_requested(species_name: String)
signal action_requested(action_name: String)
signal speed_changed(speed_name: String)

## Ordered list of speed names for +/- cycling (excludes Paused).
const SPEED_ORDER: Array = ["Normal", "Fast", "VeryFast"]

var _debug_row: HBoxContainer
var _debug_button: Button
var _debug_visible: bool = false

## Speed button references for highlighting the active speed.
var _speed_buttons: Dictionary = {}
## The last non-paused speed, for spacebar toggle.
var _last_nonpause_speed: String = "Normal"
## The currently active speed name.
var _current_speed: String = "Normal"


func _ready() -> void:
	# Anchor to top-left with some padding.
	add_theme_constant_override("margin_left", 10)
	add_theme_constant_override("margin_top", 10)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 4)
	add_child(vbox)

	# --- Main toolbar row (always visible) ---
	var main_row := HBoxContainer.new()
	main_row.add_theme_constant_override("separation", 8)
	vbox.add_child(main_row)

	# Speed controls.
	var speed_container := HBoxContainer.new()
	speed_container.add_theme_constant_override("separation", 2)
	main_row.add_child(speed_container)

	var pause_btn := Button.new()
	pause_btn.text = "||"
	pause_btn.custom_minimum_size = Vector2(32, 0)
	pause_btn.focus_mode = Control.FOCUS_NONE
	pause_btn.pressed.connect(_set_speed.bind("Paused"))
	speed_container.add_child(pause_btn)
	_speed_buttons["Paused"] = pause_btn

	var normal_btn := Button.new()
	normal_btn.text = "1x"
	normal_btn.custom_minimum_size = Vector2(32, 0)
	normal_btn.focus_mode = Control.FOCUS_NONE
	normal_btn.pressed.connect(_set_speed.bind("Normal"))
	speed_container.add_child(normal_btn)
	_speed_buttons["Normal"] = normal_btn

	var fast_btn := Button.new()
	fast_btn.text = "2x"
	fast_btn.custom_minimum_size = Vector2(32, 0)
	fast_btn.focus_mode = Control.FOCUS_NONE
	fast_btn.pressed.connect(_set_speed.bind("Fast"))
	speed_container.add_child(fast_btn)
	_speed_buttons["Fast"] = fast_btn

	var vfast_btn := Button.new()
	vfast_btn.text = "5x"
	vfast_btn.custom_minimum_size = Vector2(40, 0)
	vfast_btn.focus_mode = Control.FOCUS_NONE
	vfast_btn.pressed.connect(_set_speed.bind("VeryFast"))
	speed_container.add_child(vfast_btn)
	_speed_buttons["VeryFast"] = vfast_btn

	# Separator between speed and gameplay buttons.
	var sep := VSeparator.new()
	main_row.add_child(sep)

	var build_button := Button.new()
	build_button.text = "Build [B]"
	build_button.pressed.connect(_on_build_pressed)
	main_row.add_child(build_button)

	var tasks_button := Button.new()
	tasks_button.text = "Tasks [T]"
	tasks_button.pressed.connect(_on_tasks_pressed)
	main_row.add_child(tasks_button)

	var structures_button := Button.new()
	structures_button.text = "Structures"
	structures_button.pressed.connect(_on_structures_pressed)
	main_row.add_child(structures_button)

	var units_button := Button.new()
	units_button.text = "Units [U]"
	units_button.pressed.connect(_on_units_pressed)
	main_row.add_child(units_button)

	var tree_info_button := Button.new()
	tree_info_button.text = "Tree [I]"
	tree_info_button.pressed.connect(_on_tree_info_pressed)
	main_row.add_child(tree_info_button)

	_debug_button = Button.new()
	_debug_button.text = "Debug [F12]"
	_debug_button.pressed.connect(_toggle_debug)
	main_row.add_child(_debug_button)

	# --- Debug row (hidden by default) ---
	_debug_row = HBoxContainer.new()
	_debug_row.add_theme_constant_override("separation", 8)
	_debug_row.visible = false
	vbox.add_child(_debug_row)

	var elf_button := Button.new()
	elf_button.text = "Spawn Elf [1]"
	elf_button.pressed.connect(_on_spawn.bind("Elf"))
	_debug_row.add_child(elf_button)

	var capybara_button := Button.new()
	capybara_button.text = "Spawn Capybara [2]"
	capybara_button.pressed.connect(_on_spawn.bind("Capybara"))
	_debug_row.add_child(capybara_button)

	var boar_button := Button.new()
	boar_button.text = "Boar [3]"
	boar_button.pressed.connect(_on_spawn.bind("Boar"))
	_debug_row.add_child(boar_button)

	var deer_button := Button.new()
	deer_button.text = "Deer [4]"
	deer_button.pressed.connect(_on_spawn.bind("Deer"))
	_debug_row.add_child(deer_button)

	var monkey_button := Button.new()
	monkey_button.text = "Monkey [5]"
	monkey_button.pressed.connect(_on_spawn.bind("Monkey"))
	_debug_row.add_child(monkey_button)

	var squirrel_button := Button.new()
	squirrel_button.text = "Squirrel [6]"
	squirrel_button.pressed.connect(_on_spawn.bind("Squirrel"))
	_debug_row.add_child(squirrel_button)

	var elephant_button := Button.new()
	elephant_button.text = "Elephant [7]"
	elephant_button.pressed.connect(_on_spawn.bind("Elephant"))
	_debug_row.add_child(elephant_button)

	var summon_button := Button.new()
	summon_button.text = "Summon Elf [8]"
	summon_button.pressed.connect(_on_summon_pressed)
	_debug_row.add_child(summon_button)

	_update_speed_highlight()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		var key := event as InputEventKey
		# Speed shortcuts (always active).
		if key.keycode == KEY_SPACE:
			_toggle_pause()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_EQUAL or key.keycode == KEY_KP_ADD:
			_speed_up()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_MINUS or key.keycode == KEY_KP_SUBTRACT:
			_slow_down()
			get_viewport().set_input_as_handled()
		# Gameplay shortcuts (always active).
		elif key.keycode == KEY_B:
			_on_build_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_T:
			_on_tasks_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_U:
			_on_units_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_I:
			_on_tree_info_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_F12:
			_toggle_debug()
			get_viewport().set_input_as_handled()
		# Debug shortcuts (only when debug row is visible).
		elif _debug_visible:
			if key.keycode == KEY_1:
				_on_spawn("Elf")
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_2:
				_on_spawn("Capybara")
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_3:
				_on_spawn("Boar")
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_4:
				_on_spawn("Deer")
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_5:
				_on_spawn("Monkey")
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_6:
				_on_spawn("Squirrel")
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_7:
				_on_spawn("Elephant")
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_8:
				_on_summon_pressed()
				get_viewport().set_input_as_handled()


func _set_speed(speed_name: String) -> void:
	if speed_name != "Paused" and speed_name != _current_speed:
		_last_nonpause_speed = speed_name
	_current_speed = speed_name
	_update_speed_highlight()
	speed_changed.emit(speed_name)


func _toggle_pause() -> void:
	if _current_speed == "Paused":
		_set_speed(_last_nonpause_speed)
	else:
		_set_speed("Paused")


func _speed_up() -> void:
	var active := _current_speed if _current_speed != "Paused" else _last_nonpause_speed
	var idx := SPEED_ORDER.find(active)
	if idx < 0:
		idx = 0
	if idx < SPEED_ORDER.size() - 1:
		_set_speed(SPEED_ORDER[idx + 1])


func _slow_down() -> void:
	var active := _current_speed if _current_speed != "Paused" else _last_nonpause_speed
	var idx := SPEED_ORDER.find(active)
	if idx < 0:
		idx = 0
	if idx > 0:
		_set_speed(SPEED_ORDER[idx - 1])


func _update_speed_highlight() -> void:
	for speed_name in _speed_buttons:
		var btn: Button = _speed_buttons[speed_name]
		if speed_name == _current_speed:
			btn.add_theme_color_override("font_color", Color(0.2, 1.0, 0.4))
		else:
			btn.remove_theme_color_override("font_color")


func _toggle_debug() -> void:
	_debug_visible = not _debug_visible
	_debug_row.visible = _debug_visible


func _on_spawn(species_name: String) -> void:
	spawn_requested.emit(species_name)


func _on_summon_pressed() -> void:
	action_requested.emit("Summon")


func _on_build_pressed() -> void:
	action_requested.emit("Build")


func _on_tasks_pressed() -> void:
	action_requested.emit("Tasks")


func _on_structures_pressed() -> void:
	action_requested.emit("Structures")


func _on_units_pressed() -> void:
	action_requested.emit("Units")


func _on_tree_info_pressed() -> void:
	action_requested.emit("TreeInfo")
