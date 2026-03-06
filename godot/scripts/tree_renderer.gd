## Renders the tree's voxels using Rust-generated chunk meshes with face culling.
##
## Built at startup via setup(), then incrementally updated every frame via
## refresh() so that carved voxels disappear and new construction appears in
## real time. The Rust sim generates per-chunk ArrayMesh data with two surfaces:
## - Surface 0 (opaque): Trunk, Branch, Root, Dirt, and construction voxels
##   with per-face culling — only faces adjacent to non-opaque voxels are
##   rendered. Uses vertex colors as albedo.
## - Surface 1 (leaf): Leaf voxels with alpha-scissor transparency, cull
##   disabled so leaves are visible from both sides.
##
## Each non-empty 16x16x16 chunk becomes one MeshInstance3D child with the
## chunk's ArrayMesh. Dirty chunks (modified since last frame) are rebuilt
## incrementally — only the affected chunks are re-meshed.
##
## Fruit is kept as a separate MultiMeshInstance3D with SphereMesh (different
## geometry and emissive material, not part of the chunk mesh system).
##
## See also: mesh_gen.rs (sim crate) for the face-culled mesh generation
## algorithm, mesh_cache.rs (gdext crate) for the chunk caching layer,
## sim_bridge.rs for build_world_mesh()/update_world_mesh()/build_chunk_array_mesh(),
## main.gd which creates this node and calls setup() + refresh().

extends Node3D

var _bridge: SimBridge
var _fruit_mesh_instance: MultiMeshInstance3D
## Cached leaf texture — generated once, reused across refreshes.
var _leaf_texture: ImageTexture
## Cached opaque atlas texture (bark on top, grass on bottom).
var _opaque_texture: ImageTexture
## Opaque material: vertex color tinted with bark/grass atlas texture.
var _opaque_material: StandardMaterial3D
## Leaf material: vertex color tinted alpha-scissor with procedural texture.
var _leaf_material: StandardMaterial3D
## Map from chunk key ("cx,cy,cz") to MeshInstance3D for fast lookup.
var _chunk_instances: Dictionary = {}


## Call after SimBridge is initialized to build the chunk meshes.
func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_leaf_texture = _generate_leaf_texture()
	_opaque_texture = _generate_opaque_atlas()
	_build_materials()
	_bridge.build_world_mesh()
	_build_all_chunks()
	_refresh_fruit()


## Rebuild dirty chunks and refresh fruit. Called every frame by main.gd.
func refresh() -> void:
	var updated := _bridge.update_world_mesh()
	if updated > 0:
		var dirty := _bridge.get_dirty_chunk_coords()
		var count := dirty.size() / 3
		for i in count:
			var idx := i * 3
			var cx := dirty[idx]
			var cy := dirty[idx + 1]
			var cz := dirty[idx + 2]
			_rebuild_chunk(cx, cy, cz)
	_refresh_fruit()


func _build_materials() -> void:
	# Opaque material: vertex color × bark/grass atlas texture.
	_opaque_material = StandardMaterial3D.new()
	_opaque_material.vertex_color_use_as_albedo = true
	_opaque_material.albedo_texture = _opaque_texture
	_opaque_material.texture_filter = BaseMaterial3D.TEXTURE_FILTER_NEAREST

	# Leaf material: vertex color + alpha scissor texture, cull disabled.
	_leaf_material = StandardMaterial3D.new()
	_leaf_material.vertex_color_use_as_albedo = true
	_leaf_material.albedo_texture = _leaf_texture
	_leaf_material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA_SCISSOR
	_leaf_material.alpha_scissor_threshold = 0.5
	_leaf_material.cull_mode = BaseMaterial3D.CULL_DISABLED
	_leaf_material.texture_filter = BaseMaterial3D.TEXTURE_FILTER_NEAREST


