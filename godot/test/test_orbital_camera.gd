## Unit tests for orbital_camera.gd.
##
## Tests get_focus_voxel(), home_requested signal, Ctrl+MMB pan, Ctrl+scroll
## elevation, vertical snap, and edge scroll (compute_edge_direction + mode
## integration for off/pan/rotate) by constructing a Node3D with the orbital
## camera script attached.
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


# -- Ctrl+scroll wheel elevation -----------------------------------------------


func _make_scroll_event(direction: int, ctrl: bool = false) -> InputEventMouseButton:
	var mb := InputEventMouseButton.new()
	mb.button_index = direction
	mb.pressed = true
	mb.ctrl_pressed = ctrl
	return mb


func test_ctrl_scroll_up_raises_elevation() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._unhandled_input(_make_scroll_event(MOUSE_BUTTON_WHEEL_UP, true))
	assert_gt(_cam.position.y, 20.0, "Ctrl+scroll up should raise focal Y")


func test_ctrl_scroll_down_lowers_elevation() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._unhandled_input(_make_scroll_event(MOUSE_BUTTON_WHEEL_DOWN, true))
	assert_lt(_cam.position.y, 20.0, "Ctrl+scroll down should lower focal Y")


func test_ctrl_scroll_does_not_change_zoom() -> void:
	_cam._zoom = 30.0
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._unhandled_input(_make_scroll_event(MOUSE_BUTTON_WHEEL_UP, true))
	assert_eq(_cam._zoom, 30.0, "Ctrl+scroll should not change zoom")


func test_ctrl_scroll_step_size() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._unhandled_input(_make_scroll_event(MOUSE_BUTTON_WHEEL_UP, true))
	assert_almost_eq(_cam.position.y, 21.0, 0.001, "Ctrl+scroll up should move by 1.0")


func test_ctrl_scroll_clamped_to_max() -> void:
	_cam.position = Vector3(50.0, 256.0, 50.0)
	_cam._unhandled_input(_make_scroll_event(MOUSE_BUTTON_WHEEL_UP, true))
	assert_eq(_cam.position.y, 256.0, "Should not exceed focal_y_max")


func test_ctrl_scroll_clamped_to_min() -> void:
	_cam.position = Vector3(50.0, 0.0, 50.0)
	_cam._unhandled_input(_make_scroll_event(MOUSE_BUTTON_WHEEL_DOWN, true))
	assert_eq(_cam.position.y, 0.0, "Should not go below focal_y_min")


func test_ctrl_scroll_breaks_follow() -> void:
	_cam.start_follow(Vector3(10.0, 5.0, 10.0))
	assert_true(_cam.is_following(), "Should be following before ctrl+scroll")
	_cam._unhandled_input(_make_scroll_event(MOUSE_BUTTON_WHEEL_UP, true))
	assert_false(_cam.is_following(), "Ctrl+scroll should break follow mode")


func test_plain_scroll_still_zooms() -> void:
	_cam._zoom = 30.0
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._unhandled_input(_make_scroll_event(MOUSE_BUTTON_WHEEL_UP, false))
	assert_lt(_cam._zoom, 30.0, "Plain scroll up should zoom in")
	assert_eq(_cam.position.y, 20.0, "Plain scroll should not change Y")


func test_vertical_snap_suppressed_during_pan() -> void:
	_cam.position = Vector3(50.0, 10.3, 50.0)
	_cam.set_vertical_snap(true)
	_cam._panning = true
	_cam._process(0.1)
	assert_almost_eq(_cam.position.y, 10.3, 0.001, "Vertical snap should not fire while panning")


# -- Edge scroll direction computation ----------------------------------------


func test_edge_scroll_center_returns_zero() -> void:
	# Mouse in the center of the screen should produce no scroll direction.
	var dir := OrbitalCamera.compute_edge_direction(Vector2(500, 400), Vector2(1000, 800), 3)
	assert_eq(dir, Vector2.ZERO, "Center of screen should not trigger edge scroll")


func test_edge_scroll_left_edge() -> void:
	var dir := OrbitalCamera.compute_edge_direction(Vector2(1, 400), Vector2(1000, 800), 3)
	assert_eq(dir.x, -1.0, "Left edge should produce -1 X")
	assert_eq(dir.y, 0.0, "Left edge should not affect Y")


func test_edge_scroll_right_edge() -> void:
	var dir := OrbitalCamera.compute_edge_direction(Vector2(998, 400), Vector2(1000, 800), 3)
	assert_eq(dir.x, 1.0, "Right edge should produce +1 X")
	assert_eq(dir.y, 0.0, "Right edge should not affect Y")


