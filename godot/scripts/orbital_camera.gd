## Orbital camera controller for Elven Canopy.
##
## The camera orbits a pivot/focal point (this node's position). The actual
## Camera3D is a child node offset along the orbit radius.
##
## Controls:
## - WASD: move the focal point horizontally, relative to camera facing.
## - Q/E or Left/Right arrows: rotate the camera around the focal point.
## - Up/Down arrows or middle-mouse drag: tilt the camera up/down.
## - Ctrl+middle-mouse drag: pan the focal point horizontally.
## - Scroll wheel or +/= and - keys: zoom (distance from focal point to camera).
## - Ctrl+scroll wheel: move the focal point vertically by 1 unit per tick.
## - Page Up/Down: move the focal point vertically (clamped to world bounds).
## - Home: center focal point on the home tree (emits home_requested signal).
## - R/F formerly duplicated Page Up/Down; removed to free F for attack-move.
##
## Follow mode: main.gd calls start_follow() / update_follow_target() to lock
## the pivot onto a creature's position each frame. WASD and vertical movement
## break follow automatically; rotation and zoom do not. The creature info
## panel's Follow/Unfollow button drives entry and exit.
##
## Vertical snap mode: construction_controller.gd calls set_vertical_snap()
## to enable/disable. When active and no vertical movement inputs are held,
## the focal point's Y smoothly lerps to the nearest voxel center Y
## (+0.5 offset to match renderer centering). X and Z are not snapped.
## get_focus_voxel() returns the current floor'd voxel coordinate for use
## by construction systems (e.g., height-slice projection).
##
## See also: main.gd which instantiates the scene and drives follow updates,
## selection_controller.gd and creature_info_panel.gd for the selection and
## follow UI, construction_controller.gd for vertical snap toggling.

extends Node3D

## Emitted when the user presses the Home key to center on the home tree.
signal home_requested

## Lerp speed for vertical snap (units per second, exponential decay).
const SNAP_LERP_SPEED: float = 8.0

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
## Keyboard zoom speed (units per second).
@export var key_zoom_speed: float = 30.0
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
## Mouse pan sensitivity (world-units per pixel at zoom=1; scaled by current zoom).
@export var mouse_pan_sensitivity: float = 0.002

## Current orbit yaw (horizontal rotation), in radians.
var _yaw: float = 0.0
## Current orbit pitch (vertical angle), in radians.
var _pitch: float = 0.7  # ~40°, a comfortable default
## Current zoom distance from focal point to camera.
var _zoom: float = 30.0
## Whether middle mouse is being held for drag rotation/tilt.
var _rotating: bool = false
## Whether Ctrl+middle mouse is being held for drag panning.
var _panning: bool = false
## Whether the camera is in follow mode (tracking a creature).
var _following: bool = false
## Whether vertical-snap is enabled (construction mode — Y axis only).
var _vertical_snap: bool = false
## Whether a tentative snap target is active (for short key taps in snap mode).
var _has_tentative: bool = false
## Voxel center Y to snap to on short tap (when key released before crossing boundary).
var _tentative_target_y: float = 0.0
## Voxel center Y when vertical movement started in snap mode.
var _snap_origin_y: float = 0.0

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


## Enable or disable vertical-snap mode (used by construction_controller.gd).
## When active, only the Y axis snaps to voxel centers; X and Z move freely.
func set_vertical_snap(enabled: bool) -> void:
	_vertical_snap = enabled
	_has_tentative = false


## Returns the voxel coordinate the camera focus is currently snapped to.
## Useful for construction systems that need to know the selected voxel.
func get_focus_voxel() -> Vector3i:
	return Vector3i(int(floor(position.x)), int(floor(position.y)), int(floor(position.z)))


func _ready() -> void:
	_update_camera_transform()