## Build MeshInstance3D nodes for all non-empty chunks from the initial
## world mesh build.
func _build_all_chunks() -> void:
	var coords := _bridge.get_mesh_chunk_coords()
	var count := coords.size() / 3
	for i in count:
		var idx := i * 3
		var cx := coords[idx]
		var cy := coords[idx + 1]
		var cz := coords[idx + 2]
		_rebuild_chunk(cx, cy, cz)


## Build or rebuild the MeshInstance3D for a single chunk.
func _rebuild_chunk(cx: int, cy: int, cz: int) -> void:
	var key := "%d,%d,%d" % [cx, cy, cz]

	# Remove old instance if it exists.
	if _chunk_instances.has(key):
		var old: MeshInstance3D = _chunk_instances[key]
		old.queue_free()
		_chunk_instances.erase(key)

	var array_mesh: ArrayMesh = _bridge.build_chunk_array_mesh(cx, cy, cz)
	if array_mesh.get_surface_count() == 0:
		return

	# Assign materials to surfaces.
	var surface_count := array_mesh.get_surface_count()
	if surface_count >= 1:
		array_mesh.surface_set_material(0, _opaque_material)
	if surface_count >= 2:
		array_mesh.surface_set_material(1, _leaf_material)

	var instance := MeshInstance3D.new()
	instance.mesh = array_mesh
	instance.name = "Chunk_%s" % key
	add_child(instance)
	_chunk_instances[key] = instance


func _refresh_fruit() -> void:
	if _fruit_mesh_instance:
		_fruit_mesh_instance.queue_free()
		_fruit_mesh_instance = null

	var voxels := _bridge.get_fruit_voxels()
	var count := voxels.size() / 3
	if count == 0:
		return

	_fruit_mesh_instance = _create_fruit_multimesh(voxels, count)
	add_child(_fruit_mesh_instance)


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
	instance.name = "FruitMultiMesh"
	return instance


## Generate the opaque voxel texture atlas: 16x32 image with bark (top 16x16)
## and grass/dirt (bottom 16x16). Vertex colors provide the base hue; these
## textures add surface detail via multiplication. Centered around bright values
## so they brighten highlights and darken crevices without washing out.
func _generate_opaque_atlas() -> ImageTexture:
	var W := 16
	var H := 32  # Two 16x16 tiles stacked vertically
	var img := Image.create(W, H, false, Image.FORMAT_RGBA8)

	# --- Top half: bark texture (rows 0..15) ---
	# Vertical grain lines with knot-like darker patches.
	for y in range(W):
		for x in range(W):
			# Vertical grain: base brightness varies by column
			var grain := 0.75 + 0.15 * sin(float(x) * 2.3 + float(y) * 0.3)
			# Horizontal wobble for organic feel
			var wobble := 0.05 * sin(float(y) * 1.7 + float(x) * 0.8)
			# Occasional dark knots
			var knot_h := (x * 7 + y * 13) % 23
			var knot := 0.0
			if knot_h < 3:
				knot = -0.15
			var val := clampf(grain + wobble + knot, 0.5, 1.0)
			# Slight warm tint (bark is yellowish-brown in detail)
			img.set_pixel(x, y, Color(val * 1.05, val * 0.95, val * 0.85, 1.0))

	# --- Bottom half: grass/dirt texture (rows 16..31) ---
	# Clumpy grass pattern with earthy patches.
	for y in range(W):
		for x in range(W):
			var py := y + W  # actual pixel row in the atlas
			# Base brightness with variation
			var base := 0.8 + 0.1 * sin(float(x) * 3.1 + float(y) * 2.7)
			# Clumpy patches using a simple hash
			var clump_h := (x * 11 + y * 7 + x * y * 3) % 19
			var clump := 0.0
			if clump_h < 5:
				clump = 0.1  # lighter grass tufts
			elif clump_h > 15:
				clump = -0.12  # darker dirt patches
			var val := clampf(base + clump, 0.55, 1.0)
			# Slight green tint for grass detail
			img.set_pixel(x, py, Color(val * 0.9, val * 1.05, val * 0.85, 1.0))

	return ImageTexture.create_from_image(img)


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
