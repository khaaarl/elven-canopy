## Floating blue swirl VFX for mana-depleted work actions.
##
## Each frame, polls SimBridge for mana-wasted positions (creatures that
## attempted a work action without enough mana). At each position, spawns
## 3 small blue swirl sprites that drift upward with slight horizontal
## spread and fade out over ~0.8 seconds.
##
## Uses a pool of billboard Sprite3D nodes to avoid per-frame allocation.
## Idle sprites are hidden and reused. The swirl texture is a procedural
## 12x12 blue spiral glyph generated once at startup.
##
## See also: hp_bar.gd (overhead bar rendering), creature_renderer.gd (which
## manages creature sprites), sim_bridge.rs get_mana_wasted_positions().

extends Node3D

## Swirls spawned per wasted-action event.
const SWIRL_COUNT := 3
## World units per second upward drift.
const DRIFT_SPEED := 0.6
## Horizontal random spread radius.
const SPREAD := 0.15
## Seconds before fade-out completes.
const LIFETIME := 0.8
## Sprite scale (12px * 0.015 = 0.18 world units).
const PIXEL_SIZE := 0.015
## Max concurrent swirl sprites.
const POOL_SIZE := 32

var _bridge: SimBridge
var _sprites: Array[Sprite3D] = []
## Per-sprite state: [age, dx, dz, start_x, start_y, start_z]
var _state: Array[PackedFloat32Array] = []
var _active_count: int = 0
var _texture: ImageTexture
var _rng := RandomNumberGenerator.new()


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_texture = _generate_swirl_texture()
	# Pre-allocate sprite pool.
	for i in POOL_SIZE:
		var sprite := Sprite3D.new()
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = PIXEL_SIZE
		sprite.transparent = true
		sprite.no_depth_test = false
		sprite.render_priority = 2
		sprite.texture = _texture
		sprite.visible = false
		add_child(sprite)
		_sprites.append(sprite)
		_state.append(PackedFloat32Array([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]))


func _process(delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	# Update existing active sprites (drift upward, fade, expire).
	var i := 0
	while i < _active_count:
		var s := _state[i]
		s[0] += delta  # age
		if s[0] >= LIFETIME:
			# Expired: swap with last active and hide.
			_sprites[i].visible = false
			_active_count -= 1
			if i < _active_count:
				# Swap sprite references and state.
				var tmp_sprite := _sprites[i]
				_sprites[i] = _sprites[_active_count]
				_sprites[_active_count] = tmp_sprite
				var tmp_state := _state[i]
				_state[i] = _state[_active_count]
				_state[_active_count] = tmp_state
			continue  # re-check this index (it's now a different sprite)
		# Drift upward + slight horizontal wobble.
		var t: float = s[0] / LIFETIME  # 0..1
		var y_offset: float = DRIFT_SPEED * s[0]
		_sprites[i].global_position = Vector3(s[3] + s[1], s[4] + y_offset, s[5] + s[2])
		# Fade out: modulate alpha from 1.0 to 0.0.
		var alpha: float = 1.0 - t * t  # quadratic ease-out
		_sprites[i].modulate = Color(1.0, 1.0, 1.0, alpha)
		_state[i] = s
		i += 1

	# Poll for new mana-wasted positions and spawn swirls.
	var positions: Array = _bridge.get_mana_wasted_positions()
	for pos: Vector3 in positions:
		for _j in SWIRL_COUNT:
			if _active_count >= POOL_SIZE:
				break
			var idx := _active_count
			_active_count += 1
			var dx: float = _rng.randf_range(-SPREAD, SPREAD)
			var dz: float = _rng.randf_range(-SPREAD, SPREAD)
			_state[idx] = PackedFloat32Array([0.0, dx, dz, pos.x, pos.y, pos.z])
			_sprites[idx].global_position = Vector3(pos.x + dx, pos.y, pos.z + dz)
			_sprites[idx].modulate = Color(1.0, 1.0, 1.0, 1.0)
			_sprites[idx].visible = true


## Generate a 12x12 blue spiral glyph texture.
static func _generate_swirl_texture() -> ImageTexture:
	var size := 12
	var img := Image.create(size, size, false, Image.FORMAT_RGBA8)
	var center := Vector2(size / 2.0, size / 2.0)
	var max_r := size / 2.0
	# Draw a stylized spiral using polar coordinates.
	for y in size:
		for x in size:
			var p := Vector2(x + 0.5, y + 0.5) - center
			var r := p.length()
			if r > max_r:
				img.set_pixel(x, y, Color(0.0, 0.0, 0.0, 0.0))
				continue
			var angle := atan2(p.y, p.x)
			# Spiral arm: sin of (angle + r * twist_factor).
			var spiral := sin(angle * 2.0 + r * 1.5)
			var brightness := maxf(0.0, spiral) * (1.0 - r / max_r)
			var alpha := brightness * 0.9
			if alpha < 0.05:
				img.set_pixel(x, y, Color(0.0, 0.0, 0.0, 0.0))
			else:
				img.set_pixel(x, y, Color(0.3, 0.5, 1.0, alpha))
	return ImageTexture.create_from_image(img)
