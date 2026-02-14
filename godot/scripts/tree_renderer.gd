## Renders the tree's voxels using MultiMeshInstance3D.
##
## Reads trunk and branch voxel data from SimBridge at startup and creates
## two MultiMeshInstance3D children — one for trunk voxels (dark brown) and
## one for branch voxels (lighter brown). Each voxel is rendered as a unit
## cube.
##
## See also: sim_bridge.rs for the voxel data source, main.gd which creates
## this node and calls setup().

extends Node3D

var _trunk_mesh_instance: MultiMeshInstance3D
var _branch_mesh_instance: MultiMeshInstance3D


## Call after SimBridge is initialized to build the tree meshes.
func setup(bridge: SimBridge) -> void:
	# --- Trunk ---
	var trunk_voxels := bridge.get_trunk_voxels()
	var trunk_count := trunk_voxels.size() / 3
	if trunk_count > 0:
		_trunk_mesh_instance = _create_voxel_multimesh(
			trunk_voxels, trunk_count,
			Color(0.35, 0.22, 0.10)  # Dark brown
		)
		_trunk_mesh_instance.name = "TrunkMultiMesh"
		add_child(_trunk_mesh_instance)

	# --- Branches ---
	var branch_voxels := bridge.get_branch_voxels()
	var branch_count := branch_voxels.size() / 3
	if branch_count > 0:
		_branch_mesh_instance = _create_voxel_multimesh(
			branch_voxels, branch_count,
			Color(0.45, 0.30, 0.15)  # Lighter brown
		)
		_branch_mesh_instance.name = "BranchMultiMesh"
		add_child(_branch_mesh_instance)

	print("TreeRenderer: %d trunk voxels, %d branch voxels" % [trunk_count, branch_count])


func _create_voxel_multimesh(
	voxels: PackedInt32Array, count: int, color: Color
) -> MultiMeshInstance3D:
	var mesh := BoxMesh.new()
	mesh.size = Vector3(1.0, 1.0, 1.0)

	var mat := StandardMaterial3D.new()
	mat.albedo_color = color
	mesh.material = mat

	var multi_mesh := MultiMesh.new()
	multi_mesh.transform_format = MultiMesh.TRANSFORM_3D
	multi_mesh.mesh = mesh
	multi_mesh.instance_count = count

	for i in count:
		var idx := i * 3
		var x := float(voxels[idx])
		var y := float(voxels[idx + 1])
		var z := float(voxels[idx + 2])
		# Offset by 0.5 so the cube center aligns with voxel position.
		var xform := Transform3D(Basis.IDENTITY, Vector3(x + 0.5, y + 0.5, z + 0.5))
		multi_mesh.set_instance_transform(i, xform)

	var instance := MultiMeshInstance3D.new()
	instance.multimesh = multi_mesh
	return instance
