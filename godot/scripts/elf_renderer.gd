## Renders elves as billboard chibi sprites driven by the simulation.
##
## Each frame, reads elf positions from SimBridge.get_elf_positions(render_tick)
## and places a Sprite3D at each one. Positions are smoothly interpolated
## between nav nodes using the fractional render_tick computed by main.gd
## (sim tick + accumulator fraction). Call set_render_tick() each frame before
## _process runs.
##
## Uses a pool pattern: sprites are created on demand (never destroyed), and
## excess sprites are hidden when the elf count drops. Each sprite's texture
## is generated once by SpriteFactory using the sprite's index as a
## deterministic seed, giving every elf a unique appearance.
##
## Sprites use BILLBOARD_ENABLED so they always face the camera. Positions
## are offset by (+0.5, +0.48, +0.5) from the interpolated coordinate — the
## X/Z offset centers the sprite on the voxel, and the Y offset places the
## sprite center half its height above the floor. At pixel_size 0.02, the
## 48px sprite is 0.96 world units tall (~1.9m given 2m voxels).
##
## See also: sprite_factory.gd for chibi elf texture generation (48x48),
## capybara_renderer.gd for the equivalent capybara renderer, sim_bridge.rs
## for the Rust-side position data, main.gd which creates this node and
## calls setup() and set_render_tick().

extends Node3D

var _bridge: SimBridge
var _elf_sprites: Array[Sprite3D] = []
var _render_tick: float = 0.0


## Call after SimBridge is initialized.
func setup(bridge: SimBridge) -> void:
	_bridge = bridge


## Set the fractional render tick for smooth movement interpolation.
## Called by main.gd each frame after stepping the sim.
func set_render_tick(tick: float) -> void:
	_render_tick = tick


func _process(_delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	var positions := _bridge.get_elf_positions(_render_tick)
	var elf_count := positions.size()

	# Add sprites if we have more elves than sprites.
	while _elf_sprites.size() < elf_count:
		var idx := _elf_sprites.size()
		var params = SpriteFactory.elf_params_from_seed(idx)
		var sprite := Sprite3D.new()
		sprite.texture = SpriteFactory.create_chibi_elf(params)
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = 0.02  # Scale: 48px * 0.02 = 0.96 world units (~1.9m)
		sprite.transparent = true
		sprite.no_depth_test = false
		add_child(sprite)
		_elf_sprites.append(sprite)

	# Hide excess sprites.
	for i in _elf_sprites.size():
		if i < elf_count:
			_elf_sprites[i].visible = true
			var pos := positions[i]
			# Nav node pos is the air voxel; feet at pos.y, center at +half sprite height.
			_elf_sprites[i].global_position = Vector3(pos.x + 0.5, pos.y + 0.48, pos.z + 0.5)
		else:
			_elf_sprites[i].visible = false
