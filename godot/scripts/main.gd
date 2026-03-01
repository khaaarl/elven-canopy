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
## 4. tree_info_panel — close tree info (if visible, on CanvasLayer layer 1)
## 5. structure_list_panel — close structure list (if visible, on CanvasLayer layer 2)
## 6. task_panel — close task list (if visible, on CanvasLayer layer 2)
## 7. pause_menu — open/close (on CanvasLayer layer 2, added first)
##
## See also: orbital_camera.gd for camera controls, sim_bridge.rs (Rust)
## for the simulation interface, tree_renderer.gd / elf_renderer.gd /
## capybara_renderer.gd / blueprint_renderer.gd for rendering,
## action_toolbar.gd for the toolbar UI, placement_controller.gd for
## click-to-place logic, construction_controller.gd for construction mode
## and platform placement, selection_controller.gd for click-to-select,
## creature_info_panel.gd for the creature info panel,
## tree_info_panel.gd for the tree stats panel, task_panel.gd for
## the task list overlay, structure_list_panel.gd for the structure list
## overlay, game_session.gd for the autoload that carries the seed/load
## path from the menu, pause_menu.gd for the ESC pause overlay.

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
var _tree_info_panel: PanelContainer
var _task_panel: ColorRect
var _structure_panel: ColorRect
var _camera_pivot: Node3D
var _construction_controller: Node
var _placement_controller: Node3D
## Renderers for new species (Boar, Deer, Monkey, Squirrel). Receive
## render_tick each frame for smooth creature interpolation.
var _extra_renderers: Array = []
var _bp_renderer: Node3D
var _bldg_renderer: Node3D
var _lobby_overlay: ColorRect
## Fractional seconds of unprocessed sim time. Accumulates each frame,
## converted to ticks by dividing by tick_duration_ms / 1000.
var _sim_accumulator: float = 0.0
## Seconds per sim tick, cached from bridge.tick_duration_ms().
var _seconds_per_tick: float = 0.001
## Multiplayer: seconds since last turn was received. Used for smooth
## render_tick interpolation between turns.
var _mp_time_since_turn: float = 0.0


func _ready() -> void:
	var bridge: SimBridge = $SimBridge
	var is_loaded_game := false

	# --- Branch: multiplayer ---
	if GameSession.multiplayer_mode == "host":
		var ok := (
			bridge
			. host_game(
				GameSession.mp_port,
				GameSession.mp_session_name,
				GameSession.mp_password,
				GameSession.mp_max_players,
				GameSession.mp_ticks_per_turn,
			)
		)
		if not ok:
			push_error("main: failed to host game, returning to menu")
			GameSession.multiplayer_mode = ""
			get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
			return
		# Read seed from GameSession for use when starting.
		if GameSession.sim_seed >= 0:
			sim_seed = GameSession.sim_seed
		_show_lobby(bridge)
		return

	if GameSession.multiplayer_mode == "join":
		var ok := (
			bridge
			. join_game(
				GameSession.mp_relay_address,
				GameSession.mp_player_name,
				GameSession.mp_password,
			)
		)
		if not ok:
			push_error("main: failed to join game, returning to menu")
			GameSession.multiplayer_mode = ""
			get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
			return
		_show_lobby(bridge)
		return

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
		# Vary initial elf food so some are hungry from the start.
		# food_max is 1_000_000_000_000_000; percentages below threshold (50%) seek fruit.
		var food_max: int = 1_000_000_000_000_000
		var elf_food_pcts := [100, 90, 70, 60, 48]
		for i in elf_food_pcts.size():
			if elf_food_pcts[i] < 100:
				bridge.set_creature_food("Elf", i, food_max * elf_food_pcts[i] / 100)
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

	_setup_common(bridge)


## Show the lobby overlay and wait for the game to start.
## When game_started fires, proceed with common setup.
func _show_lobby(bridge: SimBridge) -> void:
	var lobby_layer := CanvasLayer.new()
	lobby_layer.layer = 3
	add_child(lobby_layer)

	var lobby_script = load("res://scripts/lobby_overlay.gd")
	_lobby_overlay = ColorRect.new()
	_lobby_overlay.set_script(lobby_script)
	lobby_layer.add_child(_lobby_overlay)
	_lobby_overlay.setup(bridge)
	_lobby_overlay.game_started.connect(_on_mp_game_started)


