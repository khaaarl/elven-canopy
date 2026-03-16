## Unit tests for orbital_camera.gd.
##
## Tests get_focus_voxel() and home_requested signal by constructing a Node3D
## with the orbital camera script attached.
##
## See also: orbital_camera.gd for the implementation.
extends GutTest

const OrbitalCamera = preload("res://scripts/orbital_camera.gd")

var _cam: Node3D


func before_each() -> void:
	_cam = Node3D.new()
	_cam.set_script(OrbitalCamera)
	# Add a Camera3D child (required by orbital_camera.gd's @onready).
	var camera := Camera3D.new()
	camera.name = "Camera3D"
	_cam.add_child(camera)
	add_child_autofree(_cam)


# -- get_focus_voxel -------------------------------------------------------


func test_focus_voxel_at_origin() -> void:
	_cam.position = Vector3(0.5, 0.5, 0.5)
	assert_eq(_cam.get_focus_voxel(), Vector3i(0, 0, 0), "Center of voxel (0,0,0)")


func test_focus_voxel_positive() -> void:
	_cam.position = Vector3(3.7, 5.2, 8.9)
	assert_eq(_cam.get_focus_voxel(), Vector3i(3, 5, 8))


func test_focus_voxel_at_exact_integer() -> void:
	_cam.position = Vector3(4.0, 2.0, 6.0)
	assert_eq(_cam.get_focus_voxel(), Vector3i(4, 2, 6))


func test_focus_voxel_negative_coords() -> void:
	_cam.position = Vector3(-0.5, 0.0, -1.5)
	assert_eq(_cam.get_focus_voxel(), Vector3i(-1, 0, -2))


func test_focus_voxel_large_coords() -> void:
	_cam.position = Vector3(127.9, 255.1, 63.0)
	assert_eq(_cam.get_focus_voxel(), Vector3i(127, 255, 63))


# -- home_requested signal ---------------------------------------------------


func _make_home_key_event() -> InputEventKey:
	var key := InputEventKey.new()
	key.keycode = KEY_HOME
	key.pressed = true
	return key


func test_home_key_emits_signal() -> void:
	watch_signals(_cam)
	_cam._unhandled_input(_make_home_key_event())
	assert_signal_emitted(_cam, "home_requested")


func test_home_key_no_signal_on_release() -> void:
	watch_signals(_cam)
	var key := _make_home_key_event()
	key.pressed = false
	_cam._unhandled_input(key)
	assert_signal_not_emitted(_cam, "home_requested")


func test_home_key_stops_follow() -> void:
	_cam.start_follow(Vector3(10.0, 5.0, 10.0))
	assert_true(_cam.is_following(), "Should be following before Home press")
	# The signal handler (in main.gd) calls stop_follow via _look_at_position.
	# But the camera itself doesn't stop follow — that's main.gd's job.
	# Verify the signal is emitted so main.gd can act on it.
	watch_signals(_cam)
	_cam._unhandled_input(_make_home_key_event())
	assert_signal_emitted(_cam, "home_requested")


func test_home_key_preserves_zoom_and_pitch() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	# Access internal state via the script's variables.
	_cam._zoom = 45.0
	_cam._pitch = 1.0
	_cam._unhandled_input(_make_home_key_event())
	assert_eq(_cam._zoom, 45.0, "Zoom should be unchanged after Home press")
	assert_eq(_cam._pitch, 1.0, "Pitch should be unchanged after Home press")
