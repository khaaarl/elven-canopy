## Placement controller for click-to-place creature spawning and task creation.
##
## State machine (IDLE / PLACING) that handles the placement flow:
## 1. Toolbar emits spawn_requested or action_requested → enter PLACING mode
## 2. Each frame: find the nav node closest to the mouse ray in 3D, show
##    wireframe highlight cube at that position
## 3. Left-click: confirm action via SimBridge, exit placement mode
## 4. Right-click or Escape: cancel, exit placement mode
##
## Supports two placement types:
## - Species spawn (any species): places a creature at the clicked nav node.
##   Ground-only species are restricted to ground nodes via bridge query.
## - Actions (Summon): creates a GoTo task at the clicked nav node
##
## Uses _unhandled_input() and set_input_as_handled() so placement clicks don't
## propagate to the camera (which also uses _unhandled_input). Exposes
## is_placing() so selection_controller.gd can skip creature selection while
## a placement action is in progress, and cancel_placement() so
## construction_controller.gd can exit placement mode when entering
## construction mode.
##
## See also: action_toolbar.gd which triggers placement mode, main.gd which
## wires the two together, selection_controller.gd which checks is_placing(),
## construction_controller.gd which calls cancel_placement(),
## sim_bridge.rs for get_visible_nav_nodes/get_visible_ground_nav_nodes
## (voxel-based occlusion filtering).

extends Node3D

enum State { IDLE, PLACING }

## Maximum perpendicular distance (in world units) from the mouse ray to a nav
## node for it to be considered a snap candidate.
const SNAP_THRESHOLD := 5.0

var _state: State = State.IDLE
var _species_name: String = ""
var _action_name: String = ""
var _valid_positions: PackedVector3Array
var _snapped_position: Vector3
var _has_snap: bool = false

var _bridge: SimBridge
var _camera: Camera3D
var _highlight: MeshInstance3D


func setup(bridge: SimBridge, camera: Camera3D) -> void:
	_bridge = bridge
	_camera = camera


func connect_toolbar(toolbar: Node) -> void:
	toolbar.spawn_requested.connect(_on_spawn_requested)
	toolbar.action_requested.connect(_on_action_requested)


func is_placing() -> bool:
	return _state == State.PLACING


func cancel_placement() -> void:
	if _state == State.PLACING:
		_exit_placement()


func _ready() -> void:
	_highlight = MeshInstance3D.new()
	_highlight.visible = false
	add_child(_highlight)
	_build_highlight_mesh()


func _build_highlight_mesh() -> void:
	var mesh := ImmediateMesh.new()
	_highlight.mesh = mesh

	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.albedo_color = Color(0.2, 1.0, 0.2, 1.0)
	mat.no_depth_test = true
	mat.render_priority = 1
	_highlight.material_override = mat


func _draw_wireframe_cube(mesh: ImmediateMesh) -> void:
	mesh.clear_surfaces()
	mesh.surface_begin(Mesh.PRIMITIVE_LINES)

	# 12 edges of a unit cube centered at origin.
	var half := 0.5
	var corners = [
		Vector3(-half, 0.0, -half),
		Vector3(half, 0.0, -half),
		Vector3(half, 0.0, half),
		Vector3(-half, 0.0, half),
		Vector3(-half, 1.0, -half),
		Vector3(half, 1.0, -half),
		Vector3(half, 1.0, half),
		Vector3(-half, 1.0, half),
	]
	# Bottom face edges.
	var edges = [
		[0, 1],
		[1, 2],
		[2, 3],
		[3, 0],
		# Top face edges.
		[4, 5],
		[5, 6],
		[6, 7],
		[7, 4],
		# Vertical edges.
		[0, 4],
		[1, 5],
		[2, 6],
		[3, 7],
	]
	for e in edges:
		mesh.surface_add_vertex(corners[e[0]])
		mesh.surface_add_vertex(corners[e[1]])

	mesh.surface_end()


func _on_spawn_requested(species_name: String) -> void:
	if _state == State.PLACING:
		if _species_name == species_name and _action_name == "":
			_exit_placement()
			return
	_species_name = species_name
	_action_name = ""
	_enter_placement()


func _on_action_requested(action_name: String) -> void:
	# Only handle placement actions (e.g., "Summon"). "Build" is handled by
	# construction_controller.gd, not by placement.
	if action_name != "Summon":
		return
	if _state == State.PLACING:
		if _action_name == action_name and _species_name == "":
			_exit_placement()
			return
	_action_name = action_name
	_species_name = ""
	_enter_placement()


func _enter_placement() -> void:
	_state = State.PLACING
	_draw_wireframe_cube(_highlight.mesh as ImmediateMesh)
	_highlight.visible = false
	_has_snap = false


func _process(_delta: float) -> void:
	if _state != State.PLACING:
		return

	var mouse_pos := get_viewport().get_mouse_position()
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)
	var cam_pos := _camera.global_position

	# Fetch nav nodes visible from the current camera position (Rust-side
	# voxel raycast filters out nodes occluded by solid geometry).
	# Ground-only species (Capybara, Boar, Deer) can only target ground nodes.
	if _species_name != "" and _bridge.is_species_ground_only(_species_name):
		_valid_positions = _bridge.get_visible_ground_nav_nodes(cam_pos)
	else:
		# Climbing species and task actions can target any nav node.
		_valid_positions = _bridge.get_visible_nav_nodes(cam_pos)

	# Find the nav node whose perpendicular distance to the mouse ray is
	# smallest. For each point P, the closest point on the ray is:
	#   Q = origin + max(0, dot(P - origin, dir)) * dir
	# and the snap distance is |P - Q|.
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var best_pos := Vector3.ZERO
	var found := false

	for i in _valid_positions.size():
		var pos := _valid_positions[i]
		var to_pos := pos - ray_origin
		var t := maxf(0.0, to_pos.dot(ray_dir))
		var closest_on_ray := ray_origin + ray_dir * t
		var diff := pos - closest_on_ray
		var dist_sq := diff.length_squared()
		if dist_sq < best_dist_sq:
			best_dist_sq = dist_sq
			best_pos = pos
			found = true

	if found:
		_snapped_position = best_pos
		_has_snap = true
		_highlight.global_position = best_pos
		_highlight.visible = true
	else:
		_highlight.visible = false
		_has_snap = false


func _unhandled_input(event: InputEvent) -> void:
	if _state != State.PLACING:
		return

	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.pressed:
			if mb.button_index == MOUSE_BUTTON_LEFT and _has_snap:
				_confirm_spawn()
				get_viewport().set_input_as_handled()
			elif mb.button_index == MOUSE_BUTTON_RIGHT:
				_exit_placement()
				get_viewport().set_input_as_handled()

	if event is InputEventKey:
		var key := event as InputEventKey
		if key.pressed and key.keycode == KEY_ESCAPE:
			_exit_placement()
			get_viewport().set_input_as_handled()


func _confirm_spawn() -> void:
	var x := int(_snapped_position.x)
	var y := int(_snapped_position.y)
	var z := int(_snapped_position.z)

	if _action_name == "Summon":
		_bridge.create_goto_task(x, y, z)
	elif _species_name != "":
		_bridge.spawn_creature(_species_name, x, y, z)

	_exit_placement()


func _exit_placement() -> void:
	_state = State.IDLE
	_species_name = ""
	_action_name = ""
	_highlight.visible = false
	_has_snap = false