## Called when the multiplayer game starts (lobby's game_started signal).
## Now the sim is initialized — set up renderers and UI.
func _on_mp_game_started() -> void:
	var bridge: SimBridge = $SimBridge

	# Spawn initial creatures (host only).
	if bridge.is_host():
		var cx := 128
		var cz := 128
		for i in 5:
			var ox := i * 3 - 6
			bridge.spawn_elf(cx + ox, 0, cz)
		# Vary initial elf food so some are hungry from the start.
		var food_max: int = 1_000_000_000_000_000
		var elf_food_pcts := [100, 90, 70, 60, 48]
		for i in elf_food_pcts.size():
			if elf_food_pcts[i] < 100:
				bridge.set_creature_food("Elf", i, food_max * elf_food_pcts[i] / 100)
		for i in 5:
			bridge.spawn_capybara(cx, 0, cz)
		for i in 3:
			bridge.spawn_creature("Boar", cx, 0, cz)
		for i in 3:
			bridge.spawn_creature("Deer", cx, 0, cz)
		for i in 3:
			bridge.spawn_creature("Monkey", cx, 0, cz)
		for i in 3:
			bridge.spawn_creature("Squirrel", cx, 0, cz)
		print("Elven Canopy: multiplayer game started, spawned initial creatures")

	_setup_common(bridge)
	print("Elven Canopy: multiplayer game ready")


