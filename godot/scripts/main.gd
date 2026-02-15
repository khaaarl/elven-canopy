## Main scene controller for Elven Canopy.
##
## Initializes the simulation bridge, sets up tree, elf, and capybara
## renderers, spawns initial creatures, and wires up the spawn toolbar UI
## with the placement controller. Steps the simulation forward each frame.
##
## See also: orbital_camera.gd for camera controls, SimBridge (Rust) for
## the simulation interface, tree_renderer.gd, elf_renderer.gd,
## capybara_renderer.gd for visual representation, spawn_toolbar.gd for
## the spawn UI, placement_controller.gd for click-to-place logic.

extends Node3D

## The simulation seed. Deterministic: same seed = same game.
@export var sim_seed: int = 42


func _ready() -> void:
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


func _process(_delta: float) -> void:
	var bridge: SimBridge = $SimBridge
	if bridge.is_initialized():
		bridge.step_to_tick(bridge.current_tick() + 1)
