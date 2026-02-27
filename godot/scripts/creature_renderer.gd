## Generic creature renderer — renders any species as billboard chibi sprites.
##
## Parameterized replacement for per-species renderers (elf_renderer.gd,
## capybara_renderer.gd). Configured via setup(bridge, species_name, y_offset)
## after instantiation. Each frame, reads positions from
## SimBridge.get_creature_positions(species_name, render_tick) and places a
## Sprite3D at each one with smooth interpolation.
##
## Uses the same pool pattern as elf_renderer.gd: sprites are created on
## demand via SpriteFactory, never destroyed, and hidden when the count drops.
##
## The Y offset centers the sprite above the nav node floor position. Each
## species has a different sprite height, so the offset varies:
## Boar: 0.38, Deer: 0.46, Monkey: 0.44, Squirrel: 0.28
##
## See also: sprite_factory.gd for species_params_from_seed / create_species_sprite,
## elf_renderer.gd / capybara_renderer.gd for the original per-species renderers,
## sim_bridge.rs for get_creature_positions, main.gd which creates and configures
## instances of this renderer.

extends Node3D

var _bridge: SimBridge
var _species_name: String
var _y_offset: float
var _sprites: Array[Sprite3D] = []
var _render_tick: float = 0.0


## Configure the renderer for a specific species. Call once after adding to
## the scene tree.
func setup(bridge: SimBridge, species_name: String, y_offset: float) -> void:
	_bridge = bridge
	_species_name = species_name
	_y_offset = y_offset


## Set the fractional render tick for smooth movement interpolation.
## Called by main.gd each frame after stepping the sim.
func set_render_tick(tick: float) -> void:
	_render_tick = tick


func _process(_delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	var positions := _bridge.get_creature_positions(_species_name, _render_tick)
	var count := positions.size()

	# Add sprites if we have more creatures than sprites.
	while _sprites.size() < count:
		var idx := _sprites.size()
		var params = SpriteFactory.species_params_from_seed(_species_name, idx)
		var sprite := Sprite3D.new()
		sprite.texture = SpriteFactory.create_species_sprite(_species_name, params)
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = 0.02
		sprite.transparent = true
		sprite.no_depth_test = false
		add_child(sprite)
		_sprites.append(sprite)

	# Update positions and hide excess sprites.
	for i in _sprites.size():
		if i < count:
			_sprites[i].visible = true
			var pos := positions[i]
			_sprites[i].global_position = Vector3(pos.x + 0.5, pos.y + _y_offset, pos.z + 0.5)
		else:
			_sprites[i].visible = false
