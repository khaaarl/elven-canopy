## Renders the tree's voxels using Rust-generated chunk meshes.
##
## Built at startup via setup(), then incrementally updated every frame via
## refresh() so that carved voxels disappear and new construction appears in
## real time. Uses a MegaChunk spatial hierarchy on the Rust side for
## draw-distance filtering and frustum culling — only nearby, visible chunks
## have MeshInstance3D nodes.
##
## Mesh generation is fully asynchronous: chunk meshes are generated on
## background rayon workers and completed results are drained each frame.
## This prevents frame stuttering at long draw distances. Chunks may take
## 1-2 frames to appear after entering visibility.
##
## The Rust sim generates per-chunk ArrayMesh data with up to three surfaces:
## - Surface 0 (bark): Trunk, Branch, Root, and construction voxels. Uses the
##   smooth mesh pipeline: each face is subdivided into 8 triangles, chamfered,
##   and iteratively smoothed. Procedural value noise shader modulates vertex
##   colors for organic surface variation. Bark uses anisotropic Y-scaling for
##   vertical grain lines. Per-vertex normals enable smooth shading.
## - Surface 1 (ground): Dirt voxels, same smooth mesh pipeline and noise
##   shader as bark but with isotropic noise (no grain direction).
## - Surface 2 (leaf): Leaf voxels, same smooth mesh pipeline as solid.
##   Shell-only (leaf↔leaf culled). Procedural noise shader with alpha
##   scissor for boolean transparency. Cull disabled (both sides visible).
##
## Empty surfaces are padded with degenerate placeholder triangles on the Rust
## side so surface indices are always stable (bark=0, ground=1, leaf=2).
##
## Each non-empty 16x16x16 chunk becomes one MeshInstance3D child with the
## chunk's ArrayMesh. Bark and ground use a procedural noise ShaderMaterial
## (`smooth_solid.gdshader`); the leaf surface uses a procedural noise
## ShaderMaterial (`leaf_noise.gdshader`) with alpha scissor.
##
## ## Visibility pipeline (per frame)
##
## 1. Extract camera frustum planes from the active Camera3D.
## 2. Send light direction + camera position + frustum to Rust.
## 3. Rust classifies each chunk as visible (in frustum), shadow-only
##    (outside frustum but inside the shadow caster volume extruded along
##    the light direction), or hidden. Returns delta lists.
## 4. GDScript toggles .visible and cast_shadow accordingly: visible chunks
##    use SHADOW_CASTING_SETTING_ON, shadow-only chunks use SHADOWS_ONLY,
##    hidden chunks have .visible = false.
##
## Fruit is rendered as billboarded Sprite3D nodes, one per fruit voxel,
## using procedural 16x16 pixel art textures from elven_canopy_sprites.
##
## See also: mesh_gen.rs and smooth_mesh.rs (sim crate) for the smooth mesh
## pipeline, mesh_cache.rs (gdext crate) for the MegaChunk hierarchy and LRU
## cache, sim_bridge.rs for the bridge API, smooth_solid.gdshader and
## leaf_noise.gdshader for the procedural noise shaders, main.gd which
## creates this node and calls setup() + refresh().

extends Node3D

var _bridge: SimBridge
## Reference to the active Camera3D for frustum extraction.
var _camera: Camera3D
## Reference to the main DirectionalLight3D for shadow-only culling.
var _sun: DirectionalLight3D
## Leaf material: procedural noise shader with boolean alpha discard.
var _leaf_material: ShaderMaterial
## Global procedural-noise material for bark surfaces (anisotropic grain).
var _bark_material: ShaderMaterial
## Global procedural-noise material for ground surfaces (isotropic).
var _ground_material: ShaderMaterial
## Map from chunk key ("cx,cy,cz") to MeshInstance3D for fast lookup.
var _chunk_instances: Dictionary = {}
## Fruit sprite texture cache: species_id (int) → ImageTexture.
var _fruit_textures: Dictionary = {}
## Container node for fruit billboard sprites.
var _fruit_container: Node3D
## Pool of reusable Sprite3D nodes for fruit rendering.
var _fruit_sprites: Array[Sprite3D] = []
## Last applied draw distance (voxels). Tracked to avoid redundant bridge calls.
var _current_draw_distance: int = -1


## Call after SimBridge is initialized to build the chunk meshes.
## camera: the active Camera3D used for frustum culling.
func setup(bridge: SimBridge, camera: Camera3D, sun: DirectionalLight3D) -> void:
	_bridge = bridge
	_camera = camera
	_sun = sun
	_leaf_material = _build_leaf_material()
	_fruit_container = Node3D.new()
	_fruit_container.name = "FruitSprites"
	add_child(_fruit_container)
	_bridge.build_world_mesh()
	_bark_material = _build_noise_material(0.3, 16.0)  # stretch Y, high freq
	_ground_material = _build_noise_material(1.0, 8.0)  # isotropic, medium freq
	_apply_draw_distance()
	_do_initial_visibility()
	_cache_fruit_textures()
	_refresh_fruit()


## Perform the first visibility pass to submit initial chunks for generation.
## Skips frustum culling (empty plane list) because the camera may not have
## a valid frustum yet during setup — the viewport hasn't rendered a frame.
## Chunks are submitted to background workers; they appear on subsequent frames.
func _do_initial_visibility() -> void:
	var cam_pos := _camera.global_position
	var empty_frustum := PackedFloat32Array()
	_bridge.update_visibility(cam_pos.x, cam_pos.y, cam_pos.z, empty_frustum)
	_process_generated_chunks()
	# Chunks are generated asynchronously. On the first frame, no results are
	# ready yet. Subsequent refresh() calls drain completed meshes.


