## Top toolbar for gameplay actions, speed controls, and a toggleable debug panel.
##
## The main toolbar row contains gameplay buttons: speed controls (pause/play/
## fast/very fast), Build, Tasks, Structures, Units, Military, Tree Info, and Debug toggle.
## A "Debug" toggle button (or F12) reveals a flow-wrapping row of dev/test tools:
## creature spawn buttons, Summon Elf, Test Notif (sends a debug
## notification through the full sim command pipeline via SimBridge),
## Trigger Raid (spawns a hostile raiding party at the forest edge),
## Wireframe toggle, Smoothing toggle, Decimation toggle,
## QEM-Only toggle (skip retri+collinear, run only QEM decimate),
## Export Mesh (exports chunk at camera as OBJ to user://mesh_export/),
## and a 3D Scale toggle (switches between 1.0 and 0.25 render scale
## for fill-rate vs polygon bottleneck diagnosis).
## Debug spawn buttons are click-only (no keyboard shortcuts).
##
## Keyboard shortcuts:
## [Space] Toggle pause/resume, [F1] Normal, [F2] Fast, [F3] Very Fast
## [B] Build, [T] Tasks, [U] Units, [M] Military, [I] Tree Info, [?] Help, [F12] Toggle debug panel
## [1-9] Selection groups (see selection_controller.gd)
##
## Emits six signals:
## - spawn_requested(species_name: String) — for creature spawns. Picked up
##   by placement_controller.gd to enter placement mode.
## - action_requested(action_name: String) — for task actions ("Summon"),
##   mode toggles ("Build", "Structures"), "TestNotification", "TriggerRaid",
##   and "ExportMesh" (debug mesh OBJ export at camera position).
##   "Summon" creates a GoTo task at the clicked location via SimBridge.
##   "Build" toggles construction mode, handled by construction_controller.gd.
##   "Structures" toggles the structure list panel.
## - speed_changed(speed_name: String) — emitted when the user changes sim
##   speed via buttons or keyboard. Picked up by main.gd to call
##   bridge.set_sim_speed(). Values: "Paused", "Normal", "Fast", "VeryFast".
## - smoothing_toggled(enabled: bool) — toggles mesh smoothing (chamfer vs smooth).
## - decimation_toggled(enabled: bool) — toggles mesh decimation on/off.
## - qem_only_toggled(enabled: bool) — toggles QEM-only mode (skip retri+collinear).
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
signal smoothing_toggled(enabled: bool)
signal decimation_toggled(enabled: bool)
signal qem_only_toggled(enabled: bool)

## Ordered list of speed names for +/- cycling (excludes Paused).
const SPEED_ORDER: Array = ["Normal", "Fast", "VeryFast"]

