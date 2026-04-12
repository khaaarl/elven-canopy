## Unit tests for GeometryUtils (geometry_utils.gd).
##
## Covers the pure math helpers shared by selection_controller.gd,
## tooltip_controller.gd, and placement_controller.gd: point-to-ray
## distance and screen rectangle construction from drag corners.
## Also covers the canonical DIRECTION_OFFSETS array shared by
## construction_controller.gd, blueprint_renderer.gd,
## building_renderer.gd, and ladder_renderer.gd.
##
## See also: geometry_utils.gd for the implementation.
extends GutTest

# -- point_to_ray_dist_sq -------------------------------------------------


func test_point_on_ray_returns_zero() -> void:
	# A point lying exactly on the ray should have distance 0.
	var origin := Vector3(0.0, 0.0, 0.0)
	var dir := Vector3(0.0, 0.0, -1.0)
	var point := Vector3(0.0, 0.0, -5.0)
	var dist_sq := GeometryUtils.point_to_ray_dist_sq(point, origin, dir)
	assert_almost_eq(dist_sq, 0.0, 0.0001, "Point on ray should have zero distance")


func test_point_perpendicular_to_ray() -> void:
	# Point 3 units to the right of a ray along +Z.
	var origin := Vector3(0.0, 0.0, 0.0)
	var dir := Vector3(0.0, 0.0, 1.0)
	var point := Vector3(3.0, 0.0, 5.0)
	var dist_sq := GeometryUtils.point_to_ray_dist_sq(point, origin, dir)
	assert_almost_eq(dist_sq, 9.0, 0.0001, "Should be 3^2 = 9")


func test_point_behind_ray_clamps_to_origin() -> void:
	# Point is behind the ray origin — closest point should be the origin.
	var origin := Vector3(0.0, 0.0, 0.0)
	var dir := Vector3(0.0, 0.0, 1.0)
	var point := Vector3(0.0, 4.0, -10.0)
	var dist_sq := GeometryUtils.point_to_ray_dist_sq(point, origin, dir)
	# Distance from (0,4,-10) to origin (0,0,0) = sqrt(16+100) = sqrt(116)
	var expected := point.length_squared()
	assert_almost_eq(dist_sq, expected, 0.0001, "Behind-ray point clamps to origin")


func test_point_at_ray_origin() -> void:
	var origin := Vector3(5.0, 3.0, 1.0)
	var dir := Vector3(1.0, 0.0, 0.0)
	var dist_sq := GeometryUtils.point_to_ray_dist_sq(origin, origin, dir)
	assert_almost_eq(dist_sq, 0.0, 0.0001, "Point at origin should have zero distance")


func test_diagonal_ray() -> void:
	# Ray along (1,1,0) normalized. Point (0,2,0) — project onto ray.
	var origin := Vector3.ZERO
	var dir := Vector3(1.0, 1.0, 0.0).normalized()
	var point := Vector3(0.0, 2.0, 0.0)
	# Projection: t = dot((0,2,0), dir) = 2 * 0.7071 = 1.4142
	# Closest = dir * t = (1,1,0) * 1.4142 / sqrt(2) = (1,1,0) already normalized
	# Closest = (1,1,0).normalized() * 1.4142 = (0.7071, 0.7071, 0) * 1.4142 = (1, 1, 0)
	# Diff = (0,2,0) - (1,1,0) = (-1,1,0) → dist_sq = 2.0
	var dist_sq := GeometryUtils.point_to_ray_dist_sq(point, origin, dir)
	assert_almost_eq(dist_sq, 2.0, 0.0001, "Diagonal ray distance check")


# -- make_screen_rect ------------------------------------------------------


func test_make_screen_rect_top_left_to_bottom_right() -> void:
	var rect := GeometryUtils.make_screen_rect(Vector2(10, 20), Vector2(100, 200))
	assert_eq(rect.position, Vector2(10, 20))
	assert_eq(rect.size, Vector2(90, 180))


func test_make_screen_rect_bottom_right_to_top_left() -> void:
	# Dragging from bottom-right to top-left should produce the same rect.
	var rect := GeometryUtils.make_screen_rect(Vector2(100, 200), Vector2(10, 20))
	assert_eq(rect.position, Vector2(10, 20))
	assert_eq(rect.size, Vector2(90, 180))


func test_make_screen_rect_zero_size() -> void:
	var rect := GeometryUtils.make_screen_rect(Vector2(50, 50), Vector2(50, 50))
	assert_eq(rect.position, Vector2(50, 50))
	assert_eq(rect.size, Vector2.ZERO)


func test_make_screen_rect_mixed_corners() -> void:
	# a.x < b.x but a.y > b.y — tests that each axis is handled independently.
	var rect := GeometryUtils.make_screen_rect(Vector2(10, 200), Vector2(100, 20))
	assert_eq(rect.position, Vector2(10, 20))
	assert_eq(rect.size, Vector2(90, 180))


# -- DIRECTION_OFFSETS -------------------------------------------------------


func test_direction_offsets_has_six_entries() -> void:
	assert_eq(GeometryUtils.DIRECTION_OFFSETS.size(), 6, "Should have exactly 6 face directions")


func test_direction_offsets_index_contract() -> void:
	# The sim's FaceDirection enum assigns PosX=0, NegX=1, PosY=2, NegY=3,
	# PosZ=4, NegZ=5.  All four consumer files index into this array with
	# those ordinals, so the mapping must be exact.
	var o := GeometryUtils.DIRECTION_OFFSETS
	assert_eq(o[0], Vector3.RIGHT, "Index 0 = PosX = RIGHT")
	assert_eq(o[1], Vector3.LEFT, "Index 1 = NegX = LEFT")
	assert_eq(o[2], Vector3.UP, "Index 2 = PosY = UP")
	assert_eq(o[3], Vector3.DOWN, "Index 3 = NegY = DOWN")
	assert_eq(o[4], Vector3.BACK, "Index 4 = PosZ = BACK")
	assert_eq(o[5], Vector3.FORWARD, "Index 5 = NegZ = FORWARD")
