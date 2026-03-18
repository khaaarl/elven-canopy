## Renders the tree's voxels using Rust-generated chunk meshes with face culling.
##
## Built at startup via setup(), then incrementally updated every frame via
## refresh() so that carved voxels disappear and new construction appears in
## real time. Uses a MegaChunk spatial hierarchy on the Rust side for
## draw-distance filtering and frustum culling — only nearby, visible chunks
## have MeshInstance3D nodes.
##
## The Rust sim generates per-chunk ArrayMesh data with up to three surfaces:
## - Surface 0 (bark): Trunk, Branch, Root, and construction voxels with
##   per-face culling. Textured via a custom tiling shader that samples three
##   global Texture2DArray caches at prime periods per axis. Bark caches use
##   anisotropic noise with domain warping for organic grain lines.
## - Surface 1 (ground): Dirt voxels, same shader but different tiling
##   textures with isotropic noise (no grain, no warping).
## - Surface 2 (leaf): Leaf voxels with alpha-scissor transparency, cull
##   disabled so leaves are visible from both sides.
##
## Empty surfaces are padded with degenerate placeholder triangles on the Rust
## side so surface indices are always stable (bark=0, ground=1, leaf=2).
##
## Each non-empty 16x16x16 chunk becomes one MeshInstance3D child with the
## chunk's ArrayMesh. Bark and ground each get a global ShaderMaterial with
## material-specific tiling textures; the leaf surface shares a single
## StandardMaterial3D.
##
## ## Visibility pipeline (per frame)
##
## 1. Extract camera frustum planes from the active Camera3D.
## 2. Send camera position + frustum to Rust via update_visibility().
## 3. Rust returns delta lists: chunks to show/hide/generate/evict.
## 4. GDScript toggles .visible, creates new MeshInstance3Ds for generated
##    chunks, and frees evicted ones.
##
## Fruit is rendered as billboarded Sprite3D nodes, one per fruit voxel,
## using procedural 16x16 pixel art textures from elven_canopy_sprites.
##
## See also: mesh_gen.rs (sim crate) for the face-culled mesh generation
## algorithm, texture_gen.rs for the prime-period tiling texture system,
## mesh_cache.rs (gdext crate) for the MegaChunk hierarchy and LRU cache,
## sim_bridge.rs for the bridge API, bark_ground.gdshader for the tiling
## shader, main.gd which creates this node and calls setup() + refresh().

extends Node3D

var _bridge: SimBridge
## Reference to the active Camera3D for frustum extraction.
var _camera: Camera3D
## Cached leaf texture — generated once, reused across refreshes.
var _leaf_texture: ImageTexture
## Leaf material: vertex color tinted alpha-scissor with procedural texture.
var _leaf_material: StandardMaterial3D
## Global tiling material for bark surfaces (anisotropic + domain-warped noise).
var _bark_material: ShaderMaterial
## Global tiling material for ground surfaces (isotropic noise).
var _ground_material: ShaderMaterial
## Map from chunk key ("cx,cy,cz") to MeshInstance3D for fast lookup.
var _chunk_instances: Dictionary = {}
## Fruit sprite texture cache: species_id (int) → ImageTexture.
var _fruit_textures: Dictionary = {}
## Container node for fruit billboard sprites.
var _fruit_container: Node3D
## Pool of reusable Sprite3D nodes for fruit rendering.
var _fruit_sprites: Array[Sprite3D] = []


## Call after SimBridge is initialized to build the chunk meshes.
## camera: the active Camera3D used for frustum culling.
func setup(bridge: SimBridge, camera: Camera3D) -> void:
	_bridge = bridge
	_camera = camera
	_leaf_texture = _generate_leaf_texture()
	_leaf_material = _build_leaf_material()
	_bark_material = _build_tiling_material(0)  # material 0 = bark
	_ground_material = _build_tiling_material(1)  # material 1 = ground
	_fruit_container = Node3D.new()
	_fruit_container.name = "FruitSprites"
	add_child(_fruit_container)
	_bridge.build_world_mesh()
	_do_initial_visibility()
	_cache_fruit_textures()
	_refresh_fruit()


