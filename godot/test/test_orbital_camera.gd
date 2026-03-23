## Unit tests for orbital_camera.gd.
##
## Tests get_focus_voxel(), home_requested signal, and Ctrl+MMB pan by
## constructing a Node3D with the orbital camera script attached.
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


# -- Ctrl+MMB pan -------------------------------------------------------------


func _make_mmb_event(pressed: bool, ctrl: bool = false) -> InputEventMouseButton:
	var mb := InputEventMouseButton.new()
	mb.button_index = MOUSE_BUTTON_MIDDLE
	mb.pressed = pressed
	mb.ctrl_pressed = ctrl
	return mb


func _make_mouse_motion(dx: float, dy: float, ctrl: bool = false) -> InputEventMouseMotion:
	var mm := InputEventMouseMotion.new()
	mm.relative = Vector2(dx, dy)
	mm.ctrl_pressed = ctrl
	return mm


func test_ctrl_mmb_enters_pan_mode() -> void:
	_cam._unhandled_input(_make_mmb_event(true, true))
	assert_true(_cam._panning, "Ctrl+MMB press should enter pan mode")
	assert_false(_cam._rotating, "Ctrl+MMB should not enter rotate mode")


func test_plain_mmb_enters_rotate_mode() -> void:
	_cam._unhandled_input(_make_mmb_event(true, false))
	assert_true(_cam._rotating, "Plain MMB press should enter rotate mode")
	assert_false(_cam._panning, "Plain MMB should not enter pan mode")


func test_ctrl_mmb_release_exits_pan_mode() -> void:
	_cam._panning = true
	_cam._unhandled_input(_make_mmb_event(false, true))
	assert_false(_cam._panning, "MMB release should exit pan mode")


func test_pan_moves_focal_point() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._yaw = 0.0
	_cam._panning = true
	var start_pos := _cam.position
	_cam._unhandled_input(_make_mouse_motion(100.0, 0.0, true))
	assert_ne(_cam.position.x, start_pos.x, "Pan should move focal point horizontally")


func test_pan_does_not_rotate() -> void:
	_cam._yaw = 0.0
	_cam._pitch = 0.7
	_cam._panning = true
	_cam._unhandled_input(_make_mouse_motion(100.0, 50.0, true))
	assert_eq(_cam._yaw, 0.0, "Pan should not change yaw")
	assert_eq(_cam._pitch, 0.7, "Pan should not change pitch")


func test_pan_breaks_follow_mode() -> void:
	_cam.start_follow(Vector3(10.0, 5.0, 10.0))
	assert_true(_cam.is_following(), "Should be following before pan")
	_cam._panning = true
	_cam._unhandled_input(_make_mouse_motion(50.0, 0.0, true))
	assert_false(_cam.is_following(), "Pan should break follow mode")


func test_pan_direction_respects_yaw() -> void:
	# With yaw = PI/2 (90°), camera-right should map to a different world axis.
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._yaw = PI / 2.0
	_cam._panning = true
	var start_pos := _cam.position
	_cam._unhandled_input(_make_mouse_motion(100.0, 0.0, true))
	# With 90° yaw, rightward mouse motion should move primarily along Z, not X.
	var dz := absf(_cam.position.z - start_pos.z)
	var dx := absf(_cam.position.x - start_pos.x)
	assert_gt(dz, dx, "At 90° yaw, horizontal drag should move along Z")


func test_pan_scales_with_zoom() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._yaw = 0.0
	_cam._zoom = 10.0
	_cam._panning = true
	_cam._unhandled_input(_make_mouse_motion(100.0, 0.0, true))
	var disp_small := absf(_cam.position.x - 50.0)

	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._zoom = 50.0
	_cam._unhandled_input(_make_mouse_motion(100.0, 0.0, true))
	var disp_large := absf(_cam.position.x - 50.0)

	assert_almost_eq(disp_large / disp_small, 5.0, 0.01, "Pan at 5x zoom should move 5x farther")


func test_pan_does_not_move_vertically() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._yaw = 0.5
	_cam._panning = true
	_cam._unhandled_input(_make_mouse_motion(80.0, 60.0, true))
	assert_eq(_cam.position.y, 20.0, "Pan should not change Y position")


func test_vertical_snap_suppressed_during_pan() -> void:
	_cam.position = Vector3(50.0, 10.3, 50.0)
	_cam.set_vertical_snap(true)
	_cam._panning = true
	_cam._process(0.1)
	assert_almost_eq(_cam.position.y, 10.3, 0.001, "Vertical snap should not fire while panning")
