## Main scene controller for Elven Canopy.
##
## Orchestrates startup and the per-frame sim loop. Everything in the game
## connects through this script.
##
## Startup sequence (_ready):
## 1. Check GameSession.load_save_path — if set, load a saved game instead
##    of starting fresh.
## 2. For new games: read the simulation seed from GameSession autoload (set
##    by the new-game menu), initialize SimBridge, and spawn initial creatures.
## 3. For loaded games: read the save file, call bridge.load_game_json(),
##    and skip creature spawning (creatures are already in the loaded state).
## 4. Common path: set up renderers, toolbar, placement controller,
##    construction controller, selection controller, creature info panel,
##    menu button, and pause menu.
##
## Per-frame (_process): uses a time-based accumulator to advance the sim by
## the correct number of ticks, decoupled from the frame rate. The sim runs
## at 1000 ticks per simulated second (tick_duration_ms = 1). Each frame,
## delta time is accumulated and converted to ticks. A cap of 5000 ticks per
## frame prevents spiral-of-death on slow frames. After stepping the sim,
## computes a fractional render_tick = current_tick + accumulator_fraction
## and distributes it to renderers and the selection controller for smooth
## creature movement interpolation between nav nodes.
##
## ESC precedence chain (reverse tree order — later children fire first):
## 1. placement_controller — cancel active placement
## 2. construction_controller — if placing: exit placing sub-mode;
##    if active: exit construction mode
## 3. selection_controller — deselect creature
## 4. task_panel — close task list (if visible, on CanvasLayer layer 2)
## 5. pause_menu — open/close (on CanvasLayer layer 2, added after task panel)
##
## See also: orbital_camera.gd for camera controls, sim_bridge.rs (Rust)
## for the simulation interface, tree_renderer.gd / elf_renderer.gd /
## capybara_renderer.gd / blueprint_renderer.gd for rendering,
## spawn_toolbar.gd for the toolbar UI, placement_controller.gd for
## click-to-place logic, construction_controller.gd for construction mode
## and platform placement, selection_controller.gd for click-to-select,
## creature_info_panel.gd for the creature info panel, task_panel.gd for
## the task list overlay, game_session.gd for the autoload that carries
## the seed/load path from the menu, pause_menu.gd for the ESC pause overlay.

extends Node3D

## Y offsets per species for world-space sprite positions. Must match the
## values used by elf_renderer.gd, capybara_renderer.gd, and creature_renderer.gd.
const SPECIES_Y_OFFSETS = {
	"Elf": 0.48,
	"Capybara": 0.32,
	"Boar": 0.38,
	"Deer": 0.46,
	"Monkey": 0.44,
	"Squirrel": 0.28,
}

## The simulation seed. Deterministic: same seed = same game.
## Overridden by GameSession.sim_seed when launched through the menu flow.
## The @export default (42) is a fallback for direct scene launches (F6 in editor).
@export var sim_seed: int = 42

var _selector: Node3D
var _panel: PanelContainer
var _task_panel: ColorRect
var _camera_pivot: Node3D
var _construction_controller: Node
## Renderers for new species (Boar, Deer, Monkey, Squirrel). Receive
## render_tick each frame for smooth creature interpolation.
var _extra_renderers: Array = []
var _bp_renderer: Node3D
## Fractional seconds of unprocessed sim time. Accumulates each frame,
## converted to ticks by dividing by tick_duration_ms / 1000.
var _sim_accumulator: float = 0.0
## Seconds per sim tick, cached from bridge.tick_duration_ms().
var _seconds_per_tick: float = 0.001


