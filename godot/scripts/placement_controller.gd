## Placement controller for click-to-place creature spawning and task creation.
##
## State machine (IDLE / PLACING) that handles the placement flow:
## 1. Toolbar emits spawn_requested or action_requested → enter PLACING mode
## 2. Each frame: cast a ray from the camera through the mouse cursor, hit
##    solid geometry (Rust raycast_solid), then snap to the nearest nav node
##    (Rust find_nearest_node). Show wireframe highlight cube at that position.
## 3. Left-click: confirm action via SimBridge, stay in placement mode
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
## sim_bridge.rs for snap_placement_to_ray (raycast + nearest-node snap).

extends Node3D

enum State { IDLE, PLACING }

## Maximum perpendicular distance (in world units) from the mouse ray to a nav
## node for it to be considered a snap candidate.
var _state: State = State.IDLE
var _species_name: String = ""
var _action_name: String = ""
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
	# Scale wireframe to creature footprint (2x2x2 for elephants, 1x1x1 default).
	var footprint := (
		_bridge.get_species_footprint(_species_name) if _species_name != "" else Vector3i(1, 1, 1)
	)
	_highlight.scale = Vector3(footprint.x, footprint.y, footprint.z)
	_draw_wireframe_cube(_highlight.mesh as ImmediateMesh)
	_highlight.visible = false
	_has_snap = false


func _process(_delta: float) -> void:
	if _state != State.PLACING:
		return

	var mouse_pos := get_viewport().get_mouse_position()
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	# Determine placement constraints from the species.
	var ground_only := false
	var large := false
	if _species_name != "":
		var footprint := _bridge.get_species_footprint(_species_name)
		large = footprint.x > 1 or footprint.z > 1
		ground_only = large or _bridge.is_species_ground_only(_species_name)

	# Single Rust call: raycast to find the solid surface under the cursor,
	# then snap to the nearest nav node from the adjacent air voxel.
	var result := _bridge.snap_placement_to_ray(ray_origin, ray_dir, ground_only, large)

	if result.get("hit", false):
		_snapped_position = result["position"]
		_has_snap = true
		_highlight.global_position = _snapped_position
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


func _exit_placement() -> void:
	_state = State.IDLE
	_species_name = ""
	_action_name = ""
	_highlight.visible = false
	_has_snap = false