func _unhandled_input(event: InputEvent) -> void:
	# Scroll wheel: Ctrl+scroll adjusts elevation, plain scroll zooms.
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.ctrl_pressed and mb.button_index == MOUSE_BUTTON_WHEEL_UP:
			position.y = min(position.y + 1.0, focal_y_max)
			_following = false
			_update_camera_transform()
		elif mb.ctrl_pressed and mb.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			position.y = max(position.y - 1.0, focal_y_min)
			_following = false
			_update_camera_transform()
		elif mb.button_index == MOUSE_BUTTON_WHEEL_UP:
			_zoom = max(_zoom - zoom_speed, zoom_min)
			_update_camera_transform()
		elif mb.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			_zoom = min(_zoom + zoom_speed, zoom_max)
			_update_camera_transform()
		elif mb.button_index == MOUSE_BUTTON_MIDDLE:
			if mb.ctrl_pressed:
				_panning = mb.pressed
				_rotating = false
			else:
				_rotating = mb.pressed
				_panning = false

	# Home key: center camera on the home tree.
	if event is InputEventKey and event.pressed and not event.echo:
		var key := event as InputEventKey
		if key.ctrl_pressed or key.shift_pressed or key.alt_pressed:
			return
		if key.keycode == KEY_HOME:
			home_requested.emit()
			get_viewport().set_input_as_handled()

	# Middle-mouse drag rotation and tilt.
	if event is InputEventMouseMotion and _rotating:
		var mm := event as InputEventMouseMotion
		_yaw -= mm.relative.x * mouse_rotation_sensitivity
		_pitch = clamp(_pitch + mm.relative.y * mouse_pitch_sensitivity, pitch_min, pitch_max)
		_update_camera_transform()

	# Ctrl+middle-mouse drag to pan the focal point horizontally.
	if event is InputEventMouseMotion and _panning:
		var mm := event as InputEventMouseMotion
		var right := Vector3(cos(_yaw), 0.0, -sin(_yaw))
		var forward := Vector3(-sin(_yaw), 0.0, -cos(_yaw))
		var pan_scale := mouse_pan_sensitivity * _zoom
		position += right * mm.relative.x * pan_scale + forward * mm.relative.y * pan_scale
		_following = false
		_update_camera_transform()


func _process(delta: float) -> void:
	var moved := false
	var position_moved := false
	var move_dir := Vector3.ZERO

	# Capture pre-movement voxel center Y for tentative vertical snap tracking.
	var pre_move_voxel_y: float = floorf(position.y) + 0.5

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

	# Keyboard zoom (+/= to zoom in, -/_ to zoom out).
	if Input.is_key_pressed(KEY_EQUAL) or Input.is_key_pressed(KEY_KP_ADD):
		_zoom = max(_zoom - key_zoom_speed * delta, zoom_min)
		moved = true
	if Input.is_key_pressed(KEY_MINUS) or Input.is_key_pressed(KEY_KP_SUBTRACT):
		_zoom = min(_zoom + key_zoom_speed * delta, zoom_max)
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

	# Tentative vertical snap: when moving vertically in snap mode, track the
	# intended next Y voxel so short PgUp/PgDn taps advance by one voxel.
	if _vertical_snap and position_moved and abs(move_dir.y) > 0.001:
		var post_move_voxel_y: float = floorf(position.y) + 0.5
		if not _has_tentative:
			_snap_origin_y = pre_move_voxel_y
			_has_tentative = true
			_tentative_target_y = _snap_origin_y + signf(move_dir.y)
		elif abs(post_move_voxel_y - _snap_origin_y) > 0.01:
			_has_tentative = false

	if moved:
		_update_camera_transform()

	# Vertical snap: when enabled and no inputs are active, smoothly pull the
	# focal point's Y to the nearest voxel center Y. X and Z are not snapped.
	if _vertical_snap and not moved and not _rotating and not _panning:
		var snap_y: float
		if _has_tentative:
			snap_y = _tentative_target_y
		else:
			snap_y = floorf(position.y) + 0.5
		var dy: float = absf(position.y - snap_y)
		if dy > 0.0001:
			position.y = lerpf(position.y, snap_y, SNAP_LERP_SPEED * delta)
			_update_camera_transform()
		elif dy > 0.0:
			position.y = snap_y
			_update_camera_transform()
		if _has_tentative and abs(position.y - _tentative_target_y) < 0.001:
			_has_tentative = false


func _update_camera_transform() -> void:
	# Position the camera on a sphere around the focal point (this node).
	var offset := Vector3(sin(_yaw) * cos(_pitch), sin(_pitch), cos(_yaw) * cos(_pitch)) * _zoom

	_camera.position = offset
	_camera.look_at(global_position)
