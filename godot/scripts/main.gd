## Main scene controller for Elven Canopy.
##
## Orchestrates startup and the per-frame sim loop. Everything in the game
## connects through this script.
##
## Startup sequence (_ready):
## 1. Read the simulation seed from GameSession autoload (set by the new-game
##    menu), falling back to the @export default for direct scene launches.
## 2. Initialize SimBridge with the seed.
## 3. Set up renderers: tree_renderer.gd (static voxel mesh),
##    elf_renderer.gd and capybara_renderer.gd (billboard sprites).
## 4. Spawn initial creatures at the tree base via SimBridge commands.
## 5. Create the spawn toolbar UI (spawn_toolbar.gd) on a CanvasLayer so
##    it renders on top of the 3D viewport.
## 6. Create the placement controller (placement_controller.gd) and wire
##    it to both the SimBridge and the toolbar's signals.
## 7. Create the creature info panel (creature_info_panel.gd) and
##    selection controller (selection_controller.gd), wire them to the
##    camera for follow mode.
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
## carries the seed from the menu.

extends Node3D

## The simulation seed. Deterministic: same seed = same game.
## Overridden by GameSession.sim_seed when launched through the menu flow.
## The @export default (42) is a fallback for direct scene launches (F6 in editor).
@export var sim_seed: int = 42

var _selector: Node3D
var _panel: PanelContainer
var _camera_pivot: Node3D


func _ready() -> void:
	# Use seed from GameSession autoload if available (normal flow through menus).
	# Fall back to the @export var if GameSession hasn't been set (direct scene launch).
	if GameSession.sim_seed >= 0:
		sim_seed = GameSession.sim_seed

	var bridge: SimBridge = $SimBridge
	bridge.init_sim(sim_seed)
	print("Elven Canopy: sim initialized (seed=%d, mana=%.1f)" % [sim_seed, bridge.home_tree_mana()])

	# Set up tree renderer.
	var tree_renderer = $TreeRenderer
	tree_renderer.setup(bridge)

	# Set up elf renderer.
	var elf_renderer = $ElfRenderer
	elf_renderer.setup(bridge)

	# Set up capybara renderer (sim-driven).
	var capybara_renderer = $CapybaraRenderer
	capybara_renderer.setup(bridge)

	# Spawn elves at the tree base to demonstrate chibi variety.
	# The world center is world_size/2 (128 for default 256 world).
	var cx := 128
	var cz := 128
	for i in 5:
		var ox := i * 3 - 6  # Spread elves along X axis
		bridge.spawn_elf(cx + ox, 0, cz)
	print("Elven Canopy: spawned %d elves near (%d, 0, %d)" % [bridge.elf_count(), cx, cz])

	# Spawn capybaras at ground level.
	for i in 5:
		bridge.spawn_capybara(cx, 0, cz)
	print("Elven Canopy: spawned %d capybaras near (%d, 0, %d)" % [bridge.capybara_count(), cx, cz])

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

	# Wire selection → panel.
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

	# Wire follow button → camera.
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
