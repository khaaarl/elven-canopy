## Renders designated blueprint voxels as translucent light-blue cubes.
##
## Follows the same MultiMeshInstance3D pattern as tree_renderer.gd: reads
## voxel positions from SimBridge as a flat PackedInt32Array of (x,y,z)
## triples and builds a MultiMesh with one unit BoxMesh instance per voxel,
## offset by +0.5 on all axes to center on the voxel coordinate.
##
## The material is translucent light-blue with no_depth_test=true so
## blueprints are visible through solid geometry (important for blueprints
## inside or behind the tree). The MultiMesh is rebuilt on each refresh()
## call — not polled per frame — since blueprint changes are infrequent.
##
## See also: sim_bridge.rs for get_blueprint_voxels(), blueprint.rs for
## the Blueprint data model, construction_controller.gd which emits the
## blueprint_placed signal that triggers refresh(), tree_renderer.gd for
## the MultiMesh pattern this follows, main.gd which creates this node
## and wires the signal.

extends Node3D

var _bridge: SimBridge
var _mesh_instance: MultiMeshInstance3D
var _material: StandardMaterial3D


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_build_material()
	refresh()


func _build_material() -> void:
	_material = StandardMaterial3D.new()
	_material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	_material.albedo_color = Color(0.3, 0.6, 1.0, 0.35)
	_material.no_depth_test = true
	_material.cull_mode = BaseMaterial3D.CULL_DISABLED


func refresh() -> void:
	# Remove old mesh instance if it exists.
	if _mesh_instance:
		_mesh_instance.queue_free()
		_mesh_instance = null

	var voxels := _bridge.get_blueprint_voxels()
	var count := voxels.size() / 3
	if count == 0:
		return

	var box_mesh := BoxMesh.new()
	box_mesh.size = Vector3(1.0, 1.0, 1.0)
	box_mesh.material = _material

	var multi_mesh := MultiMesh.new()
	multi_mesh.transform_format = MultiMesh.TRANSFORM_3D
	multi_mesh.mesh = box_mesh
	multi_mesh.instance_count = count

	for i in count:
		var idx := i * 3
		var x := float(voxels[idx])
		var y := float(voxels[idx + 1])
		var z := float(voxels[idx + 2])
		var xform := Transform3D(Basis.IDENTITY, Vector3(x + 0.5, y + 0.5, z + 0.5))
		multi_mesh.set_instance_transform(i, xform)

	_mesh_instance = MultiMeshInstance3D.new()
	_mesh_instance.multimesh = multi_mesh
	_mesh_instance.name = "BlueprintMultiMesh"
	add_child(_mesh_instance)
