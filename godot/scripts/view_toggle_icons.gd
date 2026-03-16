## Procedural icon drawing for view toolbar toggle buttons.
##
## Provides two static draw functions that render small icons onto Controls
## using Godot's immediate-mode draw_* API. Each icon has two visual states:
##
## - **Roof icon (draw_roof_icon):** A simple house silhouette. When inactive
##   (roofs visible / normal), the roof triangle is solidly filled. When active
##   (roofs hidden), the roof is a dashed outline only, and the body interior
##   shows a lighter color to suggest the roof has been "removed."
##
## - **Height cutoff icon (draw_height_icon):** A tree with a horizontal
##   dashed line across the middle. When inactive (full height / normal), the
##   whole tree is solidly drawn. When active (upper voxels hidden), everything
##   above the dashed line becomes a dashed outline only, visually indicating
##   that the top portion is cut away.
##
## Both functions are called from _draw() overrides on the toolbar button
## Controls. The `center` and `size` parameters let the caller position and
## scale each icon; typical usage is 36 px.
##
## See also: geometry_utils.gd (another static utility class).
class_name ViewToggleIcons

# ---------------------------------------------------------------------------
# Shared palette
# ---------------------------------------------------------------------------

const COLOR_ROOF_FILL := Color(0.72, 0.42, 0.28)  # warm terracotta
const COLOR_ROOF_DASH := Color(0.72, 0.42, 0.28, 0.8)
const COLOR_WALL_FILL := Color(0.55, 0.50, 0.45)  # muted grey-brown
const COLOR_WALL_LIGHT := Color(0.70, 0.65, 0.58)  # lighter interior
const COLOR_OUTLINE := Color(0.25, 0.22, 0.20)

const COLOR_TRUNK := Color(0.45, 0.30, 0.18)
const COLOR_CANOPY_FILL := Color(0.28, 0.58, 0.25)
const COLOR_CANOPY_GHOST := Color(0.28, 0.58, 0.25, 0.35)
const COLOR_DASH_LINE := Color(0.85, 0.25, 0.20, 0.8)
const COLOR_DASH_LINE_SUBTLE := Color(0.60, 0.30, 0.25, 0.4)

## Length of each dash segment (fraction of total line length is computed from
## this absolute pixel value).
const DASH_LEN := 3.0
## Gap between dashes.
const DASH_GAP := 2.5

# ---------------------------------------------------------------------------
# Roof icon
# ---------------------------------------------------------------------------


static func draw_roof_icon(control: Control, center: Vector2, size: float, is_active: bool) -> void:
	var half := size * 0.5
	# The icon area spans from (center - half) to (center + half).

	# Body rectangle: lower 60% of the icon area.
	var body_top := center.y - half + size * 0.40
	var body_rect := Rect2(
		center.x - half * 0.70, body_top, size * 0.70, center.y + half - body_top
	)

	if is_active:
		# Roof hidden — show lighter interior through the "missing" roof.
		control.draw_rect(body_rect, COLOR_WALL_LIGHT, true)
	else:
		control.draw_rect(body_rect, COLOR_WALL_FILL, true)

	# Walls outline.
	control.draw_rect(body_rect, COLOR_OUTLINE, false, 1.0)

	# Roof triangle: sits above the body, peaks at the center top.
	var peak := Vector2(center.x, center.y - half + size * 0.05)
	var left := Vector2(center.x - half * 0.85, body_top)
	var right := Vector2(center.x + half * 0.85, body_top)

	if is_active:
		# Dashed outline only — roof is "removed."
		_draw_dashed_polyline(control, [peak, right, left, peak], COLOR_ROOF_DASH, 1.5)
	else:
		# Solid filled roof.
		control.draw_polygon(
			PackedVector2Array([peak, right, left]),
			PackedColorArray([COLOR_ROOF_FILL, COLOR_ROOF_FILL, COLOR_ROOF_FILL])
		)
		control.draw_polyline(PackedVector2Array([peak, right, left, peak]), COLOR_OUTLINE, 1.0)


# ---------------------------------------------------------------------------
# Height cutoff icon
# ---------------------------------------------------------------------------


