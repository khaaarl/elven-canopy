## Renders selection circles at the feet of selected creatures.
##
## Draws flat ring meshes on the ground beneath each selected creature,
## colored by faction: blue for player civ, yellow for neutral, red for
## hostile. Faction is determined by the sim's diplomatic relation system
## via bridge.get_creature_player_relation(), not by species name. Rings
## use no_depth_test so they show through terrain and tree trunks, letting
## the player see selected creatures on the other side of obstacles.
## Creature sprites use a higher render_priority so they draw on top of
## the circles.
##
## Uses a pool pattern: MeshInstance3D nodes are created on demand and
## reused. Two pools exist — one for 1x1 creatures and one for 2x2
## creatures (elephant, troll) which need larger rings. Each frame,
## main.gd passes the selected creature IDs and render tick; this script
## fetches positions and species via the bridge to place and color rings.
##
## See also: selection_controller.gd for selection state,
## creature_renderer.gd for sprite rendering (render_priority coordination),
## main.gd for wiring and per-frame updates.

extends Node3D

## Faction colors for selection rings.
const COLOR_PLAYER := Color(0.3, 0.5, 1.0, 0.7)
const COLOR_NEUTRAL := Color(1.0, 0.85, 0.2, 0.7)
const COLOR_HOSTILE := Color(1.0, 0.2, 0.2, 0.7)

## Species with 2x2 footprints that need larger rings.
const LARGE_SPECIES = ["Elephant", "Troll"]

## Ring texture size in pixels.
const TEX_SIZE := 64

## Ring geometry: outer and inner radius as fraction of texture size.
const RING_OUTER := 0.48
const RING_INNER := 0.38

var _bridge: SimBridge
var _render_tick: float = 0.0

## Pool of MeshInstance3D for 1x1 creatures.
var _small_pool: Array[MeshInstance3D] = []
## Pool of MeshInstance3D for 2x2 creatures.
var _large_pool: Array[MeshInstance3D] = []

## Shared meshes (one per size).
var _small_mesh: PlaneMesh
var _large_mesh: PlaneMesh

## Cached ring textures per color.
var _ring_textures: Dictionary = {}


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_small_mesh = _create_plane_mesh(1.0)
	_large_mesh = _create_plane_mesh(2.0)


func set_render_tick(tick: float) -> void:
	_render_tick = tick


## Update highlights for the given selected creature IDs.
## Called each frame by main.gd.
func update_highlights(selected_ids: Array) -> void:
	if not _bridge or not _bridge.is_initialized():
		_hide_all()
		return

	var small_idx := 0
	var large_idx := 0

	for cid in selected_ids:
		var info: Dictionary = _bridge.get_creature_info_by_id(cid, _render_tick)
		if info.is_empty():
			continue

		var species: String = info.get("species", "")
		var x: float = info.get("x", 0.0)
		var y: float = info.get("y", 0.0)
		var z: float = info.get("z", 0.0)
		var color := _relation_color(cid)
		var is_large := species in LARGE_SPECIES

		if is_large:
			var inst := _get_large(large_idx)
			# Large creatures anchor at min corner; center the ring on the 2x2 footprint.
			inst.global_position = Vector3(x + 1.0, y + 0.02, z + 1.0)
			_apply_color(inst, color)
			inst.visible = true
			large_idx += 1
		else:
			var inst := _get_small(small_idx)
			inst.global_position = Vector3(x + 0.5, y + 0.02, z + 0.5)
			_apply_color(inst, color)
			inst.visible = true
			small_idx += 1

	# Hide unused pool entries.
	for i in range(small_idx, _small_pool.size()):
		_small_pool[i].visible = false
	for i in range(large_idx, _large_pool.size()):
		_large_pool[i].visible = false


func _hide_all() -> void:
	for inst in _small_pool:
		inst.visible = false
	for inst in _large_pool:
		inst.visible = false


func _get_small(idx: int) -> MeshInstance3D:
	while _small_pool.size() <= idx:
		var inst := _create_ring_instance(_small_mesh)
		add_child(inst)
		_small_pool.append(inst)
	return _small_pool[idx]


func _get_large(idx: int) -> MeshInstance3D:
	while _large_pool.size() <= idx:
		var inst := _create_ring_instance(_large_mesh)
		add_child(inst)
		_large_pool.append(inst)
	return _large_pool[idx]


func _create_ring_instance(mesh: PlaneMesh) -> MeshInstance3D:
	var inst := MeshInstance3D.new()
	inst.mesh = mesh
	inst.visible = false
	# Material is set per-frame via _apply_color.
	return inst


func _apply_color(inst: MeshInstance3D, color: Color) -> void:
	var tex := _get_ring_texture(color)
	# Reuse material if color matches, otherwise create new.
	var mat: StandardMaterial3D = inst.get_surface_override_material(0)
	if mat and mat.get_meta("ring_color", Color.BLACK) == color:
		return
	mat = StandardMaterial3D.new()
	mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	mat.albedo_texture = tex
	mat.no_depth_test = true
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.render_priority = -1
	mat.set_meta("ring_color", color)
	inst.set_surface_override_material(0, mat)


func _get_ring_texture(color: Color) -> ImageTexture:
	var key := color.to_html()
	if _ring_textures.has(key):
		return _ring_textures[key]
	var tex := _generate_ring_texture(color)
	_ring_textures[key] = tex
	return tex


func _generate_ring_texture(color: Color) -> ImageTexture:
	var img := Image.create(TEX_SIZE, TEX_SIZE, false, Image.FORMAT_RGBA8)
	var center := Vector2(TEX_SIZE * 0.5, TEX_SIZE * 0.5)
	var outer_r := TEX_SIZE * RING_OUTER
	var inner_r := TEX_SIZE * RING_INNER
	# Anti-aliasing band width in pixels.
	var aa := 1.5

	for py in TEX_SIZE:
		for px in TEX_SIZE:
			var dist_sq := (px - center.x) ** 2 + (py - center.y) ** 2
			var dist := sqrt(dist_sq)
			var alpha := 0.0
			if dist >= inner_r and dist <= outer_r:
				alpha = color.a
				# Smooth outer edge.
				if dist > outer_r - aa:
					alpha *= (outer_r - dist) / aa
				# Smooth inner edge.
				if dist < inner_r + aa:
					alpha *= (dist - inner_r) / aa
			img.set_pixel(px, py, Color(color.r, color.g, color.b, alpha))

	return ImageTexture.create_from_image(img)


func _create_plane_mesh(diameter: float) -> PlaneMesh:
	var mesh := PlaneMesh.new()
	mesh.size = Vector2(diameter, diameter)
	return mesh


## Map a creature's diplomatic relation to the player into a ring color.
## Queries the bridge for the sim-authoritative relation rather than
## hardcoding species names.
func _relation_color(creature_id: String) -> Color:
	var relation: String = _bridge.get_creature_player_relation(creature_id)
	if relation == "friendly":
		return COLOR_PLAYER
	if relation == "hostile":
		return COLOR_HOSTILE
	return COLOR_NEUTRAL
