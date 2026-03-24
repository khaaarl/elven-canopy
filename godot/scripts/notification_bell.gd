## Bell icon button for the notification history panel.
##
## Displays a procedurally drawn bell icon in the bottom-right corner of the
## screen, just above the notification toast area. When unread notifications
## exist, a red circle badge with the count is drawn over the bell.
##
## Clicking the bell emits `bell_pressed` so that main.gd can toggle the
## notification history panel. The unread count is updated by main.gd via
## `set_unread_count()`.
##
## The bell shape is drawn using Godot's immediate-mode `_draw()` API,
## following the same pattern as view_toggle_icons.gd.
##
## See also: notification_display.gd (toast display),
## notification_history_panel.gd (scrollable log), main.gd (wiring).

extends Button

signal bell_pressed

# -- Bell colors --
const COLOR_BELL_BODY := Color(0.82, 0.72, 0.38)  # warm gold
const COLOR_BELL_DARK := Color(0.62, 0.52, 0.22)  # darker gold for shading
const COLOR_BELL_HIGHLIGHT := Color(0.92, 0.85, 0.55)  # light highlight
const COLOR_BELL_OUTLINE := Color(0.35, 0.28, 0.12)  # dark outline
const COLOR_BELL_CLAPPER := Color(0.45, 0.35, 0.15)  # clapper ball
const COLOR_BADGE_BG := Color(0.85, 0.15, 0.15)  # red badge
const COLOR_BADGE_TEXT := Color(1.0, 1.0, 1.0)  # white text

## Button size in pixels.
const BUTTON_SIZE := 36

## Number of unread notifications, drives the badge display.
var _unread_count: int = 0


func _ready() -> void:
	custom_minimum_size = Vector2(BUTTON_SIZE, BUTTON_SIZE)
	size = Vector2(BUTTON_SIZE, BUTTON_SIZE)
	# Transparent flat button — the bell is drawn via _draw().
	flat = true
	tooltip_text = "Notification history"
	pressed.connect(func(): bell_pressed.emit())


func set_unread_count(count: int) -> void:
	if _unread_count != count:
		_unread_count = count
		queue_redraw()


func get_unread_count() -> int:
	return _unread_count


func _draw() -> void:
	var center := Vector2(BUTTON_SIZE * 0.5, BUTTON_SIZE * 0.5)
	_draw_bell(center, BUTTON_SIZE * 0.8)
	if _unread_count > 0:
		_draw_badge(center)