func test_edge_scroll_top_edge() -> void:
	var dir := OrbitalCamera.compute_edge_direction(Vector2(500, 1), Vector2(1000, 800), 3)
	assert_eq(dir.x, 0.0, "Top edge should not affect X")
	assert_eq(dir.y, -1.0, "Top edge should produce -1 Y")


func test_edge_scroll_bottom_edge() -> void:
	var dir := OrbitalCamera.compute_edge_direction(Vector2(500, 798), Vector2(1000, 800), 3)
	assert_eq(dir.x, 0.0, "Bottom edge should not affect X")
	assert_eq(dir.y, 1.0, "Bottom edge should produce +1 Y")


func test_edge_scroll_corner() -> void:
	# Top-left corner should produce both -1 X and -1 Y.
	var dir := OrbitalCamera.compute_edge_direction(Vector2(0, 0), Vector2(1000, 800), 3)
	assert_eq(dir.x, -1.0, "Top-left corner should produce -1 X")
	assert_eq(dir.y, -1.0, "Top-left corner should produce -1 Y")


func test_edge_scroll_fixed_speed_no_gradient() -> void:
	# All positions within the margin produce the same magnitude (no gradient).
	var dir_edge := OrbitalCamera.compute_edge_direction(Vector2(0, 400), Vector2(1000, 800), 3)
	var dir_inner := OrbitalCamera.compute_edge_direction(Vector2(2, 400), Vector2(1000, 800), 3)
	assert_eq(dir_edge.x, dir_inner.x, "Edge and inner margin should have same intensity")


func test_edge_scroll_at_exact_margin_boundary() -> void:
	# At exactly the margin boundary (x=3 with margin=3), should be zero.
	var dir := OrbitalCamera.compute_edge_direction(Vector2(3, 400), Vector2(1000, 800), 3)
	assert_eq(dir, Vector2.ZERO, "At margin boundary should produce no scroll")


func test_edge_scroll_just_inside_margin() -> void:
	# At x=2 with margin=3, should produce full -1 X (fixed speed).
	var dir := OrbitalCamera.compute_edge_direction(Vector2(2, 400), Vector2(1000, 800), 3)
	assert_eq(dir.x, -1.0, "Just inside margin should produce full scroll")


func test_edge_scroll_zero_margin_returns_zero() -> void:
	# With margin=0, edge scroll is effectively disabled.
	var dir := OrbitalCamera.compute_edge_direction(Vector2(0, 0), Vector2(1000, 800), 0)
	assert_eq(dir, Vector2.ZERO, "Zero margin should never trigger")


# -- Edge scroll mode integration ---------------------------------------------


