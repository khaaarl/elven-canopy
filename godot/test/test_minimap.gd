## Unit tests for minimap.gd pure logic.
##
## Tests zoom controls, follow mode toggle, and coordinate conversion
## without a SimBridge or live sim. The minimap is instantiated as a real
## node so its _ready() runs, then we exercise the logic paths.
##
## See also: minimap.gd for the implementation.
extends GutTest

const Minimap = preload("res://scripts/minimap.gd")

var _map: PanelContainer


func before_each() -> void:
	_map = Minimap.new()
	add_child_autofree(_map)


# -- Zoom controls -----------------------------------------------------------


func test_initial_zoom_index() -> void:
	assert_eq(_map._zoom_index, Minimap.DEFAULT_ZOOM_INDEX)


func test_zoom_in_decreases_index() -> void:
	_map._zoom_index = 3
	_map._zoom_in()
	assert_eq(_map._zoom_index, 2)


func test_zoom_in_clamps_at_zero() -> void:
	_map._zoom_index = 0
	_map._zoom_in()
	assert_eq(_map._zoom_index, 0)


func test_zoom_out_increases_index() -> void:
	_map._zoom_index = 1
	_map._zoom_out()
	assert_eq(_map._zoom_index, 2)


func test_zoom_out_clamps_at_max() -> void:
	_map._zoom_index = Minimap.ZOOM_LEVELS.size() - 1
	_map._zoom_out()
	assert_eq(_map._zoom_index, Minimap.ZOOM_LEVELS.size() - 1)


# -- Follow mode toggle -------------------------------------------------------


func test_initial_follow_mode_is_camera() -> void:
	assert_true(_map._follow_camera)


func test_toggle_follow_mode() -> void:
	_map._toggle_follow_mode()
	assert_false(_map._follow_camera)
	_map._toggle_follow_mode()
	assert_true(_map._follow_camera)


# -- Center calculation --------------------------------------------------------


func test_center_defaults_to_world_center_when_no_camera() -> void:
	# No bridge/camera set up, _follow_camera=true but _camera_pivot is null.
	_map._world_size = Vector3i(256, 128, 256)
	_map._follow_camera = false
	var center: Vector2 = _map._get_center()
	assert_eq(center, Vector2(128.0, 128.0))


func test_center_follows_camera_when_pivot_set() -> void:
	_map._world_size = Vector3i(256, 128, 256)
	_map._follow_camera = true
	# Create a mock pivot node at a known position.
	var pivot := Node3D.new()
	pivot.position = Vector3(50.0, 10.0, 80.0)
	add_child_autofree(pivot)
	_map._camera_pivot = pivot
	var center: Vector2 = _map._get_center()
	assert_almost_eq(center.x, 50.0, 0.01)
	assert_almost_eq(center.y, 80.0, 0.01)


func test_center_uses_world_center_when_tree_mode() -> void:
	_map._world_size = Vector3i(256, 128, 256)
	_map._follow_camera = false
	var pivot := Node3D.new()
	pivot.position = Vector3(50.0, 10.0, 80.0)
	add_child_autofree(pivot)
	_map._camera_pivot = pivot
	# Even with a camera pivot, tree mode centers on world center.
	var center: Vector2 = _map._get_center()
	assert_eq(center, Vector2(128.0, 128.0))


# -- Voxel coloring -------------------------------------------------------------


func test_color_for_voxel_empty_returns_empty_color() -> void:
	var color: Color = _map._color_for_voxel(0, 0, 128.0)
	assert_eq(color, Minimap.COLOR_EMPTY)


func test_color_for_voxel_dirt_returns_grass() -> void:
	var color: Color = _map._color_for_voxel(5, Minimap.VTYPE_DIRT, 128.0)
	assert_eq(color, Minimap.COLOR_GRASS)


func test_color_for_voxel_leaf_returns_leaf_lerp() -> void:
	var color: Color = _map._color_for_voxel(64, Minimap.VTYPE_LEAF, 128.0)
	# t = 64/128 = 0.5 -> midpoint between LEAF_LOW and LEAF_HIGH.
	var expected: Color = Minimap.COLOR_LEAF_LOW.lerp(Minimap.COLOR_LEAF_HIGH, 0.5)
	assert_almost_eq(color.r, expected.r, 0.01)
	assert_almost_eq(color.g, expected.g, 0.01)
	assert_almost_eq(color.b, expected.b, 0.01)


func test_color_for_voxel_trunk_returns_wood_lerp() -> void:
	var color: Color = _map._color_for_voxel(64, Minimap.VTYPE_TRUNK, 128.0)
	var expected: Color = Minimap.COLOR_WOOD_LOW.lerp(Minimap.COLOR_WOOD_HIGH, 0.5)
	assert_almost_eq(color.r, expected.r, 0.01)
	assert_almost_eq(color.g, expected.g, 0.01)
	assert_almost_eq(color.b, expected.b, 0.01)


func test_color_for_voxel_fruit_returns_fruit_color() -> void:
	var color: Color = _map._color_for_voxel(20, Minimap.VTYPE_FRUIT, 128.0)
	assert_eq(color, Minimap.COLOR_FRUIT)


func test_color_for_voxel_grown_returns_grown_color() -> void:
	var color: Color = _map._color_for_voxel(30, Minimap.VTYPE_GROWN_PLATFORM, 128.0)
	assert_eq(color, Minimap.COLOR_GROWN)


func test_color_for_voxel_unknown_type_uses_wood_fallback() -> void:
	# Unknown vtype 255 should use the wood lerp fallback.
	var color: Color = _map._color_for_voxel(64, 255, 128.0)
	var expected: Color = Minimap.COLOR_WOOD_LOW.lerp(Minimap.COLOR_WOOD_HIGH, 0.5)
	assert_almost_eq(color.r, expected.r, 0.01)
	assert_almost_eq(color.g, expected.g, 0.01)
	assert_almost_eq(color.b, expected.b, 0.01)
