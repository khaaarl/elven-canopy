## Shared HP bar utilities for creature renderers.
##
## Provides static helpers to create and update overhead HP bar sprites.
## Used by elf_renderer.gd, capybara_renderer.gd, and creature_renderer.gd
## to display thin health bars above creatures whose HP is below maximum.
##
## HP bars are small billboard Sprite3D nodes (20x3 px at 0.02 pixel_size =
## 0.40 x 0.06 world units) positioned above the creature sprite. The bar
## shows green/yellow/red fill proportional to current HP, with a dark
## background for the missing portion. Hidden when HP is full to avoid
## peacetime clutter.
##
## Textures are cached: 21 pre-generated textures (0%, 5%, ..., 100% fill)
## avoid per-frame allocation. Each renderer calls ensure_cache() once, then
## picks textures by quantized ratio.
##
## See also: elf_renderer.gd, capybara_renderer.gd, creature_renderer.gd
## which use these helpers, sim_bridge.rs get_creature_hp_ratios() for the
## data source.

extends RefCounted

const BAR_W := 20
const BAR_H := 3
const PIXEL_SIZE := 0.02
const STEPS := 20

## Cached textures indexed by fill level (0 = empty, 20 = full).
static var _cache: Array[ImageTexture] = []


## Ensure the texture cache is populated. Call once from each renderer's
## setup() or first _process(). Safe to call multiple times.
static func ensure_cache() -> void:
	if _cache.size() > 0:
		return
	_cache.resize(STEPS + 1)
	for step in range(STEPS + 1):
		var ratio := float(step) / float(STEPS)
		_cache[step] = _generate_bar_texture(ratio)


## Create an HP bar Sprite3D suitable for adding as a child of the creature
## sprite or as a sibling at the same position. Returns the sprite configured
## with billboard mode, transparency, and an initial full-HP (invisible) state.
static func create_bar_sprite() -> Sprite3D:
	var bar := Sprite3D.new()
	bar.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	bar.pixel_size = PIXEL_SIZE
	bar.transparent = true
	bar.no_depth_test = false
	bar.render_priority = 1
	bar.visible = false
	# Start with full HP texture (invisible anyway).
	if _cache.size() > 0:
		bar.texture = _cache[STEPS]
	return bar


## Update an HP bar sprite's texture and visibility based on the HP ratio.
## ratio should be 0.0–1.0. Bar is hidden when ratio >= 1.0.
static func update_bar(bar: Sprite3D, ratio: float) -> void:
	if ratio >= 1.0:
		bar.visible = false
		return
	bar.visible = true
	var step := clampi(int(ratio * STEPS), 0, STEPS)
	bar.texture = _cache[step]


static func _generate_bar_texture(ratio: float) -> ImageTexture:
	var img := Image.create(BAR_W, BAR_H, false, Image.FORMAT_RGBA8)
	var fill_w := int(ratio * BAR_W)
	var fill_color: Color
	if ratio > 0.5:
		fill_color = Color(0.2, 0.85, 0.2, 0.9)  # green
	elif ratio > 0.25:
		fill_color = Color(0.9, 0.8, 0.1, 0.9)  # yellow
	else:
		fill_color = Color(0.9, 0.15, 0.1, 0.9)  # red
	var bg_color := Color(0.15, 0.05, 0.05, 0.7)
	for y in BAR_H:
		for x in BAR_W:
			if x < fill_w:
				img.set_pixel(x, y, fill_color)
			else:
				img.set_pixel(x, y, bg_color)
	return ImageTexture.create_from_image(img)
