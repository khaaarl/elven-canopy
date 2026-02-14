## Main scene controller for Elven Canopy.
##
## Initializes the simulation bridge, sets up tree and elf renderers,
## and spawns an initial elf at the tree base. Steps the simulation
## forward each frame.
##
## See also: orbital_camera.gd for camera controls, SimBridge (Rust) for
## the simulation interface, tree_renderer.gd and elf_renderer.gd for
## visual representation.

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

	# Spawn one elf at the tree base (world center at y=0).
	# The world center is world_size/2 (128 for default 256 world).
	var cx := 128
	var cz := 128
	bridge.spawn_elf(cx, 0, cz)
	print("Elven Canopy: spawned elf at (%d, 0, %d), elf count=%d" % [cx, cz, bridge.elf_count()])


func _process(_delta: float) -> void:
	var bridge: SimBridge = $SimBridge
	if bridge.is_initialized():
		bridge.step_to_tick(bridge.current_tick() + 1)
