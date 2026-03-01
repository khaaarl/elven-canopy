## Renders completed ladder voxels as oriented thin panels.
##
## Ladders are non-solid voxels with a directional panel on one face (the
## "ladder face"). Two types: wood ladders (warm brown) and rope ladders
## (tan/beige). Each ladder voxel is rendered as a thin BoxMesh
## (0.9 x 0.9 x 0.05) oriented to face the correct direction, using the
## same rotation table as building_renderer.gd.
##
## Uses one MultiMeshInstance3D per ladder kind for batched drawing. Rebuilt
## every frame from get_ladder_data() which returns flat quintuples
## (x, y, z, face_direction, kind).
##
## Face direction encoding: 0=PosX, 1=NegX, 2=PosY, 3=NegY, 4=PosZ, 5=NegZ.
## Kind encoding: 0=Wood, 1=Rope.
##
## See also: sim_bridge.rs for get_ladder_data(), types.rs for LadderKind,
## building_renderer.gd for the per-face rotation table pattern,
## blueprint_renderer.gd for ladder ghost rendering, main.gd which creates
## this node and calls refresh().

extends Node3D

const FACE_INSET := 0.005

## Offset vectors indexed by face direction (0=PosX..5=NegZ).
const DIRECTION_OFFSETS: Array[Vector3] = [
	Vector3.RIGHT,  # 0 PosX
	Vector3.LEFT,  # 1 NegX
	Vector3.UP,  # 2 PosY
	Vector3.DOWN,  # 3 NegY
	Vector3.BACK,  # 4 PosZ
	Vector3.FORWARD,  # 5 NegZ
]

var _bridge: SimBridge
var _instances: Array[MultiMeshInstance3D] = []
var _materials: Array[StandardMaterial3D] = []
var _face_rotations: Array[Basis] = []


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_build_materials()
	_build_rotations()
	refresh()


func _build_materials() -> void:
	# Index 0 = Wood: warm brown.
	var wood_mat := StandardMaterial3D.new()
	wood_mat.albedo_color = Color(0.55, 0.35, 0.18)
	wood_mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	_materials.append(wood_mat)

	# Index 1 = Rope: tan/beige.
	var rope_mat := StandardMaterial3D.new()
	rope_mat.albedo_color = Color(0.72, 0.62, 0.42)
	rope_mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	_materials.append(rope_mat)


func _build_rotations() -> void:
	# Same rotation table as building_renderer.gd.
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(90)))  # PosX
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(-90)))  # NegX
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(-90)))  # PosY
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(90)))  # NegY
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(180)))  # PosZ
	_face_rotations.append(Basis.IDENTITY)  # NegZ


func refresh() -> void:
	for inst in _instances:
		inst.queue_free()
	_instances.clear()

	var data := _bridge.get_ladder_data()
	var count := data.size() / 5
	if count == 0:
		return

	# Group by kind (0=Wood, 1=Rope).
	var groups: Array[Array] = [[], []]

	for i in count:
		var idx := i * 5
		var lx := data[idx]
		var ly := data[idx + 1]
		var lz := data[idx + 2]
		var ldir := data[idx + 3]
		var lkind := data[idx + 4]
		if lkind >= 0 and lkind <= 1:
			groups[lkind].append([lx, ly, lz, ldir])

	for kind_idx in 2:
		var group: Array = groups[kind_idx]
		if group.size() == 0:
			continue

		var box := BoxMesh.new()
		box.size = Vector3(0.9, 0.9, 0.05)
		box.material = _materials[kind_idx]

		var mm := MultiMesh.new()
		mm.transform_format = MultiMesh.TRANSFORM_3D
		mm.mesh = box
		mm.instance_count = group.size()

		for j in group.size():
			var entry: Array = group[j]
			var x := float(entry[0])
			var y := float(entry[1])
			var z := float(entry[2])
			var dir_idx: int = entry[3]

			var rot: Basis = _face_rotations[dir_idx]
			var center := Vector3(x + 0.5, y + 0.5, z + 0.5)
			var offset := DIRECTION_OFFSETS[dir_idx] * (0.5 - FACE_INSET)
			var xform := Transform3D(rot, center + offset)
			mm.set_instance_transform(j, xform)

		var instance := MultiMeshInstance3D.new()
		instance.multimesh = mm
		var kind_name := "Wood" if kind_idx == 0 else "Rope"
		instance.name = "Ladder_" + kind_name
		add_child(instance)
		_instances.append(instance)