func test_edge_scroll_mode_off_no_movement() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._edge_scroll_mode = "off"
	var start_pos := _cam.position
	# Simulate mouse at left edge — but mode is off.
	_cam._override_mouse_pos = Vector2(1, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_eq(_cam.position, start_pos, "Edge scroll off should not move camera")


func test_edge_scroll_pan_moves_focal_point() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._yaw = 0.0
	_cam._edge_scroll_mode = "pan"
	var start_pos := _cam.position
	# Mouse at left edge.
	_cam._override_mouse_pos = Vector2(1, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_ne(_cam.position.x, start_pos.x, "Pan mode should move focal point X")
	assert_eq(_cam.position.y, start_pos.y, "Pan mode should not change Y")


func test_edge_scroll_rotate_changes_yaw() -> void:
	_cam._yaw = 0.0
	_cam._pitch = 0.7
	_cam._edge_scroll_mode = "rotate"
	# Mouse at right edge.
	_cam._override_mouse_pos = Vector2(999, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_ne(_cam._yaw, 0.0, "Rotate mode at right edge should change yaw")


func test_edge_scroll_rotate_changes_pitch() -> void:
	_cam._yaw = 0.0
	_cam._pitch = 0.7
	_cam._edge_scroll_mode = "rotate"
	# Mouse at top edge.
	_cam._override_mouse_pos = Vector2(500, 1)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_ne(_cam._pitch, 0.7, "Rotate mode at top edge should change pitch")


func test_edge_scroll_pan_breaks_follow() -> void:
	_cam.start_follow(Vector3(10.0, 5.0, 10.0))
	assert_true(_cam.is_following(), "Should be following before edge scroll")
	_cam._edge_scroll_mode = "pan"
	_cam._override_mouse_pos = Vector2(1, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_false(_cam.is_following(), "Edge scroll pan should break follow mode")


func test_edge_scroll_rotate_does_not_break_follow() -> void:
	_cam.start_follow(Vector3(10.0, 5.0, 10.0))
	assert_true(_cam.is_following(), "Should be following before edge scroll")
	_cam._edge_scroll_mode = "rotate"
	_cam._override_mouse_pos = Vector2(999, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_true(_cam.is_following(), "Edge scroll rotate should not break follow")


func test_edge_scroll_pan_direction_respects_yaw() -> void:
	# At yaw = PI/2 (90°), rightward edge scroll should move primarily along Z.
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._yaw = PI / 2.0
	_cam._edge_scroll_mode = "pan"
	var start_pos := _cam.position
	# Mouse at right edge.
	_cam._override_mouse_pos = Vector2(999, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	var dz := absf(_cam.position.z - start_pos.z)
	var dx := absf(_cam.position.x - start_pos.x)
	assert_gt(dz, dx, "At 90° yaw, right-edge pan should move primarily along Z")


func test_edge_scroll_pan_top_edge_moves_forward() -> void:
	# Mouse at top edge should move the focal point forward (like pressing W).
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._yaw = 0.0
	_cam._edge_scroll_mode = "pan"
	var start_z := _cam.position.z
	# Mouse at top edge.
	_cam._override_mouse_pos = Vector2(500, 1)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	# At yaw=0, forward is (0, 0, -1), so Z should decrease.
	assert_lt(_cam.position.z, start_z, "Top edge pan should move forward (negative Z at yaw=0)")


func test_edge_scroll_pan_center_mouse_no_movement() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._edge_scroll_mode = "pan"
	var start_pos := _cam.position
	# Mouse in center — outside edge margin.
	_cam._override_mouse_pos = Vector2(500, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_eq(_cam.position, start_pos, "Pan mode with centered mouse should not move")


func test_edge_scroll_rotate_center_mouse_no_change() -> void:
	_cam._yaw = 0.0
	_cam._pitch = 0.7
	_cam._edge_scroll_mode = "rotate"
	# Mouse in center — outside edge margin.
	_cam._override_mouse_pos = Vector2(500, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_eq(_cam._yaw, 0.0, "Rotate mode with centered mouse should not change yaw")
	assert_eq(_cam._pitch, 0.7, "Rotate mode with centered mouse should not change pitch")


func test_edge_scroll_rotate_pitch_clamped_at_max() -> void:
	_cam._pitch = 1.3  # Near pitch_max (1.396).
	_cam._edge_scroll_mode = "rotate"
	# Mouse at bottom edge — should increase pitch, but not beyond max.
	_cam._override_mouse_pos = Vector2(500, 799)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(1.0)  # Large delta to ensure clamp is hit.
	assert_le(_cam._pitch, _cam.pitch_max, "Pitch should not exceed pitch_max")


func test_edge_scroll_rotate_pitch_clamped_at_min() -> void:
	_cam._pitch = 0.2  # Near pitch_min (0.175).
	_cam._edge_scroll_mode = "rotate"
	# Mouse at top edge — should decrease pitch, but not below min.
	_cam._override_mouse_pos = Vector2(500, 1)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(1.0)
	assert_ge(_cam._pitch, _cam.pitch_min, "Pitch should not go below pitch_min")


func test_edge_scroll_invalid_mode_no_effect() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._yaw = 0.0
	_cam._pitch = 0.7
	_cam._edge_scroll_mode = "invalid"
	var start_pos := _cam.position
	_cam._override_mouse_pos = Vector2(1, 1)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_eq(_cam.position, start_pos, "Invalid mode should not move camera")
	assert_eq(_cam._yaw, 0.0, "Invalid mode should not change yaw")
	assert_eq(_cam._pitch, 0.7, "Invalid mode should not change pitch")


func test_edge_scroll_direction_negative_mouse() -> void:
	# Mouse outside viewport (negative coords) — should still return -1.
	var dir := OrbitalCamera.compute_edge_direction(Vector2(-50, -50), Vector2(1000, 800), 3)
	assert_eq(dir.x, -1.0, "Negative mouse X should produce -1")
	assert_eq(dir.y, -1.0, "Negative mouse Y should produce -1")


func test_edge_scroll_direction_oversized_mouse() -> void:
	# Mouse beyond viewport bounds — should still return +1.
	var dir := OrbitalCamera.compute_edge_direction(Vector2(1100, 900), Vector2(1000, 800), 3)
	assert_eq(dir.x, 1.0, "Oversized mouse X should produce +1")
	assert_eq(dir.y, 1.0, "Oversized mouse Y should produce +1")


func test_edge_scroll_suppressed_when_window_unfocused() -> void:
	_cam.position = Vector3(50.0, 20.0, 50.0)
	_cam._edge_scroll_mode = "pan"
	_cam._window_focused = false
	var start_pos := _cam.position
	_cam._override_mouse_pos = Vector2(1, 400)
	_cam._override_viewport_size = Vector2(1000, 800)
	_cam._process(0.1)
	assert_eq(_cam.position, start_pos, "Edge scroll should not move when window unfocused")