## Set up renderers, toolbar, controllers, and menus. Called for both
## single-player (immediately in _ready) and multiplayer (after game_started).
func _setup_common(bridge: SimBridge) -> void:
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

	# Set up action toolbar UI (rendered on top of 3D via CanvasLayer).
	var canvas_layer := CanvasLayer.new()
	add_child(canvas_layer)

	var toolbar_script = load("res://scripts/action_toolbar.gd")
	var toolbar := MarginContainer.new()
	toolbar.set_script(toolbar_script)
	canvas_layer.add_child(toolbar)

	# Set up placement controller.
	var controller_script = load("res://scripts/placement_controller.gd")
	_placement_controller = Node3D.new()
	_placement_controller.set_script(controller_script)
	add_child(_placement_controller)
	_placement_controller.setup(bridge, $CameraPivot/Camera3D)
	_placement_controller.connect_toolbar(toolbar)

	# Set up construction controller.
	var construction_script = load("res://scripts/construction_controller.gd")
	_construction_controller = Node.new()
	_construction_controller.set_script(construction_script)
	add_child(_construction_controller)
	_construction_controller.setup(bridge, $CameraPivot)
	_construction_controller.connect_toolbar(toolbar)
	canvas_layer.add_child(_construction_controller.get_panel())

	# Entering construction mode: deselect creature, cancel placement, hide tree info.
	_construction_controller.construction_mode_entered.connect(
		func():
			if _placement_controller.is_placing():
				_placement_controller.cancel_placement()
			if _selector:
				_selector.deselect()
			if _panel:
				_panel.hide_panel()
			if _tree_info_panel and _tree_info_panel.visible:
				_tree_info_panel.hide_panel()
			if _camera_pivot:
				_camera_pivot.stop_follow()
	)

	# Set up blueprint renderer.
	var bp_renderer_script = load("res://scripts/blueprint_renderer.gd")
	_bp_renderer = Node3D.new()
	_bp_renderer.set_script(bp_renderer_script)
	add_child(_bp_renderer)
	_bp_renderer.setup(bridge)
	_construction_controller.blueprint_placed.connect(_bp_renderer.refresh)

	# Set up building renderer.
	var bldg_renderer_script = load("res://scripts/building_renderer.gd")
	_bldg_renderer = Node3D.new()
	_bldg_renderer.set_script(bldg_renderer_script)
	_bldg_renderer.name = "BuildingRenderer"
	add_child(_bldg_renderer)
	_bldg_renderer.setup(bridge)
	_construction_controller.blueprint_placed.connect(_bldg_renderer.refresh)

	# Set up creature info panel.
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
	_selector.set_placement_controller(_placement_controller)

	# Wire selection -> panel.
	_camera_pivot = $CameraPivot
	_selector.creature_selected.connect(
		func(species: String, index: int):
			# Mutual exclusion: selecting a creature hides the tree info panel.
			if _tree_info_panel and _tree_info_panel.visible:
				_tree_info_panel.hide_panel()
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

	# Menu button.
	var menu_btn := Button.new()
	menu_btn.text = "Menu"
	menu_btn.custom_minimum_size = Vector2(80, 40)
	menu_btn.set_anchors_preset(Control.PRESET_TOP_RIGHT)
	menu_btn.position = Vector2(-90, 10)
	canvas_layer.add_child(menu_btn)

	# Pause menu overlay.
	var pause_layer := CanvasLayer.new()
	pause_layer.layer = 2
	add_child(pause_layer)

	var pause_script = load("res://scripts/pause_menu.gd")
	var pause_menu := ColorRect.new()
	pause_menu.set_script(pause_script)
	pause_layer.add_child(pause_menu)
	pause_menu.setup(bridge)
	menu_btn.pressed.connect(pause_menu.toggle)

	# Task panel overlay.
	var task_panel_layer := CanvasLayer.new()
	task_panel_layer.layer = 2
	add_child(task_panel_layer)

	var task_panel_script = load("res://scripts/task_panel.gd")
	_task_panel = ColorRect.new()
	_task_panel.set_script(task_panel_script)
	task_panel_layer.add_child(_task_panel)

	# Structure list panel overlay (on CanvasLayer 2, added after task panel
	# so its ESC handler fires first in reverse tree order).
	var structure_panel_layer := CanvasLayer.new()
	structure_panel_layer.layer = 2
	add_child(structure_panel_layer)

	var structure_panel_script = load("res://scripts/structure_list_panel.gd")
	_structure_panel = ColorRect.new()
	_structure_panel.set_script(structure_panel_script)
	structure_panel_layer.add_child(_structure_panel)

	# Tree info panel (on its own CanvasLayer, added after structure panel
	# so its ESC handler fires first in reverse tree order).
	var tree_panel_layer := CanvasLayer.new()
	tree_panel_layer.layer = 1
	add_child(tree_panel_layer)

	var tree_panel_script = load("res://scripts/tree_info_panel.gd")
	_tree_info_panel = PanelContainer.new()
	_tree_info_panel.set_script(tree_panel_script)
	tree_panel_layer.add_child(_tree_info_panel)

	# Wire structure panel zoom-to-location -> move camera pivot.
	_structure_panel.zoom_to_location.connect(
		func(x: float, y: float, z: float):
			_structure_panel.hide_panel()
			_look_at_position(Vector3(x + 0.5, y, z + 0.5))
	)

	# Wire toolbar "Tasks", "Structures", and "TreeInfo" actions -> panel toggles.
	toolbar.action_requested.connect(
		func(action: String):
			if action == "Tasks":
				_task_panel.toggle()
			elif action == "Structures":
				_structure_panel.toggle()
			elif action == "TreeInfo":
				# Mutual exclusion: opening tree info deselects creature.
				if not _tree_info_panel.visible:
					if _selector:
						_selector.deselect()
					if _panel and _panel.visible:
						_panel.hide_panel()
					if _camera_pivot:
						_camera_pivot.stop_follow()
				_tree_info_panel.toggle()
	)

	_task_panel.zoom_to_creature.connect(
		func(species: String, index: int):
			_task_panel.hide_panel()
			_selector.select_creature(species, index)
			var tick := float(bridge.current_tick())
			var pos = _get_creature_world_pos(bridge, tick, species, index)
			if pos != null:
				_camera_pivot.start_follow(pos)
	)

	_task_panel.zoom_to_location.connect(
		func(x: float, y: float, z: float):
			_task_panel.hide_panel()
			_look_at_position(Vector3(x + 0.5, y, z + 0.5))
	)

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

	# Fix ESC precedence. _unhandled_input fires in reverse tree order (last
	# child first). Move the three input controllers to the end so they get
	# ESC before panels and the pause menu. Order after move:
	#   ... → pause_menu → task_panel → structure_panel → selector → construction → placement
	# Reverse (input order): placement → construction → selector → panels → pause_menu
	move_child(_selector, -1)
	move_child(_construction_controller, -1)
	move_child(_placement_controller, -1)


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
	if not bridge.is_initialized():
		return

	var render_tick: float = float(bridge.current_tick())

	if bridge.is_multiplayer():
		# Multiplayer: poll_network handles sim stepping via turns.
		var turns_applied := bridge.poll_network()
		if turns_applied > 0:
			_mp_time_since_turn = 0.0
		else:
			_mp_time_since_turn += delta
		# Smooth interpolation: advance render_tick up to ticks_per_turn ahead.
		var ticks_ahead := int(_mp_time_since_turn / _seconds_per_tick)
		var max_ticks := bridge.mp_ticks_per_turn()
		if ticks_ahead > max_ticks:
			ticks_ahead = max_ticks
		render_tick = float(bridge.current_tick()) + float(ticks_ahead)
		# Process multiplayer events (player join/leave, chat).
		var events := bridge.poll_mp_events()
		for event_json in events:
			print("MP event: %s" % event_json)
	else:
		# Single-player: time-based accumulator.
		_sim_accumulator += delta
		var ticks_to_advance := int(_sim_accumulator / _seconds_per_tick)
		if ticks_to_advance > 5000:
			ticks_to_advance = 5000
		if ticks_to_advance > 0:
			_sim_accumulator -= ticks_to_advance * _seconds_per_tick
			bridge.step_to_tick(bridge.current_tick() + ticks_to_advance)
		render_tick = float(bridge.current_tick()) + (_sim_accumulator / _seconds_per_tick)

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
	if _bldg_renderer:
		_bldg_renderer.refresh()

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

	# Refresh structure list panel while visible.
	if _structure_panel and _structure_panel.visible:
		var structures := bridge.get_structures()
		_structure_panel.update_structures(structures)

	# Refresh tree info panel while visible.
	if _tree_info_panel and _tree_info_panel.visible:
		var tree_data := bridge.get_home_tree_info()
		_tree_info_panel.update_info(tree_data)

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