## Perform the first visibility pass to populate initial chunks.
## Uses the camera's current state to determine what's visible.
func _do_initial_visibility() -> void:
	var frustum := _extract_frustum_planes()
	var cam_pos := _camera.global_position
	_bridge.update_visibility(cam_pos.x, cam_pos.y, cam_pos.z, frustum)
	_process_generated_chunks()


## Rebuild dirty chunks, update visibility, and refresh fruit.
## Called every frame by main.gd.
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

	# Visibility update: send camera state to Rust, process deltas.
	_update_chunk_visibility()

	_refresh_fruit()


## Send camera frustum + position to Rust and process the resulting
## show/hide/generate/evict delta lists.
func _update_chunk_visibility() -> void:
	var frustum := _extract_frustum_planes()
	var cam_pos := _camera.global_position
	_bridge.update_visibility(cam_pos.x, cam_pos.y, cam_pos.z, frustum)

	# Hide chunks that left visibility.
	var to_hide := _bridge.get_chunks_to_hide()
	var hide_count := to_hide.size() / 3
	for i in hide_count:
		var idx := i * 3
		var key := "%d,%d,%d" % [to_hide[idx], to_hide[idx + 1], to_hide[idx + 2]]
		if _chunk_instances.has(key):
			var inst: MeshInstance3D = _chunk_instances[key]
			inst.visible = false

	# Show chunks that entered visibility (already have MeshInstance3D).
	var to_show := _bridge.get_chunks_to_show()
	var show_count := to_show.size() / 3
	for i in show_count:
		var idx := i * 3
		var key := "%d,%d,%d" % [to_show[idx], to_show[idx + 1], to_show[idx + 2]]
		if _chunk_instances.has(key):
			var inst: MeshInstance3D = _chunk_instances[key]
			inst.visible = true

	# Create MeshInstance3Ds for freshly generated chunks.
	_process_generated_chunks()

	# Free evicted chunks.
	var evicted := _bridge.get_chunks_evicted()
	var evict_count := evicted.size() / 3
	for i in evict_count:
		var idx := i * 3
		var key := "%d,%d,%d" % [evicted[idx], evicted[idx + 1], evicted[idx + 2]]
		if _chunk_instances.has(key):
			var inst: MeshInstance3D = _chunk_instances[key]
			inst.queue_free()
			_chunk_instances.erase(key)


## Create MeshInstance3D nodes for chunks that were freshly generated by
## the Rust visibility pass.
func _process_generated_chunks() -> void:
	var generated := _bridge.get_chunks_generated()
	var gen_count := generated.size() / 3
	for i in gen_count:
		var idx := i * 3
		_rebuild_chunk(generated[idx], generated[idx + 1], generated[idx + 2])


func _build_leaf_material() -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.vertex_color_use_as_albedo = true
	mat.albedo_texture = _leaf_texture
	mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA_SCISSOR
	mat.alpha_scissor_threshold = 0.5
	mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	mat.texture_filter = BaseMaterial3D.TEXTURE_FILTER_NEAREST
	return mat


## Build a ShaderMaterial with the tiling shader for a specific material type.
## material_id: 0=bark (anisotropic+warped), 1=ground (isotropic).
func _build_tiling_material(material_id: int) -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	var shader := load("res://shaders/bark_ground.gdshader") as Shader
	mat.shader = shader

	# Upload the three tiling caches for this material type.
	var cache_names := ["a", "b", "c"]
	for ci in 3:
		var layer_count := _bridge.get_tiling_layer_count(ci)
		var periods := _bridge.get_tiling_periods(ci)
		var tpap := _bridge.get_tiling_tiles_per_axis_pair(ci)
		var data := _bridge.get_tiling_texture_data(material_id, ci)

		# Build Texture2DArray from flat R8 data.
		var images: Array[Image] = []
		var tile_bytes := 16 * 16  # TILE_SIZE^2
		for layer_idx in layer_count:
			var offset := layer_idx * tile_bytes
			var layer_data := data.slice(offset, offset + tile_bytes)
			var img := Image.create_from_data(16, 16, false, Image.FORMAT_R8, layer_data)
			images.append(img)

		var tex_array := Texture2DArray.new()
		tex_array.create_from_images(images)

		var suffix: String = cache_names[ci]
		mat.set_shader_parameter("cache_" + suffix, tex_array)
		mat.set_shader_parameter("periods_" + suffix, periods)
		mat.set_shader_parameter("tpap_" + suffix, tpap)

	return mat



