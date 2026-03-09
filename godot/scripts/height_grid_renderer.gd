## Wireframe height-slice grid overlay for construction placement.
##
## Renders ghostly wireframe cube outlines at the camera's current Y-level,
## showing the voxel grid for platform and carve placement modes. Cubes
## near the camera focus are most opaque, fading with distance. Cubes over
## solid voxels are dimmed to avoid obscuring the tree.
##
## Blueprint-aware: uses get_voxel_solidity_slice_with_blueprints() so that
## designated (not yet built) blueprints appear as solid in the grid overlay.
##
## The grid is rebuilt only when the integer Y-level or camera focus (X, Z)
## changes — not every frame. Uses ImmediateMesh with line segments.
##
## Created by main.gd, referenced by construction_controller.gd which
## toggles visibility on mode transitions. The bridge provides
## get_voxel_solidity_slice_with_blueprints() for solid/air data.
##
## See also: construction_controller.gd for visibility control,
## orbital_camera.gd for get_focus_voxel(), sim_bridge.rs for
## get_voxel_solidity_slice_with_blueprints().

extends Node3D

const GRID_RADIUS: int = 15
const SOLID_DIM_FACTOR: float = 0.15

var _bridge: SimBridge
var _camera_pivot: Node3D
var _mesh_instance: MeshInstance3D
var _material: StandardMaterial3D
var _last_grid_y: int = -99999
var _last_grid_cx: int = -99999
var _last_grid_cz: int = -99999
var _world_size: Vector3i = Vector3i.ZERO


func setup(bridge: SimBridge, camera_pivot: Node3D) -> void:
	_bridge = bridge
	_camera_pivot = camera_pivot
	_world_size = bridge.get_world_size()

	_material = StandardMaterial3D.new()
	_material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	_material.vertex_color_use_as_albedo = true
	_material.no_depth_test = true
	_material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED

	_mesh_instance = MeshInstance3D.new()
	_mesh_instance.material_override = _material
	add_child(_mesh_instance)


func _process(_delta: float) -> void:
	if not visible:
		return
	if not _camera_pivot or not _camera_pivot.has_method("get_focus_voxel"):
		return

	var focus: Vector3i = _camera_pivot.get_focus_voxel()
	var gy: int = focus.y
	var cx: int = focus.x
	var cz: int = focus.z

	if gy == _last_grid_y and cx == _last_grid_cx and cz == _last_grid_cz:
		return

	_last_grid_y = gy
	_last_grid_cx = cx
	_last_grid_cz = cz
	_rebuild_grid(gy, cx, cz)


func _rebuild_grid(y: int, cx: int, cz: int) -> void:
	var solidity := _bridge.get_voxel_solidity_slice_with_blueprints(y, cx, cz, GRID_RADIUS)
	var side: int = 2 * GRID_RADIUS + 1
	var mesh := ImmediateMesh.new()

	mesh.surface_begin(Mesh.PRIMITIVE_LINES)

	var max_radius_f := float(GRID_RADIUS)
	for gz in range(-GRID_RADIUS, GRID_RADIUS + 1):
		for gx in range(-GRID_RADIUS, GRID_RADIUS + 1):
			var wx: int = cx + gx
			var wz: int = cz + gz

			# Skip out-of-world voxels.
			if wx < 0 or wx >= _world_size.x or wz < 0 or wz >= _world_size.z:
				continue

			var dist: float = sqrt(float(gx * gx + gz * gz))
			var base_alpha := maxf(0.0, 1.0 - dist / max_radius_f)
			if base_alpha <= 0.0:
				continue

			var idx: int = (gx + GRID_RADIUS) + (gz + GRID_RADIUS) * side
			var is_solid: bool = idx < solidity.size() and solidity[idx] == 1
			var alpha: float = base_alpha * SOLID_DIM_FACTOR if is_solid else base_alpha

			_emit_wireframe_cube(mesh, float(wx), float(y), float(wz), alpha)

	mesh.surface_end()
	_mesh_instance.mesh = mesh


## Emit the 12 line edges of a unit cube at (x, y, z) with given alpha.
func _emit_wireframe_cube(mesh: ImmediateMesh, x: float, y: float, z: float, alpha: float) -> void:
	var color := Color(0.7, 0.8, 1.0, alpha)
	# Bottom face edges.
	_line(mesh, color, x, y, z, x + 1, y, z)
	_line(mesh, color, x + 1, y, z, x + 1, y, z + 1)
	_line(mesh, color, x + 1, y, z + 1, x, y, z + 1)
	_line(mesh, color, x, y, z + 1, x, y, z)
	# Top face edges.
	_line(mesh, color, x, y + 1, z, x + 1, y + 1, z)
	_line(mesh, color, x + 1, y + 1, z, x + 1, y + 1, z + 1)
	_line(mesh, color, x + 1, y + 1, z + 1, x, y + 1, z + 1)
	_line(mesh, color, x, y + 1, z + 1, x, y + 1, z)
	# Vertical edges.
	_line(mesh, color, x, y, z, x, y + 1, z)
	_line(mesh, color, x + 1, y, z, x + 1, y + 1, z)
	_line(mesh, color, x + 1, y, z + 1, x + 1, y + 1, z + 1)
	_line(mesh, color, x, y, z + 1, x, y + 1, z + 1)


func _line(
	mesh: ImmediateMesh,
	color: Color,
	x1: float,
	y1: float,
	z1: float,
	x2: float,
	y2: float,
	z2: float,
) -> void:
	mesh.surface_set_color(color)
	mesh.surface_add_vertex(Vector3(x1, y1, z1))
	mesh.surface_set_color(color)
	mesh.surface_add_vertex(Vector3(x2, y2, z2))
