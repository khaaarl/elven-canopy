## Orbital camera controller for Elven Canopy.
##
## The camera orbits a pivot/focal point (this node's position). The actual
## Camera3D is a child node offset along the orbit radius.
##
## Controls:
## - WASD: move the focal point horizontally, relative to camera facing.
## - Q/E or Left/Right arrows: rotate the camera around the focal point.
## - Up/Down arrows or middle-mouse drag: tilt the camera up/down.
## - Scroll wheel or +/- keys: zoom (distance from focal point to camera).
## - Page Up/Down: move the focal point vertically (clamped to world bounds).
##
## Follow mode: main.gd calls start_follow() / update_follow_target() to lock
## the pivot onto a creature's position each frame. WASD and vertical movement
## break follow automatically; rotation and zoom do not. The creature info
## panel's Follow/Unfollow button drives entry and exit.
##
## Voxel snap mode: construction_controller.gd calls set_voxel_snap() to
## enable/disable. When active and no movement inputs are held, the focal
## point smoothly lerps to the nearest voxel center (+0.5 offset to match
## renderer centering). get_focus_voxel() returns the current rounded voxel
## coordinate for use by construction systems (e.g., ghost mesh placement).
##
## See also: main.gd which instantiates the scene and drives follow updates,
## selection_controller.gd and creature_info_panel.gd for the selection and
## follow UI, construction_controller.gd for voxel snap toggling.

extends Node3D

## Horizontal move speed in units per second.
@export var move_speed: float = 20.0
## Rotation speed in radians per second (keyboard).
@export var rotation_speed: float = 2.0
## Tilt speed in radians per second (keyboard).
@export var tilt_speed: float = 1.5
## Mouse drag rotation sensitivity (radians per pixel).
@export var mouse_rotation_sensitivity: float = 0.005
## Mouse drag pitch sensitivity (radians per pixel).
@export var mouse_pitch_sensitivity: float = 0.005
## Zoom speed (units per scroll step).
@export var zoom_speed: float = 2.0
## Minimum zoom distance.
@export var zoom_min: float = 5.0
## Maximum zoom distance.
@export var zoom_max: float = 100.0
## Vertical movement speed (Page Up/Down) in units per second.
@export var vertical_speed: float = 15.0
## Minimum pitch angle from horizontal (radians). 10° — nearly horizontal.
@export var pitch_min: float = 0.175
## Maximum pitch angle from horizontal (radians). 80° — nearly straight down.
@export var pitch_max: float = 1.396
## Minimum focal point height (ground level).
@export var focal_y_min: float = 0.0
## Maximum focal point height (top of prototype world).
@export var focal_y_max: float = 256.0

## Current orbit yaw (horizontal rotation), in radians.
var _yaw: float = 0.0
## Current orbit pitch (vertical angle), in radians.
var _pitch: float = 0.7  # ~40°, a comfortable default
## Current zoom distance from focal point to camera.
var _zoom: float = 30.0
## Whether middle mouse is being held for drag rotation/tilt.
var _rotating: bool = false
## Whether the camera is in follow mode (tracking a creature).
var _following: bool = false
## Whether voxel-snap is enabled (construction mode).
var _snap_to_voxel: bool = false
## Lerp speed for voxel snap (units per second, exponential decay).
const SNAP_LERP_SPEED: float = 8.0
## Whether a tentative snap target is active (for short key taps in snap mode).
var _has_tentative: bool = false
## Voxel center to snap to on short tap (when key released before crossing boundary).
var _tentative_target: Vector3 = Vector3.ZERO
## Voxel center when positional movement started in snap mode.
var _snap_origin: Vector3 = Vector3.ZERO

@onready var _camera: Camera3D = $Camera3D


## Enter follow mode: camera pivot snaps to the target and tracks it.
func start_follow(target_pos: Vector3) -> void:
	_following = true
	position = target_pos
	_update_camera_transform()


## Update the follow target position. Call each frame while following.
func update_follow_target(target_pos: Vector3) -> void:
	if _following:
		position = target_pos
		_update_camera_transform()


## Exit follow mode. Camera stays where it is.
func stop_follow() -> void:
	_following = false


## Returns true if the camera is in follow mode.
func is_following() -> bool:
	return _following


## Enable or disable voxel-snap mode (used by construction_controller.gd).
func set_voxel_snap(enabled: bool) -> void:
	_snap_to_voxel = enabled
	_has_tentative = false


## Returns the voxel coordinate the camera focus is currently snapped to.
## Useful for construction systems that need to know the selected voxel.
func get_focus_voxel() -> Vector3i:
	return Vector3i(
		int(floor(position.x)),
		int(floor(position.y)),
		int(floor(position.z))
	)


func _ready() -> void:
	_update_camera_transform()


func _unhandled_input(event: InputEvent) -> void:
	# Scroll wheel zoom.
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_WHEEL_UP:
			_zoom = max(_zoom - zoom_speed, zoom_min)
			_update_camera_transform()
		elif mb.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			_zoom = min(_zoom + zoom_speed, zoom_max)
			_update_camera_transform()
		elif mb.button_index == MOUSE_BUTTON_MIDDLE:
			_rotating = mb.pressed

	# Keyboard zoom (+/= to zoom in, -/_ to zoom out).
	if event is InputEventKey and event.pressed:
		if event.physical_keycode == KEY_EQUAL:
			_zoom = max(_zoom - zoom_speed, zoom_min)
			_update_camera_transform()
			get_viewport().set_input_as_handled()
		elif event.physical_keycode == KEY_MINUS:
			_zoom = min(_zoom + zoom_speed, zoom_max)
			_update_camera_transform()
			get_viewport().set_input_as_handled()

	# Middle-mouse drag rotation and tilt.
	if event is InputEventMouseMotion and _rotating:
		var mm := event as InputEventMouseMotion
		_yaw -= mm.relative.x * mouse_rotation_sensitivity
		_pitch = clamp(
			_pitch + mm.relative.y * mouse_pitch_sensitivity,
			pitch_min,
			pitch_max
		)
		_update_camera_transform()


