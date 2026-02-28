## Toolbar UI for spawning creatures, placing tasks, and toggling construction.
##
## Builds a horizontal row of buttons for each creature species plus action
## buttons. Each button has a keyboard shortcut handled via _unhandled_input().
##
## Keyboard shortcuts:
## [1] Spawn Elf, [2] Spawn Capybara, [3] Spawn Boar, [4] Spawn Deer,
## [5] Spawn Monkey, [6] Spawn Squirrel, [7] Summon Elf, [B] Build,
## [T] Tasks
##
## Emits two signals:
## - spawn_requested(species_name: String) — for creature spawns. Picked up
##   by placement_controller.gd to enter placement mode.
## - action_requested(action_name: String) — for task actions ("Summon") and
##   mode toggles ("Build", "Structures"). "Summon" creates a GoTo task at
##   the clicked location via SimBridge. "Build" toggles construction mode,
##   handled by construction_controller.gd. "Structures" toggles the
##   structure list panel.
##
## See also: placement_controller.gd which listens for spawn/action signals,
## construction_controller.gd which listens for the "Build" action,
## task_panel.gd which listens for the "Tasks" action,
## structure_list_panel.gd which listens for the "Structures" action,
## main.gd which wires toolbar to controllers,
## sim_bridge.rs for the spawn_creature/create_goto_task commands.

extends MarginContainer

signal spawn_requested(species_name: String)
signal action_requested(action_name: String)

var _elf_button: Button
var _capybara_button: Button
var _boar_button: Button
var _deer_button: Button
var _monkey_button: Button
var _squirrel_button: Button
var _summon_button: Button
var _build_button: Button
var _tasks_button: Button
var _structures_button: Button


func _ready() -> void:
	# Anchor to top-left with some padding.
	add_theme_constant_override("margin_left", 10)
	add_theme_constant_override("margin_top", 10)

	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 8)
	add_child(hbox)

	_elf_button = Button.new()
	_elf_button.text = "Spawn Elf [1]"
	_elf_button.pressed.connect(_on_spawn.bind("Elf"))
	hbox.add_child(_elf_button)

	_capybara_button = Button.new()
	_capybara_button.text = "Spawn Capybara [2]"
	_capybara_button.pressed.connect(_on_spawn.bind("Capybara"))
	hbox.add_child(_capybara_button)

	_boar_button = Button.new()
	_boar_button.text = "Boar [3]"
	_boar_button.pressed.connect(_on_spawn.bind("Boar"))
	hbox.add_child(_boar_button)

	_deer_button = Button.new()
	_deer_button.text = "Deer [4]"
	_deer_button.pressed.connect(_on_spawn.bind("Deer"))
	hbox.add_child(_deer_button)

	_monkey_button = Button.new()
	_monkey_button.text = "Monkey [5]"
	_monkey_button.pressed.connect(_on_spawn.bind("Monkey"))
	hbox.add_child(_monkey_button)

	_squirrel_button = Button.new()
	_squirrel_button.text = "Squirrel [6]"
	_squirrel_button.pressed.connect(_on_spawn.bind("Squirrel"))
	hbox.add_child(_squirrel_button)

	_summon_button = Button.new()
	_summon_button.text = "Summon Elf [7]"
	_summon_button.pressed.connect(_on_summon_pressed)
	hbox.add_child(_summon_button)

	_build_button = Button.new()
	_build_button.text = "Build [B]"
	_build_button.pressed.connect(_on_build_pressed)
	hbox.add_child(_build_button)

	_tasks_button = Button.new()
	_tasks_button.text = "Tasks [T]"
	_tasks_button.pressed.connect(_on_tasks_pressed)
	hbox.add_child(_tasks_button)

	_structures_button = Button.new()
	_structures_button.text = "Structures"
	_structures_button.pressed.connect(_on_structures_pressed)
	hbox.add_child(_structures_button)


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		var key := event as InputEventKey
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
			_on_summon_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_B:
			_on_build_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_T:
			_on_tasks_pressed()
			get_viewport().set_input_as_handled()


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