static func draw_height_icon(
	control: Control, center: Vector2, size: float, is_active: bool
) -> void:
	var half := size * 0.5

	# Trunk: narrow rectangle in the bottom 40%.
	var trunk_width := size * 0.14
	var trunk_top := center.y - half + size * 0.55
	var trunk_rect := Rect2(
		center.x - trunk_width * 0.5, trunk_top, trunk_width, center.y + half - trunk_top
	)
	control.draw_rect(trunk_rect, COLOR_TRUNK, true)

	# Canopy: rough diamond / triangle occupying the top 60%.
	var canopy_bottom := trunk_top + size * 0.05  # slight overlap with trunk
	var canopy_top := center.y - half + size * 0.05
	var canopy_mid := (canopy_top + canopy_bottom) * 0.5
	var canopy_hw := size * 0.38  # half-width at widest

	var canopy_pts := PackedVector2Array(
		[
			Vector2(center.x, canopy_top),  # tip
			Vector2(center.x + canopy_hw, canopy_mid),  # right
			Vector2(center.x + canopy_hw * 0.7, canopy_bottom),  # bottom-right
			Vector2(center.x - canopy_hw * 0.7, canopy_bottom),  # bottom-left
			Vector2(center.x - canopy_hw, canopy_mid),  # left
		]
	)

	# Dashed horizontal line at the midpoint of the icon.
	var cut_y := center.y
	var line_left := center.x - half * 0.95
	var line_right := center.x + half * 0.95

	if is_active:
		# Below the cut line: solid canopy portion. We approximate by drawing
		# the full canopy clipped — since draw_polygon doesn't clip, we draw
		# the bottom half as a polygon manually.
		var bottom_pts := _clip_polygon_below(canopy_pts, cut_y)
		if bottom_pts.size() >= 3:
			var colors := PackedColorArray()
			for i in range(bottom_pts.size()):
				colors.append(COLOR_CANOPY_FILL)
			control.draw_polygon(bottom_pts, colors)

		# Above the cut line: dashed outline only.
		var top_outline := _clip_polyline_above(canopy_pts, cut_y)
		for segment in top_outline:
			_draw_dashed_polyline(control, segment, COLOR_CANOPY_GHOST, 1.0)

		# Prominent dashed line.
		_draw_dashed_line(
			control, Vector2(line_left, cut_y), Vector2(line_right, cut_y), COLOR_DASH_LINE, 1.5
		)
	else:
		# Full solid tree.
		var colors := PackedColorArray()
		for i in range(canopy_pts.size()):
			colors.append(COLOR_CANOPY_FILL)
		control.draw_polygon(canopy_pts, colors)
		control.draw_polyline(
			PackedVector2Array(
				[
					canopy_pts[0],
					canopy_pts[1],
					canopy_pts[2],
					canopy_pts[3],
					canopy_pts[4],
					canopy_pts[0]
				]
			),
			COLOR_OUTLINE,
			1.0
		)

		# Subtle dashed line.
		_draw_dashed_line(
			control,
			Vector2(line_left, cut_y),
			Vector2(line_right, cut_y),
			COLOR_DASH_LINE_SUBTLE,
			1.0
		)


# ---------------------------------------------------------------------------
# Dashed drawing helpers
# ---------------------------------------------------------------------------


## Draw a dashed/dotted line between two points.
static func _draw_dashed_line(
	control: Control, from: Vector2, to: Vector2, color: Color, width: float
) -> void:
	var dir := to - from
	var length := dir.length()
	if length < 0.1:
		return
	var unit := dir / length
	var pos := 0.0
	while pos < length:
		var seg_end := minf(pos + DASH_LEN, length)
		control.draw_line(from + unit * pos, from + unit * seg_end, color, width)
		pos = seg_end + DASH_GAP


## Draw a dashed polyline through an array of points.
static func _draw_dashed_polyline(
	control: Control, points: Array, color: Color, width: float
) -> void:
	for i in range(points.size() - 1):
		_draw_dashed_line(control, points[i], points[i + 1], color, width)


# ---------------------------------------------------------------------------
# Polygon clipping helpers (simple horizontal split)
# ---------------------------------------------------------------------------


## Return the portion of a convex polygon that lies at or below `y`.
## Vertices exactly on the line are included in the bottom half.
static func _clip_polygon_below(pts: PackedVector2Array, y: float) -> PackedVector2Array:
	var result := PackedVector2Array()
	var n := pts.size()
	for i in range(n):
		var cur := pts[i]
		var nxt := pts[(i + 1) % n]
		var cur_below := cur.y >= y
		var nxt_below := nxt.y >= y
		if cur_below:
			result.append(cur)
		# If the edge crosses the line, add the intersection.
		if cur_below != nxt_below:
			var t := (y - cur.y) / (nxt.y - cur.y)
			result.append(cur.lerp(nxt, t))
	return result


## Return polyline segments for the portion of a closed polygon outline that
## lies above `y`. Returns an Array of PackedVector2Array, each representing
## a contiguous segment above the line.
static func _clip_polyline_above(pts: PackedVector2Array, y: float) -> Array:
	var segments: Array = []
	var current := PackedVector2Array()
	var n := pts.size()
	for i in range(n):
		var cur := pts[i]
		var nxt := pts[(i + 1) % n]
		var cur_above := cur.y <= y
		var nxt_above := nxt.y <= y
		if cur_above:
			current.append(cur)
		if cur_above != nxt_above:
			var t := (y - cur.y) / (nxt.y - cur.y)
			var intersection := cur.lerp(nxt, t)
			if cur_above:
				# Leaving the top half — finish this segment.
				current.append(intersection)
				if current.size() >= 2:
					segments.append(current)
				current = PackedVector2Array()
			else:
				# Entering the top half — start a new segment.
				current.append(intersection)
	# Close any remaining segment.
	if current.size() >= 2:
		segments.append(current)
	return segments
