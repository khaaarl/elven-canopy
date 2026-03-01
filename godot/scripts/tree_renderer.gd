## Renders the tree's voxels using MultiMeshInstance3D for batched drawing.
##
## Built at startup via setup(), then refreshed every frame via refresh() so
## that carved voxels disappear and new fruit appears in real time. Reads
## trunk, branch, root, leaf, fruit, and dirt voxel positions from SimBridge
## as flat PackedInt32Array (x,y,z triples) and creates six MultiMeshInstance3D
## children:
## - Trunk voxels: dark brown (0.35, 0.22, 0.10) — unit BoxMesh
## - Branch voxels: lighter brown (0.45, 0.30, 0.15) — unit BoxMesh
## - Root voxels: dark earthy brown (0.30, 0.20, 0.12) — unit BoxMesh
## - Leaf voxels: Minecraft-style cutout (alpha scissor) with a procedural
##   16x16 texture of opaque green patches and transparent holes — unit BoxMesh
## - Fruit voxels: warm amber/gold SphereMesh with subtle emissive glow,
##   hanging below leaf voxels
## - Dirt voxels: grassy green (0.25, 0.45, 0.20) — unit BoxMesh forming
##   hilly terrain above ForestFloor
##
## Each voxel is rendered as a unit BoxMesh (or SphereMesh for fruit).
## Positions are offset by +0.5 on all axes so the mesh centers on the
## voxel coordinate (voxel coords are integer corner positions, but meshes
## need to be centered).
##
## MultiMesh is used instead of individual MeshInstance3D nodes because it
## batches all instances into a single draw call per material, which is
## critical for performance with thousands of voxels.
##
## See also: sim_bridge.rs for get_trunk_voxels() / get_branch_voxels() /
## get_root_voxels() / get_leaf_voxels() / get_fruit_voxels() /
## get_dirt_voxels(), tree_gen.rs
## (Rust) for how the voxel geometry is generated via energy-based recursive
## segment growth, sim.rs for fruit spawning logic, main.gd which creates
## this node and calls setup() + refresh().

extends Node3D

var _bridge: SimBridge
var _trunk_mesh_instance: MultiMeshInstance3D
var _branch_mesh_instance: MultiMeshInstance3D
var _leaf_mesh_instance: MultiMeshInstance3D
var _root_mesh_instance: MultiMeshInstance3D
var _fruit_mesh_instance: MultiMeshInstance3D
var _dirt_mesh_instance: MultiMeshInstance3D
## Cached leaf texture — generated once, reused across refreshes.
var _leaf_texture: ImageTexture


## Call after SimBridge is initialized to build the tree meshes.
func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_leaf_texture = _generate_leaf_texture()
	refresh()


## Rebuild all tree MultiMesh instances from current voxel data.
## Called every frame by main.gd so carved voxels and new fruit are visible.
func refresh() -> void:
	_refresh_layer("trunk")
	_refresh_layer("branch")
	_refresh_layer("root")
	_refresh_layer("leaf")
	_refresh_layer("fruit")
	_refresh_layer("dirt")


func _refresh_layer(layer: String) -> void:
	# Free old instance.
	var old: MultiMeshInstance3D
	match layer:
		"trunk":
			old = _trunk_mesh_instance
		"branch":
			old = _branch_mesh_instance
		"root":
			old = _root_mesh_instance
		"leaf":
			old = _leaf_mesh_instance
		"fruit":
			old = _fruit_mesh_instance
		"dirt":
			old = _dirt_mesh_instance
	if old:
		old.queue_free()

	# Get current voxels from bridge.
	var voxels: PackedInt32Array
	match layer:
		"trunk":
			voxels = _bridge.get_trunk_voxels()
		"branch":
			voxels = _bridge.get_branch_voxels()
		"root":
			voxels = _bridge.get_root_voxels()
		"leaf":
			voxels = _bridge.get_leaf_voxels()
		"fruit":
			voxels = _bridge.get_fruit_voxels()
		"dirt":
			voxels = _bridge.get_dirt_voxels()

	var count := voxels.size() / 3
	if count == 0:
		match layer:
			"trunk":
				_trunk_mesh_instance = null
			"branch":
				_branch_mesh_instance = null
			"root":
				_root_mesh_instance = null
			"leaf":
				_leaf_mesh_instance = null
			"fruit":
				_fruit_mesh_instance = null
			"dirt":
				_dirt_mesh_instance = null
		return

	var instance: MultiMeshInstance3D
	match layer:
		"trunk":
			instance = _create_voxel_multimesh(voxels, count, Color(0.35, 0.22, 0.10))
			instance.name = "TrunkMultiMesh"
			_trunk_mesh_instance = instance
		"branch":
			instance = _create_voxel_multimesh(voxels, count, Color(0.45, 0.30, 0.15))
			instance.name = "BranchMultiMesh"
			_branch_mesh_instance = instance
		"root":
			instance = _create_voxel_multimesh(voxels, count, Color(0.30, 0.20, 0.12))
			instance.name = "RootMultiMesh"
			_root_mesh_instance = instance
		"leaf":
			instance = _create_leaf_multimesh(voxels, count)
			instance.name = "LeafMultiMesh"
			_leaf_mesh_instance = instance
		"fruit":
			instance = _create_fruit_multimesh(voxels, count)
			instance.name = "FruitMultiMesh"
			_fruit_mesh_instance = instance
		"dirt":
			instance = _create_voxel_multimesh(voxels, count, Color(0.25, 0.45, 0.20))
			instance.name = "DirtMultiMesh"
			_dirt_mesh_instance = instance

	add_child(instance)


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