func _ready() -> void:
	var bridge: SimBridge = $SimBridge
	var is_loaded_game := false

	# --- Branch: load a saved game or start a new one ---
	if GameSession.load_save_path != "":
		is_loaded_game = _try_load_save(bridge, GameSession.load_save_path)
		GameSession.load_save_path = ""
		if not is_loaded_game:
			# Load failed — return to main menu.
			push_error("main: failed to load save, returning to main menu")
			get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
			return

	if not is_loaded_game:
		# Normal new-game flow.
		if GameSession.sim_seed >= 0:
			sim_seed = GameSession.sim_seed

		if not GameSession.tree_profile.is_empty():
			var json_str := JSON.stringify(GameSession.tree_profile)
			bridge.init_sim_with_tree_profile_json(sim_seed, json_str)
		else:
			bridge.init_sim(sim_seed)
		print(
			(
				"Elven Canopy: sim initialized (seed=%d, mana=%.1f)"
				% [sim_seed, bridge.home_tree_mana()]
			)
		)

		# Spawn initial creatures.
		var cx := 128
		var cz := 128
		for i in 5:
			var ox := i * 3 - 6
			bridge.spawn_elf(cx + ox, 0, cz)
		print("Elven Canopy: spawned %d elves near (%d, 0, %d)" % [bridge.elf_count(), cx, cz])

		for i in 5:
			bridge.spawn_capybara(cx, 0, cz)
		print(
			(
				"Elven Canopy: spawned %d capybaras near (%d, 0, %d)"
				% [bridge.capybara_count(), cx, cz]
			)
		)

		for i in 3:
			bridge.spawn_creature("Boar", cx, 0, cz)
		for i in 3:
			bridge.spawn_creature("Deer", cx, 0, cz)
		for i in 3:
			bridge.spawn_creature("Monkey", cx, 0, cz)
		for i in 3:
			bridge.spawn_creature("Squirrel", cx, 0, cz)
		print(
			(
				"Elven Canopy: spawned new creatures (boar=%d, deer=%d, monkey=%d, squirrel=%d)"
				% [
					bridge.creature_count_by_name("Boar"),
					bridge.creature_count_by_name("Deer"),
					bridge.creature_count_by_name("Monkey"),
					bridge.creature_count_by_name("Squirrel"),
				]
			)
		)

	# --- Common setup (new game and loaded game) ---

	# Cache tick duration for the frame accumulator.
	_seconds_per_tick = bridge.tick_duration_ms() / 1000.0

	# Set up tree renderer.
	var tree_renderer = $TreeRenderer
	tree_renderer.setup(bridge)

	# Set up elf renderer.
	var elf_renderer = $ElfRenderer
	elf_renderer.setup(bridge)

	# Set up capybara renderer (sim-driven).
	var capybara_renderer = $CapybaraRenderer
	capybara_renderer.setup(bridge)

	# Set up generic renderers for new species.
	var renderer_script = load("res://scripts/creature_renderer.gd")
	for entry in [["Boar", 0.38], ["Deer", 0.46], ["Monkey", 0.44], ["Squirrel", 0.28]]:
		var r := Node3D.new()
		r.set_script(renderer_script)
		add_child(r)
		r.setup(bridge, entry[0], entry[1])
		_extra_renderers.append(r)

	# Set up spawn toolbar UI (rendered on top of 3D via CanvasLayer).
	var canvas_layer := CanvasLayer.new()
	add_child(canvas_layer)

	var toolbar_script = load("res://scripts/spawn_toolbar.gd")
	var toolbar := MarginContainer.new()
	toolbar.set_script(toolbar_script)
	canvas_layer.add_child(toolbar)

	# Set up placement controller.
	var controller_script = load("res://scripts/placement_controller.gd")
	var controller := Node3D.new()
	controller.set_script(controller_script)
	add_child(controller)
	controller.setup(bridge, $CameraPivot/Camera3D)
	controller.connect_toolbar(toolbar)

	# Set up construction controller (between placement and selection for
	# ESC precedence — reverse tree order means later children fire first).
	var construction_script = load("res://scripts/construction_controller.gd")
	_construction_controller = Node.new()
	_construction_controller.set_script(construction_script)
	add_child(_construction_controller)
	_construction_controller.setup(bridge, $CameraPivot)
	_construction_controller.connect_toolbar(toolbar)
	# Construction panel lives on the CanvasLayer for UI rendering.
	canvas_layer.add_child(_construction_controller.get_panel())

	# Entering construction mode: deselect creature, cancel placement.
	_construction_controller.construction_mode_entered.connect(
		func():
			if controller.is_placing():
				controller.cancel_placement()
			if _selector:
				_selector.deselect()
			if _panel:
				_panel.hide_panel()
			if _camera_pivot:
				_camera_pivot.stop_follow()
	)

	# Set up blueprint renderer (ghost cubes for unplaced blueprints,
	# solid brown cubes for materialized construction voxels).
	var bp_renderer_script = load("res://scripts/blueprint_renderer.gd")
	_bp_renderer = Node3D.new()
	_bp_renderer.set_script(bp_renderer_script)
	add_child(_bp_renderer)
	_bp_renderer.setup(bridge)
	_construction_controller.blueprint_placed.connect(_bp_renderer.refresh)

	# Set up creature info panel (on the same CanvasLayer as the toolbar).
	var panel_script = load("res://scripts/creature_info_panel.gd")
	_panel = PanelContainer.new()
	_panel.set_script(panel_script)
	canvas_layer.add_child(_panel)

	# Set up selection controller.
	var selector_script = load("res://scripts/selection_controller.gd")
	_selector = Node3D.new()
	_selector.set_script(selector_script)
	add_child(_selector)
	_selector.setup(bridge, $CameraPivot/Camera3D)
	_selector.set_placement_controller(controller)

	# Wire selection -> panel.
	_camera_pivot = $CameraPivot
	_selector.creature_selected.connect(
		func(species: String, index: int):
			var tick := float(bridge.current_tick())
			var info := bridge.get_creature_info(species, index, tick)
			if not info.is_empty():
				_panel.show_creature(species, index, info)
	)
	_selector.creature_deselected.connect(
		func():
			_panel.hide_panel()
			_camera_pivot.stop_follow()
	)

	# Menu button (top-right corner, on the same CanvasLayer as toolbar).
	var menu_btn := Button.new()
	menu_btn.text = "Menu"
	menu_btn.custom_minimum_size = Vector2(80, 40)
	menu_btn.set_anchors_preset(Control.PRESET_TOP_RIGHT)
	menu_btn.position = Vector2(-90, 10)
	canvas_layer.add_child(menu_btn)

	# Pause menu overlay (on a higher CanvasLayer so it covers toolbar/panel).
	var pause_layer := CanvasLayer.new()
	pause_layer.layer = 2
	add_child(pause_layer)

	var pause_script = load("res://scripts/pause_menu.gd")
	var pause_menu := ColorRect.new()
	pause_menu.set_script(pause_script)
	pause_layer.add_child(pause_menu)
	pause_menu.setup(bridge)
	menu_btn.pressed.connect(pause_menu.toggle)

	# Task panel overlay (on CanvasLayer 2, added after pause_layer so its
	# ESC handler fires first in reverse tree order).
	var task_panel_layer := CanvasLayer.new()
	task_panel_layer.layer = 2
	add_child(task_panel_layer)

	var task_panel_script = load("res://scripts/task_panel.gd")
	_task_panel = ColorRect.new()
	_task_panel.set_script(task_panel_script)
	task_panel_layer.add_child(_task_panel)

	# Wire toolbar "Tasks" action -> task panel toggle.
	toolbar.action_requested.connect(
		func(action: String):
			if action == "Tasks":
				_task_panel.toggle()
	)

	# Wire task panel zoom-to-creature -> select creature + camera follow.
	_task_panel.zoom_to_creature.connect(
		func(species: String, index: int):
			_task_panel.hide_panel()
			_selector.select_creature(species, index)
			var tick := float(bridge.current_tick())
			var pos = _get_creature_world_pos(bridge, tick, species, index)
			if pos != null:
				_camera_pivot.start_follow(pos)
	)

	# Wire task panel zoom-to-location -> move camera pivot.
	_task_panel.zoom_to_location.connect(
		func(x: float, y: float, z: float):
			_task_panel.hide_panel()
			_look_at_position(Vector3(x + 0.5, y, z + 0.5))
	)

	# Wire follow button -> camera.
	_panel.follow_requested.connect(
		func():
			var tick := float(bridge.current_tick())
			var pos = _get_creature_world_pos(
				bridge, tick, _selector.get_selected_species(), _selector.get_selected_index()
			)
			if pos != null:
				_camera_pivot.start_follow(pos)
	)
	_panel.unfollow_requested.connect(func(): _camera_pivot.stop_follow())
	_panel.panel_closed.connect(
		func():
			_selector.deselect()
			_camera_pivot.stop_follow()
	)