func _process(delta: float) -> void:
	var moved := false
	var position_moved := false
	var move_dir := Vector3.ZERO

	# Capture pre-movement voxel center for tentative snap tracking.
	var pre_move_voxel := Vector3(
		floor(position.x) + 0.5,
		floor(position.y) + 0.5,
		floor(position.z) + 0.5
	)

	# Horizontal movement relative to camera facing.
	var input_dir := Vector2.ZERO
	if Input.is_action_pressed("move_forward"):
		input_dir.y -= 1.0
	if Input.is_action_pressed("move_back"):
		input_dir.y += 1.0
	if Input.is_action_pressed("move_left"):
		input_dir.x -= 1.0
	if Input.is_action_pressed("move_right"):
		input_dir.x += 1.0

	if input_dir.length_squared() > 0.0:
		input_dir = input_dir.normalized()
		# Movement is relative to the camera's horizontal facing (yaw only).
		var forward := Vector3(-sin(_yaw), 0.0, -cos(_yaw))
		var right := Vector3(cos(_yaw), 0.0, -sin(_yaw))
		var movement := (forward * -input_dir.y + right * input_dir.x) * move_speed * delta
		position += movement
		move_dir += movement
		moved = true
		position_moved = true
		_following = false

	# Keyboard rotation (Q/E and Left/Right arrows).
	if Input.is_action_pressed("rotate_left"):
		_yaw += rotation_speed * delta
		moved = true
	if Input.is_action_pressed("rotate_right"):
		_yaw -= rotation_speed * delta
		moved = true

	# Keyboard tilt (Up/Down arrows).
	if Input.is_action_pressed("tilt_up"):
		_pitch = min(_pitch + tilt_speed * delta, pitch_max)
		moved = true
	if Input.is_action_pressed("tilt_down"):
		_pitch = max(_pitch - tilt_speed * delta, pitch_min)
		moved = true

	# Vertical focal point movement (Page Up/Down), clamped to world bounds.
	if Input.is_action_pressed("focal_up"):
		var old_y := position.y
		position.y = min(position.y + vertical_speed * delta, focal_y_max)
		move_dir.y += position.y - old_y
		moved = true
		position_moved = true
		_following = false
	if Input.is_action_pressed("focal_down"):
		var old_y := position.y
		position.y = max(position.y - vertical_speed * delta, focal_y_min)
		move_dir.y += position.y - old_y
		moved = true
		position_moved = true
		_following = false

	# Tentative snap target: when moving in snap mode, track the intended
	# next voxel so short key taps always advance by at least one voxel.
	# On the first frame of movement, record the origin and compute the
	# tentative target. On subsequent frames, clear tentative if we've
	# crossed a voxel boundary (natural movement takes over).
	if _snap_to_voxel and position_moved:
		var post_move_voxel := Vector3(
			floor(position.x) + 0.5,
			floor(position.y) + 0.5,
			floor(position.z) + 0.5
		)
		if not _has_tentative:
			# First frame of movement — record origin and set tentative.
			_snap_origin = pre_move_voxel
			_has_tentative = true
			var dir := move_dir.normalized()
			var step := Vector3.ZERO
			if abs(dir.x) > 0.4:
				step.x = signf(dir.x)
			if abs(dir.y) > 0.4:
				step.y = signf(dir.y)
			if abs(dir.z) > 0.4:
				step.z = signf(dir.z)
			_tentative_target = _snap_origin + step
		elif post_move_voxel.distance_squared_to(_snap_origin) > 0.01:
			# Crossed a voxel boundary naturally — tentative no longer needed.
			_has_tentative = false

	if moved:
		_update_camera_transform()

	# Voxel snap: when enabled and no inputs are active, smoothly pull the
	# focal point to the nearest voxel center (+0.5 to match renderer offset).
	# If a tentative target exists (from a short key tap), snap there instead.
	if _snap_to_voxel and not moved and not _rotating:
		var snap_target: Vector3
		if _has_tentative:
			snap_target = _tentative_target
		else:
			snap_target = Vector3(
				floor(position.x) + 0.5,
				floor(position.y) + 0.5,
				floor(position.z) + 0.5
			)
		var dist_sq := position.distance_squared_to(snap_target)
		if dist_sq > 0.0001:
			position = position.lerp(snap_target, SNAP_LERP_SPEED * delta)
			_update_camera_transform()
		elif dist_sq > 0.0:
			position = snap_target
			_update_camera_transform()
		# Clear tentative once we've arrived at the target.
		if _has_tentative and position.distance_squared_to(_tentative_target) < 0.001:
			_has_tentative = false


func _update_camera_transform() -> void:
	# Position the camera on a sphere around the focal point (this node).
	var offset := Vector3(
		sin(_yaw) * cos(_pitch),
		sin(_pitch),
		cos(_yaw) * cos(_pitch)
	) * _zoom

	_camera.position = offset
	_camera.look_at(global_position)