## Read draw distance from GameConfig and apply to the bridge if changed.
func _apply_draw_distance() -> void:
	var dist: int = GameConfig.get_setting("draw_distance")
	if dist != _current_draw_distance:
		_current_draw_distance = dist
		_bridge.set_draw_distance(float(dist))


## Submit dirty chunks for background regeneration, update visibility, and
## refresh fruit. Called every frame by main.gd.
##
## Mesh generation is fully asynchronous: update_world_mesh() and
## update_visibility() submit chunks to background workers. Completed meshes
## are drained at the start of update_visibility() and appear in the
## chunks_generated delta list, which _process_generated_chunks() handles.
func refresh() -> void:
	_bridge.update_world_mesh()

	# Apply draw distance if changed (e.g. via settings panel).
	_apply_draw_distance()

	# Visibility update: send camera state to Rust, drain completed meshes,
	# process show/hide/shadow/generate/evict delta lists.
	_update_chunk_visibility()

	_refresh_fruit()


## Send light direction, camera frustum, and position to Rust and process the
## resulting show/hide/shadow/generate/evict delta lists.
func _update_chunk_visibility() -> void:
	var frustum := _extract_frustum_planes()
	var cam_pos := _camera.global_position

	# Pass the sun's forward direction (-Z in local space) to Rust for
	# shadow-only culling. The light direction points from the light toward
	# the scene.
	if _sun:
		var light_dir := -_sun.global_basis.z
		_bridge.set_light_direction(light_dir.x, light_dir.y, light_dir.z)

	_bridge.update_visibility(cam_pos.x, cam_pos.y, cam_pos.z, frustum)

	# Create MeshInstance3Ds for freshly generated chunks FIRST so that the
	# delta loops below can find them. Without this, a shadow-only chunk
	# generated this frame would be created with default cast_shadow=ON and
	# never corrected (since it's already in shadow_set next frame).
	_process_generated_chunks()

	# Hide chunks that left full visibility (visible → hidden).
	var to_hide := _bridge.get_chunks_to_hide()
	var hide_count := to_hide.size() / 3
	for i in hide_count:
		var idx := i * 3
		var key := "%d,%d,%d" % [to_hide[idx], to_hide[idx + 1], to_hide[idx + 2]]
		if _chunk_instances.has(key):
			var inst: MeshInstance3D = _chunk_instances[key]
			inst.visible = false

	# Transition chunks to shadow-only (hidden→shadow or visible→shadow).
	var to_shadow := _bridge.get_chunks_to_shadow()
	var shadow_count := to_shadow.size() / 3
	for i in shadow_count:
		var idx := i * 3
		var key := "%d,%d,%d" % [to_shadow[idx], to_shadow[idx + 1], to_shadow[idx + 2]]
		if _chunk_instances.has(key):
			var inst: MeshInstance3D = _chunk_instances[key]
			inst.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_SHADOWS_ONLY
			inst.visible = true

	# Transition chunks from shadow-only to hidden (shadow→hidden).
	var from_shadow := _bridge.get_chunks_from_shadow()
	var from_shadow_count := from_shadow.size() / 3
	for i in from_shadow_count:
		var idx := i * 3
		var key := "%d,%d,%d" % [from_shadow[idx], from_shadow[idx + 1], from_shadow[idx + 2]]
		if _chunk_instances.has(key):
			var inst: MeshInstance3D = _chunk_instances[key]
			inst.visible = false

	# Show chunks that entered full visibility (hidden→visible or shadow→visible).
	# Restore normal shadow casting for chunks that were shadow-only.
	var to_show := _bridge.get_chunks_to_show()
	var show_count := to_show.size() / 3
	for i in show_count:
		var idx := i * 3
		var key := "%d,%d,%d" % [to_show[idx], to_show[idx + 1], to_show[idx + 2]]
		if _chunk_instances.has(key):
			var inst: MeshInstance3D = _chunk_instances[key]
			inst.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_ON
			inst.visible = true

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


func _build_leaf_material() -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	var shader := load("res://shaders/leaf_noise.gdshader") as Shader
	mat.shader = shader
	mat.set_shader_parameter("noise_freq", 32.0)
	mat.set_shader_parameter("alpha_threshold", 0.35)
	return mat


## Build a ShaderMaterial with procedural noise for smooth solid surfaces.
## y_scale: Y-axis frequency multiplier (0.3 for bark grain, 1.0 for ground).
func _build_noise_material(y_scale_val: float, freq: float) -> ShaderMaterial:
	var mat := ShaderMaterial.new()
	var shader := load("res://shaders/smooth_solid.gdshader") as Shader
	mat.shader = shader
	mat.set_shader_parameter("y_scale", y_scale_val)
	mat.set_shader_parameter("noise_freq", freq)
	mat.set_shader_parameter("noise_strength", 0.4)
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

	# Surface 0 = bark: procedural noise with anisotropic grain.
	array_mesh.surface_set_material(0, _bark_material)

	# Surface 1 = ground: procedural noise, isotropic.
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


## Toggle directional face tinting on all chunk materials. When enabled,
## upward-facing surfaces are warmed and downward-facing surfaces are cooled
## (sky-dome ambient effect). When disabled, face_tint_strength is zeroed out.
func set_face_tint_enabled(enabled: bool) -> void:
	var strength := 0.5 if enabled else 0.0
	_bark_material.set_shader_parameter("face_tint_strength", strength)
	_ground_material.set_shader_parameter("face_tint_strength", strength)
	_leaf_material.set_shader_parameter("face_tint_strength", strength)