## Try to load a save file. Returns true on success.
func _try_load_save(bridge: SimBridge, save_path: String) -> bool:
	var file := FileAccess.open(save_path, FileAccess.READ)
	if file == null:
		push_error("main: cannot open save file: %s" % save_path)
		return false
	var json := file.get_as_text()
	file.close()
	if json.is_empty():
		push_error("main: save file is empty: %s" % save_path)
		return false
	var ok := bridge.load_game_json(json)
	if ok:
		print("Elven Canopy: loaded save from %s (tick=%d)" % [save_path, bridge.current_tick()])
	return ok


func _process(delta: float) -> void:
	var bridge: SimBridge = $SimBridge
	if bridge.is_initialized():
		_sim_accumulator += delta
		var ticks_to_advance := int(_sim_accumulator / _seconds_per_tick)
		if ticks_to_advance > 5000:
			ticks_to_advance = 5000
		if ticks_to_advance > 0:
			_sim_accumulator -= ticks_to_advance * _seconds_per_tick
			bridge.step_to_tick(bridge.current_tick() + ticks_to_advance)

	# Compute the fractional render tick for smooth creature interpolation.
	# This is the sim's current tick plus the fraction of an unprocessed tick
	# remaining in the accumulator.
	var render_tick: float = float(bridge.current_tick()) + (_sim_accumulator / _seconds_per_tick)

	# Distribute render_tick to all consumers that read creature positions.
	$ElfRenderer.set_render_tick(render_tick)
	$CapybaraRenderer.set_render_tick(render_tick)
	for r in _extra_renderers:
		r.set_render_tick(render_tick)
	_selector.set_render_tick(render_tick)

	# Refresh blueprint/construction renderer so materialized voxels appear
	# as solid wood and ghost cubes disappear as voxels are placed.
	if _bp_renderer:
		_bp_renderer.refresh()

	# Update follow target each frame so the camera tracks creature movement.
	if _camera_pivot and _camera_pivot.is_following():
		var pos = _get_creature_world_pos(
			bridge, render_tick, _selector.get_selected_species(), _selector.get_selected_index()
		)
		if pos != null:
			_camera_pivot.update_follow_target(pos)
		else:
			_camera_pivot.stop_follow()
			_panel.set_follow_state(false)

	# Detect if camera broke follow via movement keys.
	if _panel and _panel.visible and not _camera_pivot.is_following():
		_panel.set_follow_state(false)

	# Refresh task panel while visible.
	if _task_panel and _task_panel.visible:
		var tasks := bridge.get_active_tasks()
		_task_panel.update_tasks(tasks)

	# Refresh panel info while a creature is selected.
	if _panel and _panel.visible and _selector.get_selected_index() >= 0:
		var info := bridge.get_creature_info(
			_selector.get_selected_species(), _selector.get_selected_index(), render_tick
		)
		if not info.is_empty():
			_panel.update_info(info)


## Move the camera to look at a world-space position, stopping any active
## creature follow. Use this for any "jump the camera here" action that
## isn't creature-tracking (task sites, landmarks, etc.).
func _look_at_position(pos: Vector3) -> void:
	_camera_pivot.stop_follow()
	if _panel and _panel.visible:
		_panel.set_follow_state(false)
	_camera_pivot.position = pos


## Get the world-space position of a creature sprite, matching the offsets
## used by the species renderers. Uses render_tick for smooth interpolation.
func _get_creature_world_pos(
	bridge: SimBridge, render_tick: float, species: String, index: int
) -> Variant:
	var y_off: float = SPECIES_Y_OFFSETS.get(species, 0.4)
	var positions := bridge.get_creature_positions(species, render_tick)
	if index >= 0 and index < positions.size():
		var p := positions[index]
		return Vector3(p.x + 0.5, p.y + y_off, p.z + 0.5)
	return null
