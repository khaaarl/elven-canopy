## Pure geometry helper functions shared across controllers.
##
## Extracted from selection_controller.gd and tooltip_controller.gd so they
## can be unit-tested independently without Godot scene dependencies (SimBridge,
## Camera3D, etc.). Both controllers now delegate to these functions.
## placement_controller.gd uses the same math but inlines it in a hot loop.
##
## See also: selection_controller.gd (click-to-select, box-select),
## tooltip_controller.gd (hover detection), placement_controller.gd
## (click-to-place snap — inlines the same ray-distance math for performance).
class_name GeometryUtils


## Perpendicular distance squared from a point to a ray (origin + t * dir).
## Clamps t >= 0 so points behind the ray origin are handled correctly — the
## closest point on the ray is the origin itself when the projection is negative.
static func point_to_ray_dist_sq(point: Vector3, ray_origin: Vector3, ray_dir: Vector3) -> float:
	var to_point := point - ray_origin
	var t := maxf(0.0, to_point.dot(ray_dir))
	var closest := ray_origin + ray_dir * t
	return (point - closest).length_squared()


## Build a Rect2 from two corner points, handling any drag direction.
## The result always has positive size regardless of whether a > b or b > a
## on either axis.
static func make_screen_rect(a: Vector2, b: Vector2) -> Rect2:
	var top_left := Vector2(minf(a.x, b.x), minf(a.y, b.y))
	var bottom_right := Vector2(maxf(a.x, b.x), maxf(a.y, b.y))
	return Rect2(top_left, bottom_right - top_left)