var _debug_row: HFlowContainer
var _debug_button: Button
var _debug_visible: bool = false
var _scale_button: Button
var _low_res: bool = false

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
	pause_btn.pressed.connect(_set_speed.bind("Paused"))
	speed_container.add_child(pause_btn)
	_speed_buttons["Paused"] = pause_btn

	var normal_btn := Button.new()
	normal_btn.text = "1x"
	normal_btn.custom_minimum_size = Vector2(32, 0)
	normal_btn.pressed.connect(_set_speed.bind("Normal"))
	speed_container.add_child(normal_btn)
	_speed_buttons["Normal"] = normal_btn

	var fast_btn := Button.new()
	fast_btn.text = "2x"
	fast_btn.custom_minimum_size = Vector2(32, 0)
	fast_btn.pressed.connect(_set_speed.bind("Fast"))
	speed_container.add_child(fast_btn)
	_speed_buttons["Fast"] = fast_btn

	var vfast_btn := Button.new()
	vfast_btn.text = "5x"
	vfast_btn.custom_minimum_size = Vector2(40, 0)
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

	var military_button := Button.new()
	military_button.text = "Military [M]"
	military_button.pressed.connect(_on_military_pressed)
	main_row.add_child(military_button)

	var tree_info_button := Button.new()
	tree_info_button.text = "Tree [I]"
	tree_info_button.pressed.connect(_on_tree_info_pressed)
	main_row.add_child(tree_info_button)

	var help_button := Button.new()
	help_button.text = "? Help"
	help_button.pressed.connect(_on_help_pressed)
	main_row.add_child(help_button)

	_debug_button = Button.new()
	_debug_button.text = "Debug [F12]"
	_debug_button.pressed.connect(_toggle_debug)
	main_row.add_child(_debug_button)

	# --- Debug row (hidden by default) ---
	_debug_row = HFlowContainer.new()
	_debug_row.add_theme_constant_override("h_separation", 8)
	_debug_row.add_theme_constant_override("v_separation", 4)
	_debug_row.visible = false
	vbox.add_child(_debug_row)

	var elf_button := Button.new()
	elf_button.text = "Spawn Elf"
	elf_button.pressed.connect(_on_spawn.bind("Elf"))
	_debug_row.add_child(elf_button)

	var capybara_button := Button.new()
	capybara_button.text = "Spawn Capybara"
	capybara_button.pressed.connect(_on_spawn.bind("Capybara"))
	_debug_row.add_child(capybara_button)

	var boar_button := Button.new()
	boar_button.text = "Boar"
	boar_button.pressed.connect(_on_spawn.bind("Boar"))
	_debug_row.add_child(boar_button)

	var deer_button := Button.new()
	deer_button.text = "Deer"
	deer_button.pressed.connect(_on_spawn.bind("Deer"))
	_debug_row.add_child(deer_button)

	var monkey_button := Button.new()
	monkey_button.text = "Monkey"
	monkey_button.pressed.connect(_on_spawn.bind("Monkey"))
	_debug_row.add_child(monkey_button)

	var squirrel_button := Button.new()
	squirrel_button.text = "Squirrel"
	squirrel_button.pressed.connect(_on_spawn.bind("Squirrel"))
	_debug_row.add_child(squirrel_button)

	var elephant_button := Button.new()
	elephant_button.text = "Elephant"
	elephant_button.pressed.connect(_on_spawn.bind("Elephant"))
	_debug_row.add_child(elephant_button)

	var goblin_button := Button.new()
	goblin_button.text = "Goblin"
	goblin_button.pressed.connect(_on_spawn.bind("Goblin"))
	_debug_row.add_child(goblin_button)

	var orc_button := Button.new()
	orc_button.text = "Orc"
	orc_button.pressed.connect(_on_spawn.bind("Orc"))
	_debug_row.add_child(orc_button)

	var troll_button := Button.new()
	troll_button.text = "Troll"
	troll_button.pressed.connect(_on_spawn.bind("Troll"))
	_debug_row.add_child(troll_button)

	var summon_button := Button.new()
	summon_button.text = "Summon Elf"
	summon_button.pressed.connect(_on_summon_pressed)
	_debug_row.add_child(summon_button)

	var dance_button := Button.new()
	dance_button.text = "Debug Dance"
	dance_button.pressed.connect(func(): action_requested.emit("DebugDance"))
	_debug_row.add_child(dance_button)

	var notif_button := Button.new()
	notif_button.text = "Test Notif"
	notif_button.pressed.connect(_on_test_notif_pressed)
	_debug_row.add_child(notif_button)

	var raid_button := Button.new()
	raid_button.text = "Trigger Raid"
	raid_button.pressed.connect(func(): action_requested.emit("TriggerRaid"))
	_debug_row.add_child(raid_button)

	var hornet_button := Button.new()
	hornet_button.text = "Hornet"
	hornet_button.pressed.connect(_on_spawn.bind("Hornet"))
	_debug_row.add_child(hornet_button)

	var wyvern_button := Button.new()
	wyvern_button.text = "Wyvern"
	wyvern_button.pressed.connect(_on_spawn.bind("Wyvern"))
	_debug_row.add_child(wyvern_button)

	var wireframe_button := Button.new()
	wireframe_button.text = "Wireframe"
	wireframe_button.toggle_mode = true
	wireframe_button.pressed.connect(_toggle_wireframe.bind(wireframe_button))
	_debug_row.add_child(wireframe_button)

	var smooth_button := Button.new()
	smooth_button.text = "Smoothing: OFF"
	smooth_button.pressed.connect(_toggle_smoothing.bind(smooth_button))
	_debug_row.add_child(smooth_button)

	var decimate_button := Button.new()
	decimate_button.text = "Decimation: ON"
	decimate_button.pressed.connect(_toggle_decimation.bind(decimate_button))
	_debug_row.add_child(decimate_button)

	var qem_only_button := Button.new()
	qem_only_button.text = "QEM-Only: ✗"
	qem_only_button.pressed.connect(_toggle_qem_only.bind(qem_only_button))
	_debug_row.add_child(qem_only_button)

	var export_mesh_button := Button.new()
	export_mesh_button.text = "Export Mesh"
	export_mesh_button.pressed.connect(func(): action_requested.emit("ExportMesh"))
	_debug_row.add_child(export_mesh_button)

	var debug_sep := VSeparator.new()
	_debug_row.add_child(debug_sep)

	_scale_button = Button.new()
	_scale_button.text = "3D Scale: 1.0"
	_scale_button.pressed.connect(_toggle_3d_scale)
	_debug_row.add_child(_scale_button)

	_update_speed_highlight()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		var key := event as InputEventKey
		if key.ctrl_pressed or key.shift_pressed or key.alt_pressed:
			return
		# Speed shortcuts (always active).
		if key.keycode == KEY_SPACE:
			_toggle_pause()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_F1:
			_set_speed("Normal")
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_F2:
			_set_speed("Fast")
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_F3:
			_set_speed("VeryFast")
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
		elif key.keycode == KEY_M:
			_on_military_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_I:
			_on_tree_info_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_F12:
			_toggle_debug()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_QUESTION:
			_on_help_pressed()
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


