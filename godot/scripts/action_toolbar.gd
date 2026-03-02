## Top toolbar for gameplay actions, speed controls, and a toggleable debug
## panel.
##
## The main toolbar row contains gameplay buttons (Build, Tasks, Structures,
## Tree Info), sim speed controls (Pause / 1x / 2x / 3x), and a Debug toggle.
## A "Debug" toggle button (or F12) reveals a second row with dev/test tools:
## creature spawn buttons and Summon Elf. When the debug row is hidden, its
## keyboard shortcuts (1–7) are inactive.
##
## Keyboard shortcuts:
## [Space] Toggle pause, [+] Speed up, [-] Speed down
## [B] Build, [T] Tasks, [I] Tree Info, [F12] Toggle debug panel
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
## - speed_changed(new_speed: int) — sim speed change (0=paused, 1/2/3=
##   multiplier). main.gd applies the multiplier to the tick accumulator.
##
## See also: placement_controller.gd which listens for spawn/action signals,
## construction_controller.gd which listens for the "Build" action,
## task_panel.gd which listens for the "Tasks" action,
## structure_list_panel.gd which listens for the "Structures" action,
## main.gd which wires toolbar to controllers and applies speed changes,
## sim_bridge.rs for the spawn_creature/create_goto_task commands.

extends MarginContainer

signal spawn_requested(species_name: String)
signal action_requested(action_name: String)
signal speed_changed(new_speed: int)

var _debug_row: HBoxContainer
var _debug_button: Button
var _debug_visible: bool = false
## Current sim speed: 0 = paused, 1/2/3 = speed multiplier.
var _speed: int = 1
var _speed_buttons: Array = []


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

	var tree_info_button := Button.new()
	tree_info_button.text = "Tree [I]"
	tree_info_button.pressed.connect(_on_tree_info_pressed)
	main_row.add_child(tree_info_button)

	_debug_button = Button.new()
	_debug_button.text = "Debug [F12]"
	_debug_button.pressed.connect(_toggle_debug)
	main_row.add_child(_debug_button)

	# --- Speed controls ---
	var speed_separator := VSeparator.new()
	main_row.add_child(speed_separator)

	var speed_labels := ["|| [Space]", "> [Space]", ">> [+]", ">>> [+]"]
	for i in 4:
		var btn := Button.new()
		btn.text = speed_labels[i]
		btn.pressed.connect(_set_speed.bind(i))
		main_row.add_child(btn)
		_speed_buttons.append(btn)
	_update_speed_buttons()

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


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		var key := event as InputEventKey
		# Speed shortcuts (always active).
		if key.keycode == KEY_SPACE:
			_toggle_pause()
			get_viewport().set_input_as_handled()
			return
		if key.keycode == KEY_EQUAL or key.keycode == KEY_KP_ADD:
			_change_speed(1)
			get_viewport().set_input_as_handled()
			return
		if key.keycode == KEY_MINUS or key.keycode == KEY_KP_SUBTRACT:
			_change_speed(-1)
			get_viewport().set_input_as_handled()
			return
		# Gameplay shortcuts (always active).
		if key.keycode == KEY_B:
			_on_build_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_T:
			_on_tasks_pressed()
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


func _on_tree_info_pressed() -> void:
	action_requested.emit("TreeInfo")


func _set_speed(new_speed: int) -> void:
	_speed = clampi(new_speed, 0, 3)
	_update_speed_buttons()
	speed_changed.emit(_speed)


func _toggle_pause() -> void:
	if _speed == 0:
		_set_speed(1)
	else:
		_set_speed(0)


## Increment or decrement speed, clamped to 1–3 (skips pause).
func _change_speed(delta: int) -> void:
	var new_speed := clampi(_speed + delta, 1, 3)
	_set_speed(new_speed)


func _update_speed_buttons() -> void:
	for i in _speed_buttons.size():
		_speed_buttons[i].disabled = (i == _speed)
