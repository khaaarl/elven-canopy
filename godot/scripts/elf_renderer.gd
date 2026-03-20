## Renders elves as billboard chibi sprites driven by the simulation.
##
## Each frame, reads elf positions and sprite textures from SimBridge. Sprite
## textures are cached in Rust (keyed on CreatureDrawInfo — seed, equipment,
## wear state); the bridge returns textures and per-elf change flags so we
## only set_texture on sprites that actually changed.
##
## Uses a pool pattern: sprites are created on demand (never destroyed), and
## excess sprites are hidden when the elf count drops. Each sprite has an
## overhead HP bar (red/yellow/green) and MP bar (blue), both from hp_bar.gd.
## HP bars show when HP is below maximum; MP bars show when mana is below max.
##
## Sprites use BILLBOARD_ENABLED so they always face the camera. Positions
## are offset by (+0.5, +0.48, +0.5) from the interpolated coordinate — the
## X/Z offset centers the sprite on the voxel, and the Y offset places the
## sprite center half its height above the floor. At pixel_size 0.02, the
## 48px sprite is 0.96 world units tall (~1.9m given 2m voxels).
##
## See also: elven_canopy_sprites (Rust crate) for chibi elf texture generation
## (48x48), elf_equipment.rs for equipment overlay drawing, hp_bar.gd for
## overhead HP bar rendering, sim_bridge.rs for the Rust-side sprite cache,
## main.gd which creates this node and calls setup() and set_render_tick().

extends Node3D

const HpBar = preload("res://scripts/hp_bar.gd")
const Y_OFFSET := 0.48
const HP_BAR_GAP := 0.06
const MP_BAR_GAP := -0.01  # MP bar sits just below the HP bar (negative = lower)

var _bridge: SimBridge
var _elf_sprites: Array[Sprite3D] = []
var _hp_bars: Array[Sprite3D] = []
var _mp_bars: Array[Sprite3D] = []
var _render_tick: float = 0.0


## Call after SimBridge is initialized.
func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	HpBar.ensure_cache()


## Set the fractional render tick for smooth movement interpolation.
## Called by main.gd each frame after stepping the sim.
func set_render_tick(tick: float) -> void:
	_render_tick = tick


func _process(_delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	var positions := _bridge.get_elf_positions(_render_tick)
	var hp_ratios := _bridge.get_creature_hp_ratios("Elf")
	var mp_ratios := _bridge.get_creature_mp_ratios("Elf")
	var incap_flags := _bridge.get_creature_incapacitated("Elf")
	var sprite_data: Dictionary = _bridge.get_elf_sprites()
	var sprite_textures: Array = sprite_data.get("textures", [])
	var sprite_changed: PackedByteArray = sprite_data.get("changed", PackedByteArray())
	var elf_count := positions.size()

	# Add sprites if we have more elves than sprites.
	while _elf_sprites.size() < elf_count:
		var idx := _elf_sprites.size()
		var sprite := Sprite3D.new()
		# Initial texture comes from the Rust cache (already in sprite_textures).
		if idx < sprite_textures.size():
			sprite.texture = sprite_textures[idx]
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = 0.02  # Scale: 48px * 0.02 = 0.96 world units (~1.9m)
		sprite.transparent = true
		sprite.no_depth_test = false
		sprite.render_priority = 1
		add_child(sprite)
		_elf_sprites.append(sprite)
		var bar: Sprite3D = HpBar.create_bar_sprite()
		add_child(bar)
		_hp_bars.append(bar)
		var mp_bar: Sprite3D = HpBar.create_mp_bar_sprite()
		add_child(mp_bar)
		_mp_bars.append(mp_bar)

	# Update positions, HP bars, textures, and hide excess sprites.
	for i in _elf_sprites.size():
		if i < elf_count:
			_elf_sprites[i].visible = true
			var pos := positions[i]
			var is_incap := i < incap_flags.size() and incap_flags[i] != 0
			# Nav node pos is the air voxel; feet at pos.y, center at +half sprite height.
			_elf_sprites[i].global_position = Vector3(pos.x + 0.5, pos.y + Y_OFFSET, pos.z + 0.5)
			# Rotate sprite 90 degrees around Z axis when incapacitated (falls sideways).
			if is_incap:
				_elf_sprites[i].rotation_degrees = Vector3(0.0, 0.0, 90.0)
			else:
				_elf_sprites[i].rotation_degrees = Vector3.ZERO
			if is_incap:
				HpBar.update_bar_incapacitated(_hp_bars[i])
			else:
				var ratio: float = hp_ratios[i] if i < hp_ratios.size() else 1.0
				HpBar.update_bar(_hp_bars[i], ratio)
			_hp_bars[i].global_position = Vector3(
				pos.x + 0.5, pos.y + Y_OFFSET * 2.0 + HP_BAR_GAP, pos.z + 0.5
			)
			var mp_ratio: float = mp_ratios[i] if i < mp_ratios.size() else 1.0
			HpBar.update_mp_bar(_mp_bars[i], mp_ratio)
			_mp_bars[i].global_position = Vector3(
				pos.x + 0.5, pos.y + Y_OFFSET * 2.0 + MP_BAR_GAP, pos.z + 0.5
			)
			# Only update texture when Rust reports a change.
			if i < sprite_changed.size() and sprite_changed[i] != 0:
				if i < sprite_textures.size():
					_elf_sprites[i].texture = sprite_textures[i]
		else:
			_elf_sprites[i].visible = false
			_elf_sprites[i].rotation_degrees = Vector3.ZERO
			_hp_bars[i].visible = false
			_mp_bars[i].visible = false