func _toggle_smoothing(button: Button) -> void:
	var currently_on := button.text == "Smoothing: ON"
	var new_state := not currently_on
	button.text = "Smoothing: ON" if new_state else "Smoothing: OFF"
	smoothing_toggled.emit(new_state)


func _toggle_decimation(button: Button) -> void:
	var currently_on := button.text == "Decimation: ON"
	var new_state := not currently_on
	button.text = "Decimation: ON" if new_state else "Decimation: OFF"
	decimation_toggled.emit(new_state)


func _toggle_qem_only(button: Button) -> void:
	var currently_on := button.text == "QEM-Only: ✓"
	var new_state := not currently_on
	button.text = "QEM-Only: ✓" if new_state else "QEM-Only: ✗"
	qem_only_toggled.emit(new_state)


func _toggle_wireframe(button: Button) -> void:
	var vp := get_viewport()
	if button.button_pressed:
		vp.debug_draw = Viewport.DEBUG_DRAW_WIREFRAME
	else:
		vp.debug_draw = Viewport.DEBUG_DRAW_DISABLED


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


func _on_military_pressed() -> void:
	action_requested.emit("Military")


func _on_tree_info_pressed() -> void:
	action_requested.emit("TreeInfo")


func _on_help_pressed() -> void:
	action_requested.emit("Help")


func _on_test_notif_pressed() -> void:
	action_requested.emit("TestNotification")


func _toggle_3d_scale() -> void:
	_low_res = not _low_res
	if _low_res:
		get_viewport().scaling_3d_scale = 0.25
		_scale_button.text = "3D Scale: 0.25"
	else:
		get_viewport().scaling_3d_scale = 1.0
		_scale_button.text = "3D Scale: 1.0"
