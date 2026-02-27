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
## 4. Common path: set up renderers, toolbar, placement controller, selection
##    controller, creature info panel, menu button, and pause menu.
##
## Per-frame (_process): advances the sim by one tick via
## SimBridge.step_to_tick(). If the camera is following a creature, updates
## the follow target and refreshes the info panel.
##
## See also: orbital_camera.gd for camera controls, sim_bridge.rs (Rust)
## for the simulation interface, tree_renderer.gd / elf_renderer.gd /
## capybara_renderer.gd for rendering, spawn_toolbar.gd for the toolbar
## UI, placement_controller.gd for click-to-place logic,
## selection_controller.gd for click-to-select, creature_info_panel.gd
## for the creature info panel, game_session.gd for the autoload that
## carries the seed/load path from the menu, pause_menu.gd for the ESC
## pause overlay.

extends Node3D

## The simulation seed. Deterministic: same seed = same game.
## Overridden by GameSession.sim_seed when launched through the menu flow.
## The @export default (42) is a fallback for direct scene launches (F6 in editor).
@export var sim_seed: int = 42

var _selector: Node3D
var _panel: PanelContainer
var _camera_pivot: Node3D


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
		print("Elven Canopy: sim initialized (seed=%d, mana=%.1f)" % [sim_seed, bridge.home_tree_mana()])

		# Spawn initial creatures.
		var cx := 128
		var cz := 128
		for i in 5:
			var ox := i * 3 - 6
			bridge.spawn_elf(cx + ox, 0, cz)
		print("Elven Canopy: spawned %d elves near (%d, 0, %d)" % [bridge.elf_count(), cx, cz])

		for i in 5:
			bridge.spawn_capybara(cx, 0, cz)
		print("Elven Canopy: spawned %d capybaras near (%d, 0, %d)" % [bridge.capybara_count(), cx, cz])

	# --- Common setup (new game and loaded game) ---

	# Set up tree renderer.
	var tree_renderer = $TreeRenderer
	tree_renderer.setup(bridge)

	# Set up elf renderer.
	var elf_renderer = $ElfRenderer
	elf_renderer.setup(bridge)

	# Set up capybara renderer (sim-driven).
	var capybara_renderer = $CapybaraRenderer
	capybara_renderer.setup(bridge)

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
	_selector.creature_selected.connect(func(species: String, index: int):
		var info := bridge.get_creature_info(species, index)
		if not info.is_empty():
			_panel.show_creature(species, index, info)
	)
	_selector.creature_deselected.connect(func():
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

	# Wire follow button -> camera.
	_panel.follow_requested.connect(func():
		var pos = _get_creature_world_pos(bridge,
			_selector.get_selected_species(), _selector.get_selected_index())
		if pos != null:
			_camera_pivot.start_follow(pos)
	)
	_panel.unfollow_requested.connect(func():
		_camera_pivot.stop_follow()
	)
	_panel.panel_closed.connect(func():
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


func _process(_delta: float) -> void:
	var bridge: SimBridge = $SimBridge
	if bridge.is_initialized():
		bridge.step_to_tick(bridge.current_tick() + 1)

	# Update follow target each frame so the camera tracks creature movement.
	if _camera_pivot and _camera_pivot.is_following():
		var pos = _get_creature_world_pos(bridge,
			_selector.get_selected_species(), _selector.get_selected_index())
		if pos != null:
			_camera_pivot.update_follow_target(pos)
		else:
			_camera_pivot.stop_follow()
			_panel.set_follow_state(false)

	# Detect if camera broke follow via movement keys.
	if _panel and _panel.visible and not _camera_pivot.is_following():
		_panel.set_follow_state(false)

	# Refresh panel info while a creature is selected.
	if _panel and _panel.visible and _selector.get_selected_index() >= 0:
		var info := bridge.get_creature_info(
			_selector.get_selected_species(), _selector.get_selected_index())
		if not info.is_empty():
			_panel.update_info(info)


## Get the world-space position of a creature sprite, matching the offsets
## used by elf_renderer.gd (+0.48 Y) and capybara_renderer.gd (+0.32 Y).
func _get_creature_world_pos(bridge: SimBridge, species: String,
		index: int) -> Variant:
	if species == "Elf":
		var positions := bridge.get_elf_positions()
		if index >= 0 and index < positions.size():
			var p := positions[index]
			return Vector3(p.x + 0.5, p.y + 0.48, p.z + 0.5)
	elif species == "Capybara":
		var positions := bridge.get_capybara_positions()
		if index >= 0 and index < positions.size():
			var p := positions[index]
			return Vector3(p.x + 0.5, p.y + 0.32, p.z + 0.5)
	return null