func _create_leaf_multimesh(voxels: PackedInt32Array, count: int) -> MultiMeshInstance3D:
	var mesh := BoxMesh.new()
	mesh.size = Vector3(1.0, 1.0, 1.0)

	var mat := StandardMaterial3D.new()
	mat.albedo_color = Color(1.0, 1.0, 1.0, 1.0)
	mat.albedo_texture = _leaf_texture
	mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA_SCISSOR
	mat.alpha_scissor_threshold = 0.5
	mat.cull_mode = BaseMaterial3D.CULL_DISABLED  # Visible from inside too
	mat.texture_filter = BaseMaterial3D.TEXTURE_FILTER_NEAREST

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
		var xform := Transform3D(Basis.IDENTITY, Vector3(x + 0.5, y + 0.5, z + 0.5))
		multi_mesh.set_instance_transform(i, xform)

	var instance := MultiMeshInstance3D.new()
	instance.multimesh = multi_mesh
	return instance


func _create_fruit_multimesh(voxels: PackedInt32Array, count: int) -> MultiMeshInstance3D:
	var mesh := SphereMesh.new()
	mesh.radius = 0.4
	mesh.height = 0.8
	mesh.radial_segments = 8
	mesh.rings = 4

	var mat := StandardMaterial3D.new()
	mat.albedo_color = Color(0.95, 0.65, 0.15)  # Warm amber/gold
	mat.emission_enabled = true
	mat.emission = Color(0.6, 0.35, 0.05)  # Subtle warm glow
	mat.emission_energy_multiplier = 0.3
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
		var xform := Transform3D(Basis.IDENTITY, Vector3(x + 0.5, y + 0.5, z + 0.5))
		multi_mesh.set_instance_transform(i, xform)

	var instance := MultiMeshInstance3D.new()
	instance.multimesh = multi_mesh
	return instance


## Generate a Minecraft-style leaf texture: 16x16 with opaque green patches
## and fully transparent holes, giving an organic canopy look.
func _generate_leaf_texture() -> ImageTexture:
	var size := 16
	var img := Image.create(size, size, false, Image.FORMAT_RGBA8)
	img.fill(Color(0.0, 0.0, 0.0, 0.0))  # Start fully transparent

	# Several green shades for variation.
	var greens := [
		Color(0.18, 0.55, 0.15, 1.0),  # Base green
		Color(0.15, 0.48, 0.12, 1.0),  # Dark green
		Color(0.22, 0.62, 0.18, 1.0),  # Light green
		Color(0.20, 0.50, 0.14, 1.0),  # Mid green
	]

	# Fill ~60% of pixels with green, leaving ~40% transparent (holes).
	# Use a deterministic pattern based on pixel position.
	for y in range(size):
		for x in range(size):
			# Simple hash for deterministic pseudo-random pattern.
			var h := (x * 7 + y * 13 + x * y * 3) % 17
			if h < 10:  # ~60% fill rate
				var shade_idx := (x * 3 + y * 5) % greens.size()
				img.set_pixel(x, y, greens[shade_idx])

	return ImageTexture.create_from_image(img)
