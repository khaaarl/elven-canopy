## Unit tests for ViewToggleIcons polygon clipping helpers.
##
## Covers _clip_polygon_below() and _clip_polyline_above(), the
## Sutherland-Hodgman-style horizontal clipping used by the height cutoff
## icon to split the canopy polygon into a solid bottom half and dashed
## top outline segments.
##
## See also: view_toggle_icons.gd for the implementation.
extends GutTest

# -- Helpers ----------------------------------------------------------------

const EPSILON := 0.0001


## Assert two PackedVector2Arrays are element-wise almost equal.
func _assert_pv2_almost_eq(
	actual: PackedVector2Array, expected: PackedVector2Array, msg: String
) -> void:
	assert_eq(actual.size(), expected.size(), msg + " (size mismatch)")
	for i in range(mini(actual.size(), expected.size())):
		assert_almost_eq(actual[i].x, expected[i].x, EPSILON, msg + " x[%d]" % i)
		assert_almost_eq(actual[i].y, expected[i].y, EPSILON, msg + " y[%d]" % i)


# -- _clip_polygon_below ---------------------------------------------------


func test_clip_polygon_below_all_below() -> void:
	# Square polygon entirely below the cut line (y=0). All points have y >= 0.
	var pts := PackedVector2Array(
		[Vector2(0.0, 2.0), Vector2(4.0, 2.0), Vector2(4.0, 6.0), Vector2(0.0, 6.0)]
	)
	var result := ViewToggleIcons._clip_polygon_below(pts, 0.0)
	_assert_pv2_almost_eq(result, pts, "All-below polygon should be returned unchanged")


func test_clip_polygon_below_all_above() -> void:
	# Square polygon entirely above the cut line (y=10). All points have y < 10.
	var pts := PackedVector2Array(
		[Vector2(0.0, 2.0), Vector2(4.0, 2.0), Vector2(4.0, 6.0), Vector2(0.0, 6.0)]
	)
	var result := ViewToggleIcons._clip_polygon_below(pts, 10.0)
	assert_eq(result.size(), 0, "All-above polygon should return empty")


func test_clip_polygon_below_split() -> void:
	# Unit square: (0,0), (4,0), (4,4), (0,4). Cut at y=2.
	# "Below" means y >= 2, so the bottom portion is the strip from y=2 to y=4.
	# Walking the edges:
	#   (0,0)->(4,0): both above (y<2), no output
	#   (4,0)->(4,4): crosses at (4,2), then (4,4) is below
	#   (4,4)->(0,4): both below
	#   (0,4)->(0,0): (0,4) below, crosses at (0,2)
	# Expected: (4,2), (4,4), (0,4), (0,2)
	var pts := PackedVector2Array(
		[Vector2(0.0, 0.0), Vector2(4.0, 0.0), Vector2(4.0, 4.0), Vector2(0.0, 4.0)]
	)
	var result := ViewToggleIcons._clip_polygon_below(pts, 2.0)
	var expected := PackedVector2Array(
		[Vector2(4.0, 2.0), Vector2(4.0, 4.0), Vector2(0.0, 4.0), Vector2(0.0, 2.0)]
	)
	_assert_pv2_almost_eq(result, expected, "Split polygon bottom half")


# -- _clip_polyline_above ---------------------------------------------------


func test_clip_polyline_above_all_above() -> void:
	# Triangle entirely above the cut line (y=10). All points have y <= 10.
	var pts := PackedVector2Array([Vector2(0.0, 0.0), Vector2(4.0, 0.0), Vector2(2.0, 4.0)])
	var result := ViewToggleIcons._clip_polyline_above(pts, 10.0)
	# The closed polygon outline walks 3 edges; all vertices are above,
	# so we get one contiguous segment with all 3 vertices plus the first
	# vertex repeated as the edge back to start never leaves the top half.
	# Actually: the function walks edges and only emits segments of length >= 2.
	# Every vertex is above, so current accumulates all 3 vertices with no
	# crossing. The segment is flushed at the end with size 3 (>= 2).
	assert_eq(result.size(), 1, "Should produce one segment")
	var seg: PackedVector2Array = result[0]
	_assert_pv2_almost_eq(seg, pts, "Segment should contain all original points")


func test_clip_polyline_above_all_below() -> void:
	# Square polygon entirely below the cut line (y=0). All points have y > 0.
	var pts := PackedVector2Array(
		[Vector2(0.0, 2.0), Vector2(4.0, 2.0), Vector2(4.0, 6.0), Vector2(0.0, 6.0)]
	)
	var result := ViewToggleIcons._clip_polyline_above(pts, 0.0)
	assert_eq(result.size(), 0, "All-below polygon should return no segments")


func test_clip_polyline_above_split() -> void:
	# Unit square: (0,0), (4,0), (4,4), (0,4). Cut at y=2.
	# "Above" means y <= 2.
	# Walking edges of the closed polygon:
	#   i=0: cur=(0,0) above, nxt=(4,0) above → append (0,0)
	#   i=1: cur=(4,0) above, nxt=(4,4) NOT above → append (4,0), cross at (4,2),
	#         append (4,2), flush segment [(0,0),(4,0),(4,2)]
	#   i=2: cur=(4,4) NOT above, nxt=(0,4) NOT above → skip
	#   i=3: cur=(0,4) NOT above, nxt=(0,0) above → cross at (0,2),
	#         start new segment with (0,2)
	#   End: current = [(0,2)] size 1, not flushed (< 2).
	#
	# So we get one segment: [(0,0), (4,0), (4,2)]
	var pts := PackedVector2Array(
		[Vector2(0.0, 0.0), Vector2(4.0, 0.0), Vector2(4.0, 4.0), Vector2(0.0, 4.0)]
	)
	var result := ViewToggleIcons._clip_polyline_above(pts, 2.0)
	assert_eq(result.size(), 1, "Should produce one segment")
	var expected := PackedVector2Array([Vector2(0.0, 0.0), Vector2(4.0, 0.0), Vector2(4.0, 2.0)])
	var seg: PackedVector2Array = result[0]
	_assert_pv2_almost_eq(seg, expected, "Above segment of split square")
