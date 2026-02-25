## Orbital camera controller for Elven Canopy.
##
## The camera orbits a pivot/focal point (this node's position). The actual
## Camera3D is a child node offset along the orbit radius.
##
## Controls:
## - WASD: move the focal point horizontally, relative to camera facing.
## - Q/E or Left/Right arrows: rotate the camera around the focal point.
## - Up/Down arrows or middle-mouse drag: tilt the camera up/down.
## - Scroll wheel: zoom (distance from focal point to camera).
## - Page Up/Down: move the focal point vertically (clamped to world bounds).
##
## See also: main.gd which instantiates the scene.

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
		moved = true
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
		position.y = min(position.y + vertical_speed * delta, focal_y_max)
		moved = true
		_following = false
	if Input.is_action_pressed("focal_down"):
		position.y = max(position.y - vertical_speed * delta, focal_y_min)
		moved = true
		_following = false

	if moved:
		_update_camera_transform()


func _update_camera_transform() -> void:
	# Position the camera on a sphere around the focal point (this node).
	var offset := Vector3(
		sin(_yaw) * cos(_pitch),
		sin(_pitch),
		cos(_yaw) * cos(_pitch)
	) * _zoom

	_camera.position = offset
	_camera.look_at(global_position)
