## Renders blueprint and construction voxels.
##
## Three visual layers:
## - **Ghost cubes** (translucent light-blue, no_depth_test): unplaced blueprint
##   voxels — the player's designated intent that hasn't been built yet.
## - **Ladder ghost panels** (translucent light-blue, thin oriented panels):
##   unbuilt ladder blueprint voxels, shown as thin panels on the correct face
##   rather than full cubes.
## - **Platform cubes** (solid brown): voxels that elves have already
##   materialized through construction work. Excludes BuildingInterior and
##   ladder voxels, which are rendered by building_renderer.gd and
##   ladder_renderer.gd respectively.
##
## Follows the same MultiMeshInstance3D pattern as tree_renderer.gd: reads
## voxel positions from SimBridge as flat PackedInt32Array of (x,y,z) triples
## and builds MultiMeshes with unit BoxMesh instances offset by +0.5 on all
## axes to center on the voxel coordinate.
##
## The ghost material uses no_depth_test=true so blueprints are visible
## through solid geometry. The platform material is opaque like tree voxels.
## All MultiMeshes are rebuilt on each refresh() call. main.gd calls
## refresh() every frame so materialized voxels appear as solid wood
## immediately.
##
## See also: sim_bridge.rs for get_blueprint_voxels(),
## get_platform_voxels(), and get_ladder_blueprint_data(),
## blueprint.rs for the Blueprint data model,
## construction_controller.gd which emits blueprint_placed, tree_renderer.gd
## for the MultiMesh pattern, ladder_renderer.gd for completed ladder
## rendering, main.gd which creates this node and calls refresh().

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
var _ghost_instance: MultiMeshInstance3D
var _platform_instance: MultiMeshInstance3D
var _ladder_ghost_instance: MultiMeshInstance3D
var _ghost_material: StandardMaterial3D
var _platform_material: StandardMaterial3D
var _face_rotations: Array[Basis] = []


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_build_materials()
	_build_rotations()
	refresh()


func _build_materials() -> void:
	# Ghost material: translucent blue for unplaced blueprint voxels.
	_ghost_material = StandardMaterial3D.new()
	_ghost_material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	_ghost_material.albedo_color = Color(0.3, 0.6, 1.0, 0.35)
	_ghost_material.no_depth_test = true
	_ghost_material.cull_mode = BaseMaterial3D.CULL_DISABLED

	# Platform material: solid brown for materialized construction voxels.
	_platform_material = StandardMaterial3D.new()
	_platform_material.albedo_color = Color(0.50, 0.35, 0.18)


func _build_rotations() -> void:
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(90)))  # PosX
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(-90)))  # NegX
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(-90)))  # PosY
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(90)))  # NegY
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(180)))  # PosZ
	_face_rotations.append(Basis.IDENTITY)  # NegZ


func refresh() -> void:
	_refresh_ghosts()
	_refresh_platforms()
	_refresh_ladder_ghosts()


func _refresh_ghosts() -> void:
	if _ghost_instance:
		_ghost_instance.queue_free()
		_ghost_instance = null

	var voxels := _bridge.get_blueprint_voxels()
	var count := voxels.size() / 3
	if count == 0:
		return

	_ghost_instance = _build_multimesh(voxels, count, _ghost_material, "BlueprintMultiMesh")
	add_child(_ghost_instance)


func _refresh_platforms() -> void:
	if _platform_instance:
		_platform_instance.queue_free()
		_platform_instance = null

	var voxels := _bridge.get_platform_voxels()
	var count := voxels.size() / 3
	if count == 0:
		return

	_platform_instance = _build_multimesh(voxels, count, _platform_material, "PlatformMultiMesh")
	add_child(_platform_instance)


func _build_multimesh(
	voxels: PackedInt32Array, count: int, material: StandardMaterial3D, node_name: String
) -> MultiMeshInstance3D:
	var box_mesh := BoxMesh.new()
	box_mesh.size = Vector3(1.0, 1.0, 1.0)
	box_mesh.material = material

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

	var instance := MultiMeshInstance3D.new()
	instance.multimesh = multi_mesh
	instance.name = node_name
	return instance


func _refresh_ladder_ghosts() -> void:
	if _ladder_ghost_instance:
		_ladder_ghost_instance.queue_free()
		_ladder_ghost_instance = null

	var data := _bridge.get_ladder_blueprint_data()
	var count := data.size() / 5
	if count == 0:
		return

	var box := BoxMesh.new()
	box.size = Vector3(0.9, 0.9, 0.05)
	box.material = _ghost_material

	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = box
	mm.instance_count = count

	for i in count:
		var idx := i * 5
		var x := float(data[idx])
		var y := float(data[idx + 1])
		var z := float(data[idx + 2])
		var dir_idx := data[idx + 3]

		var rot: Basis = _face_rotations[dir_idx]
		var center := Vector3(x + 0.5, y + 0.5, z + 0.5)
		var offset := DIRECTION_OFFSETS[dir_idx] * (0.5 - FACE_INSET)
		var xform := Transform3D(rot, center + offset)
		mm.set_instance_transform(i, xform)

	_ladder_ghost_instance = MultiMeshInstance3D.new()
	_ladder_ghost_instance.multimesh = mm
	_ladder_ghost_instance.name = "LadderGhostMultiMesh"
	add_child(_ladder_ghost_instance)
