## Renders capybaras as billboard chibi sprites driven by the simulation.
##
## Mirrors the elf_renderer.gd pattern: reads positions from
## SimBridge.get_capybara_positions() each frame, creates Sprite3D nodes
## on demand, and hides extras when counts shrink.
##
## See also: sprite_factory.gd for texture generation, sim_bridge.rs for
## capybara position data, main.gd which creates this node and calls setup().

extends Node3D

var _bridge: SimBridge
var _capybara_sprites: Array[Sprite3D] = []


## Call after SimBridge is initialized.
func setup(bridge: SimBridge) -> void:
	_bridge = bridge


func _process(_delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	var positions := _bridge.get_capybara_positions()
	var count := positions.size()

	# Add sprites if we have more capybaras than sprites.
	while _capybara_sprites.size() < count:
		var idx := _capybara_sprites.size()
		var params = SpriteFactory.capybara_params_from_seed(idx)
		var sprite := Sprite3D.new()
		sprite.texture = SpriteFactory.create_capybara(params)
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = 0.06  # 40px * 0.06 = 2.4 world units wide
		sprite.transparent = true
		sprite.no_depth_test = false
		add_child(sprite)
		_capybara_sprites.append(sprite)

	# Update positions and hide excess sprites.
	for i in _capybara_sprites.size():
		if i < count:
			_capybara_sprites[i].visible = true
			var pos := positions[i]
			_capybara_sprites[i].global_position = Vector3(pos.x + 0.5, pos.y + 0.5, pos.z + 0.5)
		else:
			_capybara_sprites[i].visible = false
