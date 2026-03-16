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
