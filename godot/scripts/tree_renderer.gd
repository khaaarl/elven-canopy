## Renders the tree using a beveled ArrayMesh for wood and MultiMesh for foliage.
##
## Built once at startup (static mesh — not updated per frame). Wood voxels
## (trunk, branch, root) use a neighbor-aware beveled mesh generated in Rust
## (`tree_mesh.rs`): hidden faces between adjacent wood voxels are culled, and
## exposed faces are decomposed into a center quad plus 4 chamfered edge strips,
## giving a softer organic look. A procedural bark texture is applied to each
## wood type.
##
## Leaves and fruit continue to use MultiMeshInstance3D with their existing
## rendering approach (alpha-scissor leaf texture, emissive sphere fruit).
##
## Data flow for wood types:
##   1. Load `res://mesh_config.json` → pass to SimBridge.set_mesh_config_json()
##   2. Call SimBridge.generate_wood_meshes() to build beveled geometry in Rust
##   3. Retrieve flat packed arrays (vertices, normals, UVs, indices) per type
##   4. Build Godot ArrayMesh + StandardMaterial3D with bark ImageTexture
##
## Bevel amount, bark texture size, colors, and noise are all configurable via
## `godot/mesh_config.json` — edit and restart to see changes.
##
## See also: tree_mesh.rs (Rust) for the beveled mesh algorithm and bark texture
## generation, sim_bridge.rs for the GDExtension methods, tree_gen.rs for how
## voxel geometry is generated, main.gd which creates this node and calls
## setup().

extends Node3D

var _trunk_mesh_instance: MeshInstance3D
var _branch_mesh_instance: MeshInstance3D
var _root_mesh_instance: MeshInstance3D
var _leaf_mesh_instance: MultiMeshInstance3D
var _fruit_mesh_instance: MultiMeshInstance3D


## Call after SimBridge is initialized to build the tree meshes.
func setup(bridge: SimBridge) -> void:
	# --- Load mesh config ---
	var config_path := "res://mesh_config.json"
	if FileAccess.file_exists(config_path):
		var f := FileAccess.open(config_path, FileAccess.READ)
		var json_text := f.get_as_text()
		f.close()
		bridge.set_mesh_config_json(json_text)

	# --- Generate beveled wood meshes in Rust ---
	bridge.generate_wood_meshes()

	# --- Trunk ---
	var trunk_verts := bridge.get_trunk_mesh_vertices()
	if trunk_verts.size() > 0:
		_trunk_mesh_instance = _create_wood_mesh(
			trunk_verts,
			bridge.get_trunk_mesh_normals(),
			bridge.get_trunk_mesh_uvs(),
			bridge.get_trunk_mesh_indices(),
			bridge.get_bark_texture("trunk")
		)
		_trunk_mesh_instance.name = "TrunkMesh"
		add_child(_trunk_mesh_instance)

	# --- Branches ---
	var branch_verts := bridge.get_branch_mesh_vertices()
	if branch_verts.size() > 0:
		_branch_mesh_instance = _create_wood_mesh(
			branch_verts,
			bridge.get_branch_mesh_normals(),
			bridge.get_branch_mesh_uvs(),
			bridge.get_branch_mesh_indices(),
			bridge.get_bark_texture("branch")
		)
		_branch_mesh_instance.name = "BranchMesh"
		add_child(_branch_mesh_instance)

	# --- Roots ---
	var root_verts := bridge.get_root_mesh_vertices()
	if root_verts.size() > 0:
		_root_mesh_instance = _create_wood_mesh(
			root_verts,
			bridge.get_root_mesh_normals(),
			bridge.get_root_mesh_uvs(),
			bridge.get_root_mesh_indices(),
			bridge.get_bark_texture("root")
		)
		_root_mesh_instance.name = "RootMesh"
		add_child(_root_mesh_instance)

	# --- Leaves (unchanged — MultiMesh) ---
	var leaf_voxels := bridge.get_leaf_voxels()
	var leaf_count := leaf_voxels.size() / 3
	if leaf_count > 0:
		_leaf_mesh_instance = _create_leaf_multimesh(leaf_voxels, leaf_count)
		_leaf_mesh_instance.name = "LeafMultiMesh"
		add_child(_leaf_mesh_instance)

	# --- Fruit (unchanged — MultiMesh) ---
	var fruit_voxels := bridge.get_fruit_voxels()
	var fruit_count := fruit_voxels.size() / 3
	if fruit_count > 0:
		_fruit_mesh_instance = _create_fruit_multimesh(fruit_voxels, fruit_count)
		_fruit_mesh_instance.name = "FruitMultiMesh"
		add_child(_fruit_mesh_instance)

	print("TreeRenderer: trunk=%d verts, branch=%d verts, root=%d verts, %d leaf, %d fruit voxels" % [
		trunk_verts.size() / 3,
		branch_verts.size() / 3,
		root_verts.size() / 3,
		leaf_count,
		fruit_count
	])


## Build a MeshInstance3D from flat packed arrays returned by SimBridge.
## bark_data is a PackedByteArray: [width_u32_le, height_u32_le, rgba_pixels...].
func _create_wood_mesh(
	verts: PackedFloat32Array,
	norms: PackedFloat32Array,
	uvs: PackedFloat32Array,
	indices: PackedInt32Array,
	bark_data: PackedByteArray
) -> MeshInstance3D:
	# Convert flat float arrays to packed vector arrays.
	var vert_count := verts.size() / 3
	var vert_arr := PackedVector3Array()
	vert_arr.resize(vert_count)
	for i in vert_count:
		var idx := i * 3
		vert_arr[i] = Vector3(verts[idx], verts[idx + 1], verts[idx + 2])

	var norm_arr := PackedVector3Array()
	norm_arr.resize(vert_count)
	for i in vert_count:
		var idx := i * 3
		norm_arr[i] = Vector3(norms[idx], norms[idx + 1], norms[idx + 2])

	var uv_count := uvs.size() / 2
	var uv_arr := PackedVector2Array()
	uv_arr.resize(uv_count)
	for i in uv_count:
		var idx := i * 2
		uv_arr[i] = Vector2(uvs[idx], uvs[idx + 1])

	# Build ArrayMesh surface.
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = vert_arr
	arrays[Mesh.ARRAY_NORMAL] = norm_arr
	arrays[Mesh.ARRAY_TEX_UV] = uv_arr
	arrays[Mesh.ARRAY_INDEX] = indices

	var arr_mesh := ArrayMesh.new()
	arr_mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)

	# Decode bark texture from packed bytes.
	var tex_width := bark_data.decode_u32(0)
	var tex_height := bark_data.decode_u32(4)
	var pixel_data := bark_data.slice(8)
	var img := Image.create_from_data(tex_width, tex_height, false, Image.FORMAT_RGBA8, pixel_data)
	var tex := ImageTexture.create_from_image(img)

	# Material with bark texture.
	var mat := StandardMaterial3D.new()
	mat.albedo_texture = tex
	mat.texture_filter = BaseMaterial3D.TEXTURE_FILTER_LINEAR_WITH_MIPMAPS
	arr_mesh.surface_set_material(0, mat)

	var instance := MeshInstance3D.new()
	instance.mesh = arr_mesh
	return instance


func _create_leaf_multimesh(
	voxels: PackedInt32Array, count: int
) -> MultiMeshInstance3D:
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


func _create_fruit_multimesh(
	voxels: PackedInt32Array, count: int
) -> MultiMeshInstance3D:
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
