## Unit tests for orbital_camera.gd math.
##
## Tests get_focus_voxel() by constructing a Node3D with the orbital camera
## script attached, setting its position, and calling the real function.
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
