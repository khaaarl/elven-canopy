## Renders elves as billboard sprites.
##
## Creates one Sprite3D per elf with BILLBOARD_ENABLED. Uses a
## programmatically generated placeholder texture (simple silhouette).
## Positions are updated each frame from SimBridge.
##
## See also: sim_bridge.rs for elf position data, main.gd which creates
## this node and calls setup().

extends Node3D

var _bridge: SimBridge
var _elf_sprites: Array[Sprite3D] = []
var _elf_texture: ImageTexture


func _ready() -> void:
	_elf_texture = _create_elf_texture()


## Call after SimBridge is initialized.
func setup(bridge: SimBridge) -> void:
	_bridge = bridge


func _process(_delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	var positions := _bridge.get_elf_positions()
	var elf_count := positions.size()

	# Add sprites if we have more elves than sprites.
	while _elf_sprites.size() < elf_count:
		var sprite := Sprite3D.new()
		sprite.texture = _elf_texture
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = 0.08  # Scale: 32px * 0.08 = 2.56 world units wide
		sprite.transparent = true
		sprite.no_depth_test = false
		add_child(sprite)
		_elf_sprites.append(sprite)

	# Hide excess sprites.
	for i in _elf_sprites.size():
		if i < elf_count:
			_elf_sprites[i].visible = true
			var pos := positions[i]
			# Offset Y by ~1.5 so the sprite stands on top of the voxel.
			_elf_sprites[i].global_position = Vector3(pos.x + 0.5, pos.y + 1.5, pos.z + 0.5)
		else:
			_elf_sprites[i].visible = false


## Generate a simple 32x48 elf silhouette texture.
func _create_elf_texture() -> ImageTexture:
	var width := 32
	var height := 48
	var img := Image.create(width, height, false, Image.FORMAT_RGBA8)
	img.fill(Color(0.0, 0.0, 0.0, 0.0))  # Transparent background.

	var skin_color := Color(0.85, 0.70, 0.55, 1.0)
	var tunic_color := Color(0.15, 0.75, 0.25, 1.0)  # Bright green tunic
	var hair_color := Color(0.95, 0.85, 0.40, 1.0)   # Bright blonde hair
	var boot_color := Color(0.35, 0.22, 0.12, 1.0)   # Brown boots

	var cx := width / 2  # 16

	# Head (circle, radius 6, centered at y=8).
	for y in range(2, 15):
		for x in range(0, width):
			var dx := x - cx
			var dy := y - 8
			if dx * dx + dy * dy <= 36:  # r=6
				img.set_pixel(x, y, skin_color)

	# Hair (top of head).
	for y in range(1, 7):
		for x in range(0, width):
			var dx := x - cx
			var dy := y - 5
			if dx * dx + dy * dy <= 42 and y < 6:  # Slightly larger than head
				img.set_pixel(x, y, hair_color)

	# Pointed ears.
	for i in range(3):
		if cx - 8 - i >= 0:
			img.set_pixel(cx - 8 - i, 7 - i, skin_color)
		if cx + 7 + i < width:
			img.set_pixel(cx + 7 + i, 7 - i, skin_color)

	# Body / tunic (rectangle, from y=15 to y=34).
	for y in range(15, 35):
		var half_width := 7
		if y > 30:
			half_width = 7 + (y - 30)  # Flared bottom
		for x in range(cx - half_width, cx + half_width + 1):
			if x >= 0 and x < width:
				img.set_pixel(x, y, tunic_color)

	# Legs (two rectangles, from y=35 to y=42).
	for y in range(35, 43):
		for x in range(cx - 5, cx - 1):
			if x >= 0 and x < width:
				img.set_pixel(x, y, skin_color)
		for x in range(cx + 1, cx + 5):
			if x >= 0 and x < width:
				img.set_pixel(x, y, skin_color)

	# Boots (y=43 to y=47).
	for y in range(43, 48):
		for x in range(cx - 6, cx - 0):
			if x >= 0 and x < width:
				img.set_pixel(x, y, boot_color)
		for x in range(cx + 0, cx + 6):
			if x >= 0 and x < width:
				img.set_pixel(x, y, boot_color)

	return ImageTexture.create_from_image(img)
