## Main scene controller for Elven Canopy.
##
## Initializes the simulation bridge and coordinates between the Godot
## scene tree and the Rust simulation. In Phase 0 this is minimal:
## it creates the sim with a fixed seed and steps it forward each frame.
##
## See also: orbital_camera.gd for camera controls, SimBridge (Rust) for
## the simulation interface.

extends Node3D

## The simulation seed. Deterministic: same seed = same game.
@export var sim_seed: int = 42


func _ready() -> void:
	var bridge: SimBridge = $SimBridge
	bridge.init_sim(sim_seed)
	print("Elven Canopy: sim initialized (seed=%d, mana=%.1f)" % [sim_seed, bridge.home_tree_mana()])


func _process(_delta: float) -> void:
	# In Phase 0, we step the sim forward by 1 tick per frame.
	# Later this will be driven by sim speed and frame timing.
	var bridge: SimBridge = $SimBridge
	if bridge.is_initialized():
		bridge.step_to_tick(bridge.current_tick() + 1)
