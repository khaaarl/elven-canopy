## Unified creature renderer — renders all species as billboard chibi sprites.
##
## One instance handles every creature in the game. Each frame, calls
## SimBridge.get_creature_render_data() which returns positions, HP/MP,
## incapacitation, and sprite textures for ALL alive creatures in one call.
## Textures are stored in the central CreatureSprites cache so that UI panels
## get the same sprites as the in-world renderer.
##
## Uses a pool pattern: Sprite3D nodes are created on demand, never destroyed,
## and hidden when the count drops. Each sprite has an overhead HP bar. MP bars
## are created on demand for creatures that have mana (mp_ratio < 1.0).
##
## Species Y offsets (vertical sprite positioning) are loaded from
## SpeciesData config via bridge.get_species_display_info() at setup time,
## rather than hardcoded.
##
## See also: creature_sprites.gd for the central sprite cache,
## elven_canopy_sprites (Rust crate) for species sprite generation,
## hp_bar.gd for overhead HP/MP bar rendering, sim_bridge.rs for
## get_creature_render_data, main.gd which creates this renderer.

extends Node3D

const HpBar = preload("res://scripts/hp_bar.gd")
## Vertical gap between the top of the creature sprite and the HP bar center.
const HP_BAR_GAP := 0.06
## MP bar sits just below the HP bar (negative = lower).
const MP_BAR_GAP := -0.01

var _bridge: SimBridge
var _sprites: Array[Sprite3D] = []
var _hp_bars: Array[Sprite3D] = []
## MP bars, keyed by pool index. Only created for creatures that have mana.
var _mp_bars: Dictionary = {}
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

	var data: Dictionary = _bridge.get_creature_render_data(_render_tick)
	var ids: PackedStringArray = data.get("creature_ids", PackedStringArray())
	var species_arr: PackedStringArray = data.get("species", PackedStringArray())
	var positions: PackedVector3Array = data.get("positions", PackedVector3Array())
	var hp_ratios: PackedFloat32Array = data.get("hp_ratios", PackedFloat32Array())
	var mp_ratios: PackedFloat32Array = data.get("mp_ratios", PackedFloat32Array())
	var incap_flags: PackedByteArray = data.get("incap_flags", PackedByteArray())
	var count := positions.size()

	# Grow sprite pool if needed.
	while _sprites.size() < count:
		var sprite := Sprite3D.new()
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = 0.02
		sprite.transparent = true
		sprite.no_depth_test = false
		sprite.render_priority = 1
		add_child(sprite)
		_sprites.append(sprite)
		var bar: Sprite3D = HpBar.create_bar_sprite()
		add_child(bar)
		_hp_bars.append(bar)

	# Update positions, HP/MP bars, textures, and hide excess sprites.
	for i in _sprites.size():
		if i < count:
			_sprites[i].visible = true
			var pos := positions[i]
			var cid: String = ids[i]
			var sp: String = species_arr[i] if i < species_arr.size() else ""
			var y_off: float = CreatureSprites.get_y_offset(sp)
			var is_incap := i < incap_flags.size() and incap_flags[i] != 0
			_sprites[i].global_position = Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
			# Read textures from central cache.
			if is_incap:
				_sprites[i].texture = CreatureSprites.get_fallen_sprite(_bridge, cid)
				HpBar.update_bar_incapacitated(_hp_bars[i])
			else:
				_sprites[i].texture = CreatureSprites.get_sprite(_bridge, cid)
				var ratio: float = hp_ratios[i] if i < hp_ratios.size() else 1.0
				HpBar.update_bar(_hp_bars[i], ratio)
			_hp_bars[i].global_position = Vector3(
				pos.x + 0.5, pos.y + y_off * 2.0 + HP_BAR_GAP, pos.z + 0.5
			)
			# MP bar (created on demand for creatures with mana).
			var mp_ratio: float = mp_ratios[i] if i < mp_ratios.size() else 1.0
			if mp_ratio < 1.0:
				if not _mp_bars.has(i):
					var mp_bar: Sprite3D = HpBar.create_mp_bar_sprite()
					add_child(mp_bar)
					_mp_bars[i] = mp_bar
				HpBar.update_mp_bar(_mp_bars[i], mp_ratio)
				_mp_bars[i].global_position = Vector3(
					pos.x + 0.5, pos.y + y_off * 2.0 + MP_BAR_GAP, pos.z + 0.5
				)
			elif _mp_bars.has(i):
				_mp_bars[i].visible = false
		else:
			_sprites[i].visible = false
			_hp_bars[i].visible = false
			if _mp_bars.has(i):
				_mp_bars[i].visible = false
