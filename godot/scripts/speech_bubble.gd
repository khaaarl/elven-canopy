## Individual speech bubble — a Node3D with a Label3D for text and a Sprite3D
## for the rounded-rect background panel with a tail pointer.
##
## Positioned above a creature in world space. Billboard mode makes both the
## text and background always face the camera. Text shrinks naturally with
## distance (Label3D world-space behavior — "harder to hear from far away").
##
## Lifecycle: show_speech() makes the bubble visible with text, then after
## DISPLAY_DURATION_SEC it fades out over FADE_DURATION_SEC via a Tween on
## modulate alpha. After fade, the bubble is hidden and available for reuse
## by speech_bubble_manager.gd's pool.
##
## See also: speech_bubble_manager.gd (owner/pool manager),
## creature_renderer.gd (creature positions and Y offsets),
## hp_bar.gd (similar billboard Sprite3D pattern for overhead bars).

extends Node3D

## How long the bubble stays fully visible before fading.
const DISPLAY_DURATION_SEC := 3.0
## How long the fade-out takes.
const FADE_DURATION_SEC := 0.5
## World-space size of each pixel for both Label3D and background Sprite3D.
const PIXEL_SIZE := 0.008
## Font size for the label text (in font pixels).
const FONT_SIZE := 16
## Horizontal padding (pixels) on each side of the text inside the bubble.
const PAD_X := 6
## Vertical padding (pixels) above and below the text inside the bubble.
const PAD_Y := 4
## Tail height in pixels (triangle pointing down from the panel).
const TAIL_HEIGHT := 5
## Corner radius in pixels for the rounded rect.
const CORNER_RADIUS := 3

## Cached background textures keyed by (width, height) string.
## Shared across all bubble instances.
static var _bg_cache: Dictionary = {}

## Label3D for the speech text.
var _label: Label3D
## Sprite3D for the background panel (behind the label).
var _bg: Sprite3D
## Tween managing the fade-out. Null when not fading.
var _fade_tween: Tween = null
## Timer tracking how long the bubble has been displayed.
var _display_timer: float = 0.0
## Whether the bubble is currently showing (visible and counting down).
var _active: bool = false


func _ready() -> void:
	_label = Label3D.new()
	_label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	_label.no_depth_test = true
	_label.render_priority = 3
	_label.pixel_size = PIXEL_SIZE
	_label.font_size = FONT_SIZE
	_label.modulate = Color(0.1, 0.05, 0.0, 1.0)
	_label.outline_modulate = Color(0.1, 0.05, 0.0, 0.0)
	_label.outline_size = 2
	_label.position = Vector3(0.0, 0.0, 0.0)
	add_child(_label)

	_bg = Sprite3D.new()
	_bg.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	_bg.no_depth_test = true
	_bg.render_priority = 2
	_bg.pixel_size = PIXEL_SIZE
	_bg.transparent = true
	# Offset background behind the text so label renders in front.
	_bg.position = Vector3(0.0, 0.0, 0.01)
	add_child(_bg)

	visible = false


func _process(delta: float) -> void:
	if not _active:
		return
	_display_timer += delta
	if _display_timer >= DISPLAY_DURATION_SEC and _fade_tween == null:
		_start_fade()


## Show the bubble with the given text. Resets any in-progress fade.
func show_speech(text: String) -> void:
	if _fade_tween != null:
		_fade_tween.kill()
		_fade_tween = null

	_label.text = text
	_display_timer = 0.0
	_active = true
	visible = true
	_label.modulate = Color(0.1, 0.05, 0.0, 1.0)
	_bg.modulate = Color(1.0, 1.0, 1.0, 1.0)

	# Measure text size using the label's font and generate a matching background.
	var font: Font = _label.font if _label.font else ThemeDB.fallback_font
	var text_size: Vector2 = font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE)
	var bg_w: int = int(text_size.x) + PAD_X * 2
	var bg_h: int = int(text_size.y) + PAD_Y * 2 + TAIL_HEIGHT
	_bg.texture = _get_bg_texture(bg_w, bg_h)
	# Center the background behind the text. The label is centered at origin;
	# the background sprite's center is at its midpoint. Shift down by half
	# the tail height so the panel portion aligns with the text.
	_bg.position.y = -float(TAIL_HEIGHT) * PIXEL_SIZE * 0.5