func _draw_bell(center: Vector2, size: float) -> void:
	var half := size * 0.5

	# Bell body: a rounded trapezoid shape built from an ellipse top and
	# widening sides down to a flat brim.
	var top_y := center.y - half + size * 0.1
	var brim_y := center.y + half * 0.55
	var crown_y := top_y - size * 0.06  # small nub at top

	# Crown nub (small circle at the top).
	draw_circle(Vector2(center.x, crown_y), size * 0.06, COLOR_BELL_OUTLINE)
	draw_circle(Vector2(center.x, crown_y), size * 0.04, COLOR_BELL_HIGHLIGHT)

	# Bell dome (upper ellipse portion).
	var dome_cx := center.x
	var dome_cy := top_y + size * 0.12
	var dome_rx := size * 0.22
	var dome_ry := size * 0.14
	_draw_filled_ellipse(Vector2(dome_cx, dome_cy), dome_rx, dome_ry, COLOR_BELL_BODY)

	# Bell body: wider trapezoid from dome bottom to brim.
	var body_top_y := dome_cy + dome_ry * 0.5
	var body_top_hw := dome_rx  # half-width at top of body
	var body_bot_hw := size * 0.38  # half-width at brim

	var body_pts := PackedVector2Array(
		[
			Vector2(center.x - body_top_hw, body_top_y),
			Vector2(center.x + body_top_hw, body_top_y),
			Vector2(center.x + body_bot_hw, brim_y),
			Vector2(center.x - body_bot_hw, brim_y),
		]
	)
	var body_colors := PackedColorArray(
		[
			COLOR_BELL_BODY,
			COLOR_BELL_BODY,
			COLOR_BELL_BODY,
			COLOR_BELL_BODY,
		]
	)
	draw_polygon(body_pts, body_colors)

	# Highlight stripe on the left side of the body for a 3D effect.
	var hl_pts := PackedVector2Array(
		[
			Vector2(center.x - body_top_hw + size * 0.04, body_top_y + size * 0.02),
			Vector2(center.x - body_top_hw * 0.4, body_top_y + size * 0.02),
			Vector2(center.x - body_bot_hw * 0.4, brim_y - size * 0.02),
			Vector2(center.x - body_bot_hw + size * 0.04, brim_y - size * 0.02),
		]
	)
	var hl_colors := PackedColorArray(
		[
			COLOR_BELL_HIGHLIGHT,
			COLOR_BELL_HIGHLIGHT,
			COLOR_BELL_HIGHLIGHT,
			COLOR_BELL_HIGHLIGHT,
		]
	)
	draw_polygon(hl_pts, hl_colors)

	# Shadow stripe on the right side.
	var sh_pts := PackedVector2Array(
		[
			Vector2(center.x + body_top_hw * 0.5, body_top_y + size * 0.02),
			Vector2(center.x + body_top_hw - size * 0.02, body_top_y + size * 0.02),
			Vector2(center.x + body_bot_hw - size * 0.02, brim_y - size * 0.02),
			Vector2(center.x + body_bot_hw * 0.5, brim_y - size * 0.02),
		]
	)
	var sh_colors := PackedColorArray(
		[
			COLOR_BELL_DARK,
			COLOR_BELL_DARK,
			COLOR_BELL_DARK,
			COLOR_BELL_DARK,
		]
	)
	draw_polygon(sh_pts, sh_colors)

	# Brim line (flat bottom of the bell).
	var brim_extend := size * 0.06
	draw_line(
		Vector2(center.x - body_bot_hw - brim_extend, brim_y),
		Vector2(center.x + body_bot_hw + brim_extend, brim_y),
		COLOR_BELL_OUTLINE,
		2.0
	)

	# Outline around the bell body.
	var outline_pts := PackedVector2Array(
		[
			Vector2(center.x - body_top_hw, body_top_y),
			Vector2(center.x - body_bot_hw, brim_y),
			Vector2(center.x + body_bot_hw, brim_y),
			Vector2(center.x + body_top_hw, body_top_y),
			Vector2(center.x - body_top_hw, body_top_y),
		]
	)
	draw_polyline(outline_pts, COLOR_BELL_OUTLINE, 1.0)

	# Clapper (small circle hanging below the brim).
	var clapper_y := brim_y + size * 0.08
	draw_circle(Vector2(center.x, clapper_y), size * 0.055, COLOR_BELL_CLAPPER)


func _draw_badge(center: Vector2) -> void:
	# Red circle badge in the top-right quadrant of the button.
	var badge_center := Vector2(center.x + BUTTON_SIZE * 0.25, center.y - BUTTON_SIZE * 0.22)
	var badge_radius := 7.0 if _unread_count < 10 else 8.0
	draw_circle(badge_center, badge_radius, COLOR_BADGE_BG)
	# Outline for legibility.
	draw_arc(badge_center, badge_radius, 0.0, TAU, 24, Color(0.0, 0.0, 0.0, 0.5), 1.0)

	# Count text — use draw_string for the number.
	var count_str := str(_unread_count) if _unread_count < 100 else "99+"
	var font := get_theme_font("font", "Label")
	var font_size := 10 if _unread_count < 10 else 9
	var text_size := font.get_string_size(count_str, HORIZONTAL_ALIGNMENT_CENTER, -1, font_size)
	var text_pos := Vector2(badge_center.x - text_size.x * 0.5, badge_center.y + text_size.y * 0.3)
	draw_string(
		font, text_pos, count_str, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, COLOR_BADGE_TEXT
	)


func _draw_filled_ellipse(center_pos: Vector2, rx: float, ry: float, color: Color) -> void:
	# Draw a filled ellipse using a polygon approximation.
	var segments := 16
	var points := PackedVector2Array()
	var colors := PackedColorArray()
	for i in range(segments):
		var angle := TAU * float(i) / float(segments)
		var px := center_pos.x + cos(angle) * rx
		var py := center_pos.y + sin(angle) * ry
		points.append(Vector2(px, py))
		colors.append(color)
	draw_polygon(points, colors)
