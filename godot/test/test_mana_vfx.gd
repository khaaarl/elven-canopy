## Unit tests for mana_vfx.gd swirl texture generation.
##
## Verifies the procedural blue spiral glyph texture has the expected
## dimensions and contains non-transparent pixels (the spiral pattern).
extends GutTest

const ManaVfx = preload("res://scripts/mana_vfx.gd")


func test_swirl_texture_dimensions() -> void:
	var tex: ImageTexture = ManaVfx._generate_swirl_texture()
	assert_not_null(tex, "texture should not be null")
	var img := tex.get_image()
	assert_eq(img.get_width(), 12, "texture width should be 12")
	assert_eq(img.get_height(), 12, "texture height should be 12")


func test_swirl_texture_has_visible_pixels() -> void:
	var tex: ImageTexture = ManaVfx._generate_swirl_texture()
	var img := tex.get_image()
	var has_color := false
	for y in img.get_height():
		for x in img.get_width():
			var c := img.get_pixel(x, y)
			if c.a > 0.05:
				has_color = true
				break
		if has_color:
			break
	assert_true(has_color, "swirl texture should have some visible (non-transparent) pixels")


func test_swirl_texture_uses_blue_tones() -> void:
	var tex: ImageTexture = ManaVfx._generate_swirl_texture()
	var img := tex.get_image()
	# Find a visible pixel and verify it's blue-ish (b > r and b > g).
	for y in img.get_height():
		for x in img.get_width():
			var c := img.get_pixel(x, y)
			if c.a > 0.1:
				assert_true(
					c.b > c.r and c.b > c.g,
					"visible pixels should be blue-toned: got r=%f g=%f b=%f" % [c.r, c.g, c.b]
				)
				return
	assert_true(false, "no visible pixel found to check color")
