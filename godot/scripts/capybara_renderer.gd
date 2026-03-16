## Renders capybaras as billboard chibi sprites driven by the simulation.
##
## Mirrors the elf_renderer.gd pool pattern: reads positions from
## SimBridge.get_capybara_positions(render_tick) each frame, creates Sprite3D
## nodes on demand, and hides extras when counts shrink. Positions are smoothly
## interpolated between nav nodes using the fractional render_tick computed by
## main.gd. Call set_render_tick() each frame before _process runs.
##
## Each capybara gets a unique texture from SpriteGenerator using the sprite
## index as a seed (varying body color and accessory). Each sprite has an
## overhead HP bar (see hp_bar.gd) shown only when HP is below maximum.
##
## Positions are offset by (+0.5, +0.32, +0.5) from the interpolated
## coordinate. The Y offset places the sprite center half its height above
## the floor. At pixel_size 0.02, the 32px-tall sprite is 0.64 world units
## (~1.3m given 2m voxels).
##
## See also: elven_canopy_sprites (Rust crate) for capybara texture generation (40x32),
## hp_bar.gd for overhead HP bar rendering, elf_renderer.gd for the
## equivalent elf renderer, sim_bridge.rs for the Rust-side position data,
## main.gd which creates this node and calls setup() and set_render_tick().

extends Node3D

const HpBar = preload("res://scripts/hp_bar.gd")
const Y_OFFSET := 0.32
const HP_BAR_GAP := 0.06

var _bridge: SimBridge
var _capybara_sprites: Array[Sprite3D] = []
var _hp_bars: Array[Sprite3D] = []
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

	var positions := _bridge.get_capybara_positions(_render_tick)
	var hp_ratios := _bridge.get_creature_hp_ratios("Capybara")
	var count := positions.size()

	# Add sprites if we have more capybaras than sprites.
	while _capybara_sprites.size() < count:
		var idx := _capybara_sprites.size()
		var sprite := Sprite3D.new()
		sprite.texture = SpriteGenerator.species_sprite("Capybara", idx)
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = 0.02  # 40px * 0.02 = 0.80 world units wide
		sprite.transparent = true
		sprite.no_depth_test = false
		sprite.render_priority = 1
		add_child(sprite)
		_capybara_sprites.append(sprite)
		var bar: Sprite3D = HpBar.create_bar_sprite()
		add_child(bar)
		_hp_bars.append(bar)

	# Update positions, HP bars, and hide excess sprites.
	for i in _capybara_sprites.size():
		if i < count:
			_capybara_sprites[i].visible = true
			var pos := positions[i]
			# Nav node pos is the air voxel; feet at pos.y, center at +half sprite height.
			_capybara_sprites[i].global_position = Vector3(
				pos.x + 0.5, pos.y + Y_OFFSET, pos.z + 0.5
			)
			var ratio: float = hp_ratios[i] if i < hp_ratios.size() else 1.0
			HpBar.update_bar(_hp_bars[i], ratio)
			_hp_bars[i].global_position = Vector3(
				pos.x + 0.5, pos.y + Y_OFFSET * 2.0 + HP_BAR_GAP, pos.z + 0.5
			)
		else:
			_capybara_sprites[i].visible = false
			_hp_bars[i].visible = false