## Returns true if the bubble has finished fading and is ready for reuse.
func is_expired() -> bool:
	return not _active


func _start_fade() -> void:
	_fade_tween = create_tween()
	_fade_tween.set_parallel(true)
	_fade_tween.tween_property(_label, "modulate:a", 0.0, FADE_DURATION_SEC)
	_fade_tween.tween_property(_bg, "modulate:a", 0.0, FADE_DURATION_SEC)
	_fade_tween.chain().tween_callback(_on_fade_complete)


func _on_fade_complete() -> void:
	_active = false
	visible = false
	_fade_tween = null


## Get or generate a cached background texture for the given pixel dimensions.
static func _get_bg_texture(w: int, h: int) -> ImageTexture:
	var key := "%d_%d" % [w, h]
	if _bg_cache.has(key):
		return _bg_cache[key]
	var tex := _generate_bg_texture(w, h)
	_bg_cache[key] = tex
	return tex


## Generate a rounded-rect speech bubble texture with a tail pointer.
static func _generate_bg_texture(w: int, h: int) -> ImageTexture:
	var img := Image.create(w, h, false, Image.FORMAT_RGBA8)
	var bg_color := Color(0.97, 0.95, 0.88, 0.92)
	var border_color := Color(0.3, 0.25, 0.2, 0.85)
	var transparent := Color(0.0, 0.0, 0.0, 0.0)
	var panel_h := h - TAIL_HEIGHT
	var r := CORNER_RADIUS

	# Fill transparent first.
	img.fill(transparent)

	# Draw rounded rectangle body.
	for y in panel_h:
		for x in w:
			if _in_rounded_rect(x, y, w, panel_h, r):
				# Border: 1px edge.
				if _on_rounded_rect_border(x, y, w, panel_h, r):
					img.set_pixel(x, y, border_color)
				else:
					img.set_pixel(x, y, bg_color)

	# Draw tail (small triangle centered at bottom of panel).
	var tail_half := 3
	var cx := w / 2
	for ty in TAIL_HEIGHT:
		var row_half: int = tail_half - ty
		if row_half < 0:
			continue
		for tx in range(cx - row_half, cx + row_half + 1):
			if tx >= 0 and tx < w:
				var py := panel_h + ty
				if ty == 0 or tx == cx - row_half or tx == cx + row_half:
					img.set_pixel(tx, py, border_color)
				else:
					img.set_pixel(tx, py, bg_color)

	return ImageTexture.create_from_image(img)


## Check if a pixel is inside the rounded rectangle.
static func _in_rounded_rect(x: int, y: int, w: int, h: int, r: int) -> bool:
	# Main body (excluding corners).
	if x >= r and x < w - r:
		return true
	if y >= r and y < h - r:
		return true
	# Check corner circles.
	var corners := [
		Vector2i(r, r),
		Vector2i(w - 1 - r, r),
		Vector2i(r, h - 1 - r),
		Vector2i(w - 1 - r, h - 1 - r),
	]
	for c in corners:
		var dx: int = x - c.x
		var dy: int = y - c.y
		if dx * dx + dy * dy <= r * r:
			return true
	return false


## Check if a pixel is on the 1px border of the rounded rectangle.
static func _on_rounded_rect_border(x: int, y: int, w: int, h: int, r: int) -> bool:
	if x == 0 or x == w - 1 or y == 0 or y == h - 1:
		# On the straight edges (but only if inside the rounded rect).
		return _in_rounded_rect(x, y, w, h, r)
	# Check if just inside the corner radius boundary.
	if (x < r or x >= w - r) and (y < r or y >= h - r):
		var corners := [
			Vector2i(r, r),
			Vector2i(w - 1 - r, r),
			Vector2i(r, h - 1 - r),
			Vector2i(w - 1 - r, h - 1 - r),
		]
		for c in corners:
			var dx: int = x - c.x
			var dy: int = y - c.y
			var dist_sq: int = dx * dx + dy * dy
			if dist_sq <= r * r and dist_sq > (r - 1) * (r - 1):
				return true
	return false