## Build or rebuild the MeshInstance3D for a single chunk.
##
## The Rust side always emits exactly 3 surfaces in fixed order:
## 0 = bark, 1 = ground, 2 = leaf (empty surfaces get a degenerate
## placeholder triangle so the indices stay stable).
##
## Bark and ground each use their own global tiling material (different
## noise character). Leaf uses the shared leaf material.
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

	# Surface 0 = bark: anisotropic grain noise.
	array_mesh.surface_set_material(0, _bark_material)

	# Surface 1 = ground: isotropic noise.
	array_mesh.surface_set_material(1, _ground_material)

	# Surface 2 = leaf: shared leaf material.
	array_mesh.surface_set_material(2, _leaf_material)

	var instance := MeshInstance3D.new()
	instance.mesh = array_mesh
	instance.name = "Chunk_%s" % key
	add_child(instance)
	_chunk_instances[key] = instance


## Extract the 6 camera frustum planes as a flat PackedFloat32Array of
## 24 floats: [nx, ny, nz, d] × 6 (Godot convention).
func _extract_frustum_planes() -> PackedFloat32Array:
	var planes := _camera.get_frustum()
	var arr := PackedFloat32Array()
	arr.resize(24)
	for i in planes.size():
		var p: Plane = planes[i]
		arr[i * 4] = p.normal.x
		arr[i * 4 + 1] = p.normal.y
		arr[i * 4 + 2] = p.normal.z
		arr[i * 4 + 3] = p.d
	return arr



## Generate and cache fruit textures for all species in the world.
## Called once at setup; textures persist for the life of the scene.
func _cache_fruit_textures() -> void:
	var species_list: Array = _bridge.get_fruit_species_appearances()
	for entry in species_list:
		var dict: Dictionary = entry
		var sid: int = dict.get("id", -1)
		if sid < 0:
			continue
		var params := {
			"shape": dict.get("shape", "Round"),
			"color":
			Color(dict.get("color_r", 0.9), dict.get("color_g", 0.5), dict.get("color_b", 0.2)),
			"size_percent": dict.get("size_percent", 100),
			"glows": dict.get("glows", false),
		}
		_fruit_textures[sid] = SpriteGenerator.fruit_sprite_from_dict(params)


## Refresh fruit billboard sprites from current sim state.
## Uses a pool pattern: sprites are created on demand, never freed, only
## hidden when the fruit count decreases.
func _refresh_fruit() -> void:
	var voxels := _bridge.get_fruit_voxels()
	var count := voxels.size() / 4  # (x, y, z, species_id) quads

	# Ensure pool has enough sprites.
	while _fruit_sprites.size() < count:
		var sprite := Sprite3D.new()
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.pixel_size = 0.065
		sprite.texture_filter = BaseMaterial3D.TEXTURE_FILTER_NEAREST
		sprite.transparent = true
		_fruit_container.add_child(sprite)
		_fruit_sprites.append(sprite)

	# Update active sprites with current positions and textures.
	for i in count:
		var idx := i * 4
		var x := float(voxels[idx])
		var y := float(voxels[idx + 1])
		var z := float(voxels[idx + 2])
		var sid := voxels[idx + 3]
		var sprite := _fruit_sprites[i]
		sprite.position = Vector3(x + 0.5, y + 0.5, z + 0.5)
		if _fruit_textures.has(sid):
			sprite.texture = _fruit_textures[sid]
			sprite.visible = true
		else:
			sprite.visible = false

	# Hide excess sprites from pool.
	for i in range(count, _fruit_sprites.size()):
		_fruit_sprites[i].visible = false


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
