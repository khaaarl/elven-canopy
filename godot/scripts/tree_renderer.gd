## Renders the tree's voxels using MultiMeshInstance3D for batched drawing.
##
## Built once at startup (static mesh — not updated per frame). Reads trunk,
## branch, root, leaf, and fruit voxel positions from SimBridge as flat
## PackedInt32Array (x,y,z triples) and creates five MultiMeshInstance3D
## children:
## - Trunk voxels: dark brown (0.35, 0.22, 0.10) — unit BoxMesh
## - Branch voxels: lighter brown (0.45, 0.30, 0.15) — unit BoxMesh
## - Root voxels: dark earthy brown (0.30, 0.20, 0.12) — unit BoxMesh
## - Leaf voxels: Minecraft-style cutout (alpha scissor) with a procedural
##   16x16 texture of opaque green patches and transparent holes — unit BoxMesh
## - Fruit voxels: warm amber/gold SphereMesh with subtle emissive glow,
##   hanging below leaf voxels
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
## get_root_voxels() / get_leaf_voxels() / get_fruit_voxels(), tree_gen.rs
## (Rust) for how the voxel geometry is generated via energy-based recursive
## segment growth, sim.rs for fruit spawning logic, main.gd which creates
## this node and calls setup().

extends Node3D

var _trunk_mesh_instance: MultiMeshInstance3D
var _branch_mesh_instance: MultiMeshInstance3D
var _leaf_mesh_instance: MultiMeshInstance3D
var _root_mesh_instance: MultiMeshInstance3D
var _fruit_mesh_instance: MultiMeshInstance3D


## Call after SimBridge is initialized to build the tree meshes.
func setup(bridge: SimBridge) -> void:
	# --- Trunk ---
	var trunk_voxels := bridge.get_trunk_voxels()
	var trunk_count := trunk_voxels.size() / 3
	if trunk_count > 0:
		_trunk_mesh_instance = _create_voxel_multimesh(
			trunk_voxels, trunk_count, Color(0.35, 0.22, 0.10)  # Dark brown
		)
		_trunk_mesh_instance.name = "TrunkMultiMesh"
		add_child(_trunk_mesh_instance)

	# --- Branches ---
	var branch_voxels := bridge.get_branch_voxels()
	var branch_count := branch_voxels.size() / 3
	if branch_count > 0:
		_branch_mesh_instance = _create_voxel_multimesh(
			branch_voxels, branch_count, Color(0.45, 0.30, 0.15)  # Lighter brown
		)
		_branch_mesh_instance.name = "BranchMultiMesh"
		add_child(_branch_mesh_instance)

	# --- Roots ---
	var root_voxels := bridge.get_root_voxels()
	var root_count := root_voxels.size() / 3
	if root_count > 0:
		_root_mesh_instance = _create_voxel_multimesh(
			root_voxels, root_count, Color(0.30, 0.20, 0.12)  # Dark earthy brown
		)
		_root_mesh_instance.name = "RootMultiMesh"
		add_child(_root_mesh_instance)

	# --- Leaves ---
	var leaf_voxels := bridge.get_leaf_voxels()
	var leaf_count := leaf_voxels.size() / 3
	if leaf_count > 0:
		_leaf_mesh_instance = _create_leaf_multimesh(leaf_voxels, leaf_count)
		_leaf_mesh_instance.name = "LeafMultiMesh"
		add_child(_leaf_mesh_instance)

	# --- Fruit ---
	var fruit_voxels := bridge.get_fruit_voxels()
	var fruit_count := fruit_voxels.size() / 3
	if fruit_count > 0:
		_fruit_mesh_instance = _create_fruit_multimesh(fruit_voxels, fruit_count)
		_fruit_mesh_instance.name = "FruitMultiMesh"
		add_child(_fruit_mesh_instance)

	print(
		(
			"TreeRenderer: %d trunk, %d branch, %d root, %d leaf, %d fruit voxels"
			% [trunk_count, branch_count, root_count, leaf_count, fruit_count]
		)
	)


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
	mat.albedo_texture = _generate_leaf_texture()
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
