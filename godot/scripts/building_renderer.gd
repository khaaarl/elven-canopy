## Renders building faces as oriented quads.
##
## Unlike tree/platform voxels (solid cubes), buildings use paper-thin walls
## represented as per-face restrictions on BuildingInterior voxels. Each
## non-Open face is rendered as a QuadMesh oriented to the correct direction.
##
## Face types map to distinct materials:
## - Window: semi-transparent light blue (alpha 0.5)
## - Wall: opaque dark brown
## - Door: opaque warm brown with slightly different shade
## - Ceiling: opaque light grey
## - Floor: opaque medium grey
##
## Uses one MultiMeshInstance3D per face type for batched drawing. Rebuilt
## every frame from get_building_face_data() which returns flat quintuples
## (x, y, z, face_direction, face_type).
##
## Face direction encoding: 0=PosX, 1=NegX, 2=PosY, 3=NegY, 4=PosZ, 5=NegZ.
## Face type encoding: 0=Open, 1=Wall, 2=Window, 3=Door, 4=Ceiling, 5=Floor.
##
## See also: sim_bridge.rs for get_building_face_data(), types.rs for
## FaceDirection/FaceType, building.rs for face layout computation,
## blueprint_renderer.gd for the MultiMesh pattern, main.gd which creates
## this node and calls refresh().

extends Node3D

const FACE_INSET := 0.005

## Offset vectors indexed by face direction (0=PosX..5=NegZ).
const DIRECTION_OFFSETS := GeometryUtils.DIRECTION_OFFSETS

var _bridge: SimBridge
var _instances: Array[MultiMeshInstance3D] = []
var _ceiling_instance: MultiMeshInstance3D = null
var _roofs_hidden: bool = false
var _last_face_data: PackedInt32Array = PackedInt32Array()

# Materials indexed by face type (1=Wall, 2=Window, 3=Door, 4=Ceiling, 5=Floor).
var _materials: Array[StandardMaterial3D] = []

# Pre-computed basis rotations for each face direction.
var _face_rotations: Array[Basis] = []


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_build_materials()
	_build_rotations()
	refresh()


func _build_materials() -> void:
	# Index 0 = Open (unused, placeholder).
	var open_mat := StandardMaterial3D.new()
	_materials.append(open_mat)

	# Index 1 = Wall: opaque dark brown.
	var wall_mat := StandardMaterial3D.new()
	wall_mat.albedo_color = Color(0.40, 0.28, 0.15)
	wall_mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	_materials.append(wall_mat)

	# Index 2 = Window: semi-transparent light blue.
	var window_mat := StandardMaterial3D.new()
	window_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	window_mat.albedo_color = Color(0.6, 0.8, 1.0, 0.5)
	window_mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	_materials.append(window_mat)

	# Index 3 = Door: warm brown.
	var door_mat := StandardMaterial3D.new()
	door_mat.albedo_color = Color(0.55, 0.35, 0.15)
	door_mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	_materials.append(door_mat)

	# Index 4 = Ceiling: light grey.
	var ceiling_mat := StandardMaterial3D.new()
	ceiling_mat.albedo_color = Color(0.65, 0.60, 0.55)
	ceiling_mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	_materials.append(ceiling_mat)

	# Index 5 = Floor: medium grey.
	var floor_mat := StandardMaterial3D.new()
	floor_mat.albedo_color = Color(0.50, 0.45, 0.40)
	floor_mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	_materials.append(floor_mat)


func _build_rotations() -> void:
	# PosX: quad faces +X. Default quad faces -Z, so rotate 90° around Y.
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(90)))
	# NegX: quad faces -X. Rotate -90° around Y.
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(-90)))
	# PosY: quad faces +Y. Rotate -90° around X.
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(-90)))
	# NegY: quad faces -Y. Rotate 90° around X.
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(90)))
	# PosZ: quad faces +Z. Rotate 180° around Y.
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(180)))
	# NegZ: quad faces -Z. Identity (default quad orientation).
	_face_rotations.append(Basis.IDENTITY)


func refresh() -> void:
	var data := _bridge.get_building_face_data()
	if data == _last_face_data:
		return
	_last_face_data = data

	# Remove previous instances.  Use free() not queue_free() so the dying
	# nodes are removed from the tree immediately — avoids Godot silently
	# renaming new children that share a name with a queue_free'd sibling.
	for inst in _instances:
		inst.free()
	_instances.clear()
	_ceiling_instance = null
	var quintuple_count := data.size() / 5
	if quintuple_count == 0:
		return

	# Group faces by face_type (1-5). Each group gets its own MultiMesh.
	# face_groups[type] = array of {x, y, z, dir}
	var face_groups: Array[Array] = []
	for i in 6:
		face_groups.append([])

	for i in quintuple_count:
		var idx := i * 5
		var fx := data[idx]
		var fy := data[idx + 1]
		var fz := data[idx + 2]
		var fdir := data[idx + 3]
		var ftype := data[idx + 4]
		if ftype >= 1 and ftype <= 5:
			face_groups[ftype].append([fx, fy, fz, fdir])

	# Build a MultiMeshInstance3D per face type.
	for ftype in range(1, 6):
		var group: Array = face_groups[ftype]
		if group.size() == 0:
			continue

		var quad := QuadMesh.new()
		quad.size = Vector2(1.0, 1.0)
		quad.material = _materials[ftype]

		var mm := MultiMesh.new()
		mm.transform_format = MultiMesh.TRANSFORM_3D
		mm.mesh = quad
		mm.instance_count = group.size()

		for j in group.size():
			var entry: Array = group[j]
			var x := float(entry[0])
			var y := float(entry[1])
			var z := float(entry[2])
			var dir_idx: int = entry[3]

			var basis: Basis = _face_rotations[dir_idx]
			# Position: center of voxel + half-voxel offset toward the face.
			var center := Vector3(x + 0.5, y + 0.5, z + 0.5)
			var offset := DIRECTION_OFFSETS[dir_idx] * (0.5 - FACE_INSET)
			var xform := Transform3D(basis, center + offset)
			mm.set_instance_transform(j, xform)

		var instance := MultiMeshInstance3D.new()
		instance.multimesh = mm
		instance.name = "BuildingFaces_" + str(ftype)
		if ftype == 4:
			_ceiling_instance = instance
			instance.visible = not _roofs_hidden
		add_child(instance)
		_instances.append(instance)


func set_roofs_hidden(hidden: bool) -> void:
	_roofs_hidden = hidden
	_apply_roof_visibility()


func is_roofs_hidden() -> bool:
	return _roofs_hidden


func _apply_roof_visibility() -> void:
	if _ceiling_instance:
		_ceiling_instance.visible = not _roofs_hidden
