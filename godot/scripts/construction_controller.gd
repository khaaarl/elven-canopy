## Construction mode controller with mouse-driven click-drag placement.
##
## Manages the construction mode lifecycle with a five-state state machine:
##   INACTIVE → ACTIVE → HOVER → DRAGGING → PREVIEW → (confirm → HOVER)
##
## Five build modes:
##   "platform" — flat rectangular platform (1 voxel high), placed on a
##     height-slice via mouse projection onto the camera's Y-level plane.
##   "building" — enclosed room (min 3x3 footprint), placed by raycasting
##     to solid surfaces. Height adjustable in PREVIEW via +/- buttons.
##   "ladder" — vertical 1x1 column, placed by raycasting to surfaces.
##     Auto-orientation picks the facing with most adjacent solid voxels.
##   "carve" — removes voxels from a 3D rectangular prism. Uses height-slice
##     projection; vertical extent set by camera height changes during drag.
##   "strut" — diagonal support strut between two endpoints. Uses height-slice
##     projection; endpoint A Y set on mousedown, endpoint B Y changes with
##     camera height during drag. Ghost via MultiMesh of voxel cubes.
##
## Mouse-to-voxel projection:
##   Platform/carve/strut: ray-plane intersection at Y = camera_focus_y + 0.5.
##   Building/ladder: bridge.raycast_solid_with_blueprints() to find solid
##     surfaces (blueprint-aware — designated blueprints are hittable).
##
## Input handling: hover position updates in _process() (polling mouse),
## discrete events (click/release/keys) in _unhandled_input(). ESC exits
## the current sub-state first (PREVIEW/DRAGGING → HOVER → ACTIVE →
## INACTIVE), sitting between placement_controller.gd and
## selection_controller.gd in the ESC precedence chain.
##
## See also: action_toolbar.gd which emits the "Build" action,
## orbital_camera.gd which provides set_vertical_snap()/get_focus_voxel(),
## height_grid_renderer.gd for the wireframe grid overlay,
## main.gd which wires this controller into the scene,
## sim_bridge.rs for raycast_solid_with_blueprints(), auto_ladder_orientation(),
## designate_build_rect(), designate_building(), designate_ladder(),
## validate_*_preview() methods.

extends Node

signal construction_mode_entered
signal construction_mode_exited
signal blueprint_placed

enum State { INACTIVE, ACTIVE, HOVER, DRAGGING, PREVIEW }

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

## Valid horizontal face directions for ladder orientation, cycled by rotate button.
## Order: Auto → East(+X) → South(+Z) → West(-X) → North(-Z).
const LADDER_ORIENTATIONS: Array[int] = [0, 4, 1, 5]

## Human-readable names for ladder orientations, indexed by face direction.
## -1 = Auto (computed from adjacent solid voxels).
const ORIENTATION_NAMES = {
	-1: "Auto",
	0: "East (+X)",
	1: "West (-X)",
	4: "South (+Z)",
	5: "North (-Z)",
}

var _state: int = State.INACTIVE
var _bridge: SimBridge
var _camera_pivot: Node3D
var _camera: Camera3D
var _panel: PanelContainer
var _ghost: MeshInstance3D
var _ghost_material: StandardMaterial3D
var _height_grid: Node3D

## Current build mode: "platform", "building", "ladder", "carve", or "strut".
var _build_mode: String = "platform"

## Drag state.
var _drag_start: Vector3i = Vector3i.ZERO
var _drag_current: Vector3i = Vector3i.ZERO
var _drag_start_face: int = 2  # Face of the surface clicked to start drag.
var _drag_y_start: int = 0  # Carve: camera Y when drag began.
var _drag_y_current: int = 0  # Carve: current camera Y during drag.
var _last_valid_drag: Vector3i = Vector3i.ZERO  # Last valid drag position.
var _has_valid_hover: bool = false
var _hover_voxel: Vector3i = Vector3i.ZERO

## Strut-specific state.
var _strut_endpoint_a: Vector3i = Vector3i.ZERO
var _strut_endpoint_b: Vector3i = Vector3i.ZERO
var _strut_y_a: int = 0  # Locked Y-level of endpoint A (set on mousedown).
var _strut_multimesh: MultiMeshInstance3D
var _strut_ghost_voxels: PackedInt32Array = PackedInt32Array()

## Computed dimensions (from drag rectangle, capped at 10).
var _width: int = 1
var _depth: int = 1
var _height: int = 1

## Ladder-specific state.
## -1 = Auto, 0/1/4/5 = manual orientation.
var _ladder_orientation: int = -1
## Effective orientation used for validation/ghost (resolved from auto).
var _effective_orientation: int = 0
var _ladder_kind: int = 0  # 0=Wood, 1=Rope.

## Structural preview tier: "Ok", "Warning", or "Blocked".
var _validation_tier: String = "Ok"
var _focus_valid: bool = false

## Cached inputs for change-detection — only re-validate when these change.
var _last_preview_voxel: Vector3i = Vector3i(999999, 999999, 999999)
var _last_preview_width: int = -1
var _last_preview_depth: int = -1
var _last_preview_height: int = -1
var _last_preview_orientation: int = -1
var _last_preview_kind: int = -1

## World bounds for clamping.
var _world_size: Vector3i = Vector3i.ZERO

## Pre-computed face rotations for ladder ghost orientation.
var _face_rotations: Array[Basis] = []

## UI references for panel controls.
var _placing_controls: VBoxContainer
var _height_row: HBoxContainer
var _height_label: Label
var _orientation_row: HBoxContainer
var _orientation_label: Label
var _kind_row: HBoxContainer
var _kind_label: Label
var _confirm_btn: Button
var _cancel_btn: Button

## Temporary label for build validation messages.
var _message_label: Label
var _message_timer: float = 0.0


func setup(bridge: SimBridge, camera_pivot: Node3D) -> void:
	_bridge = bridge
	_camera_pivot = camera_pivot
	_camera = camera_pivot.get_node("Camera3D")
	_world_size = bridge.get_world_size()


func is_active() -> bool:
	return _state != State.INACTIVE


func is_placing() -> bool:
	return _state >= State.HOVER


func get_panel() -> PanelContainer:
	return _panel


func set_height_grid_renderer(grid: Node3D) -> void:
	_height_grid = grid


func connect_toolbar(toolbar: Node) -> void:
	toolbar.action_requested.connect(_on_action_requested)


func _ready() -> void:
	_build_rotations()
	_build_panel()
	_build_ghost()
	_build_strut_ghost()


func _build_rotations() -> void:
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(90)))  # PosX
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(-90)))  # NegX
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(-90)))  # PosY
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(90)))  # NegY
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(180)))  # PosZ
	_face_rotations.append(Basis.IDENTITY)  # NegZ


func _build_panel() -> void:
	_panel = PanelContainer.new()
	_panel.anchor_left = 1.0
	_panel.anchor_top = 0.0
	_panel.anchor_right = 1.0
	_panel.anchor_bottom = 1.0
	_panel.offset_left = -250
	_panel.offset_top = 0
	_panel.offset_right = 0
	_panel.offset_bottom = 0

	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	margin.add_theme_constant_override("margin_right", 12)
	margin.add_theme_constant_override("margin_top", 12)
	margin.add_theme_constant_override("margin_bottom", 12)
	_panel.add_child(margin)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 8)
	margin.add_child(vbox)

	# Header.
	var header := HBoxContainer.new()
	vbox.add_child(header)

	var title := Label.new()
	title.text = "Construction"
	title.add_theme_font_size_override("font_size", 20)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_deactivate)
	header.add_child(close_btn)

	vbox.add_child(HSeparator.new())

	# Mode buttons (visible in ACTIVE and HOVER).
	var platform_btn := Button.new()
	platform_btn.text = "Platform [P]"
	platform_btn.pressed.connect(func(): _switch_mode("platform"))
	vbox.add_child(platform_btn)

	var building_btn := Button.new()
	building_btn.text = "Building [G]"
	building_btn.pressed.connect(func(): _switch_mode("building"))
	vbox.add_child(building_btn)

	var ladder_btn := Button.new()
	ladder_btn.text = "Ladder [L]"
	ladder_btn.pressed.connect(func(): _switch_mode("ladder"))
	vbox.add_child(ladder_btn)

	var carve_btn := Button.new()
	carve_btn.text = "Carve [C]"
	carve_btn.pressed.connect(func(): _switch_mode("carve"))
	vbox.add_child(carve_btn)

	var strut_btn := Button.new()
	strut_btn.text = "Strut"
	strut_btn.pressed.connect(func(): _switch_mode("strut"))
	vbox.add_child(strut_btn)

	# PREVIEW controls container.
	_placing_controls = VBoxContainer.new()
	_placing_controls.add_theme_constant_override("separation", 6)
	_placing_controls.visible = false
	vbox.add_child(_placing_controls)

	# Height row (building/carve PREVIEW only).
	_height_row = HBoxContainer.new()
	_height_row.add_theme_constant_override("separation", 4)
	_height_row.visible = false
	_placing_controls.add_child(_height_row)

	var height_text := Label.new()
	height_text.text = "Height:"
	height_text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_height_row.add_child(height_text)

	var height_minus := Button.new()
	height_minus.text = "-"
	height_minus.custom_minimum_size = Vector2(30, 0)
	height_minus.pressed.connect(_height_decrease)
	_height_row.add_child(height_minus)

	_height_label = Label.new()
	_height_label.text = "1"
	_height_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_height_label.custom_minimum_size = Vector2(24, 0)
	_height_row.add_child(_height_label)

	var height_plus := Button.new()
	height_plus.text = "+"
	height_plus.custom_minimum_size = Vector2(30, 0)
	height_plus.pressed.connect(_height_increase)
	_height_row.add_child(height_plus)

	# Orientation row (ladder PREVIEW only).
	_orientation_row = HBoxContainer.new()
	_orientation_row.add_theme_constant_override("separation", 4)
	_orientation_row.visible = false
	_placing_controls.add_child(_orientation_row)

	var orient_text := Label.new()
	orient_text.text = "Face:"
	orient_text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_orientation_row.add_child(orient_text)

	_orientation_label = Label.new()
	_orientation_label.text = "Auto"
	_orientation_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_orientation_label.custom_minimum_size = Vector2(90, 0)
	_orientation_row.add_child(_orientation_label)

	var rotate_btn := Button.new()
	rotate_btn.text = "R"
	rotate_btn.custom_minimum_size = Vector2(30, 0)
	rotate_btn.pressed.connect(_rotate_ladder)
	_orientation_row.add_child(rotate_btn)

	# Kind row (ladder PREVIEW only).
	_kind_row = HBoxContainer.new()
	_kind_row.add_theme_constant_override("separation", 4)
	_kind_row.visible = false
	_placing_controls.add_child(_kind_row)

	var kind_text := Label.new()
	kind_text.text = "Type:"
	kind_text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_kind_row.add_child(kind_text)

	var wood_btn := Button.new()
	wood_btn.text = "Wood"
	wood_btn.pressed.connect(_set_ladder_wood)
	_kind_row.add_child(wood_btn)

	var rope_btn := Button.new()
	rope_btn.text = "Rope"
	rope_btn.pressed.connect(_set_ladder_rope)
	_kind_row.add_child(rope_btn)

	_kind_label = Label.new()
	_kind_label.text = "[Wood]"
	_kind_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_kind_label.custom_minimum_size = Vector2(50, 0)
	_kind_row.add_child(_kind_label)

	# Confirm / Cancel buttons (PREVIEW only).
	var btn_row := HBoxContainer.new()
	btn_row.add_theme_constant_override("separation", 8)
	_placing_controls.add_child(btn_row)

	_confirm_btn = Button.new()
	_confirm_btn.text = "Confirm"
	_confirm_btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_confirm_btn.disabled = true
	_confirm_btn.pressed.connect(_confirm_placement)
	btn_row.add_child(_confirm_btn)

	_cancel_btn = Button.new()
	_cancel_btn.text = "Cancel"
	_cancel_btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_cancel_btn.pressed.connect(func(): _cancel_to_hover())
	btn_row.add_child(_cancel_btn)

	# Validation message label.
	_message_label = Label.new()
	_message_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_message_label.add_theme_font_size_override("font_size", 14)
	_message_label.add_theme_color_override("font_color", Color(1.0, 0.85, 0.3))
	_message_label.visible = false
	_placing_controls.add_child(_message_label)

	_panel.visible = false


func _build_ghost() -> void:
	_ghost = MeshInstance3D.new()
	var box_mesh := BoxMesh.new()
	box_mesh.size = Vector3(1.0, 1.0, 1.0)
	_ghost.mesh = box_mesh

	_ghost_material = StandardMaterial3D.new()
	_ghost_material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	_ghost_material.albedo_color = Color(0.3, 0.5, 1.0, 0.4)
	_ghost_material.no_depth_test = true
	_ghost.material_override = _ghost_material

	_ghost.visible = false
	add_child(_ghost)


func _build_strut_ghost() -> void:
	_strut_multimesh = MultiMeshInstance3D.new()
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.mesh = BoxMesh.new()
	(mm.mesh as BoxMesh).size = Vector3(0.95, 0.95, 0.95)
	mm.instance_count = 100
	mm.visible_instance_count = 0
	_strut_multimesh.multimesh = mm

	var mat := StandardMaterial3D.new()
	mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	mat.vertex_color_use_as_albedo = true
	mat.no_depth_test = true
	_strut_multimesh.material_override = mat

	_strut_multimesh.visible = false
	add_child(_strut_multimesh)


# -----------------------------------------------------------------------
# State transitions
# -----------------------------------------------------------------------


func _on_action_requested(action_name: String) -> void:
	if action_name != "Build":
		return
	if _state != State.INACTIVE:
		_deactivate()
	else:
		_activate()


func _activate() -> void:
	_state = State.ACTIVE
	_panel.visible = true
	construction_mode_entered.emit()


func _deactivate() -> void:
	_cancel_any_drag()
	_state = State.INACTIVE
	_panel.visible = false
	_ghost.visible = false
	_strut_multimesh.visible = false
	_placing_controls.visible = false
	_message_label.visible = false
	_message_timer = 0.0
	_ladder_orientation = -1  # Auto
	_ladder_kind = 0
	_set_grid_visible(false)
	if _camera_pivot and _camera_pivot.has_method("set_vertical_snap"):
		_camera_pivot.set_vertical_snap(false)
	construction_mode_exited.emit()


func _switch_mode(mode: String) -> void:
	_cancel_any_drag()
	_build_mode = mode
	_ladder_orientation = -1  # Auto
	_ladder_kind = 0
	_enter_hover()


func _enter_hover() -> void:
	_state = State.HOVER
	_ghost.visible = false
	_placing_controls.visible = false
	_message_label.visible = false
	_message_timer = 0.0
	_has_valid_hover = false
	_invalidate_preview_cache()

	# Enable vertical snap and grid for height-slice modes.
	var uses_height_slice := (
		_build_mode == "platform" or _build_mode == "carve" or _build_mode == "strut"
	)
	if _camera_pivot and _camera_pivot.has_method("set_vertical_snap"):
		_camera_pivot.set_vertical_snap(uses_height_slice)
	_set_grid_visible(uses_height_slice)


func _enter_dragging() -> void:
	_state = State.DRAGGING
	_drag_start = _hover_voxel
	_drag_current = _hover_voxel
	_last_valid_drag = _hover_voxel
	_ghost.visible = true
	_placing_controls.visible = false

	if _build_mode == "carve":
		var focus: Vector3i = _camera_pivot.get_focus_voxel()
		_drag_y_start = focus.y
		_drag_y_current = focus.y
	elif _build_mode == "strut":
		_strut_endpoint_a = _hover_voxel
		_strut_y_a = _hover_voxel.y

	_update_drag_dimensions()
	_update_ghost()


func _enter_preview() -> void:
	_state = State.PREVIEW
	_placing_controls.visible = true

	# Compute final dimensions from drag.
	_update_drag_dimensions()

	# Show mode-specific controls.
	if _build_mode == "building":
		_height_row.visible = true
		_orientation_row.visible = false
		_kind_row.visible = false
		if _height < 2:
			_height = 2
		_height_label.text = str(_height)
	elif _build_mode == "ladder":
		_height_row.visible = false
		_orientation_row.visible = true
		_kind_row.visible = true
		_orientation_label.text = ORIENTATION_NAMES.get(_ladder_orientation, "Auto")
		_kind_label.text = "[Wood]" if _ladder_kind == 0 else "[Rope]"
	elif _build_mode == "carve":
		_height_row.visible = true
		_orientation_row.visible = false
		_kind_row.visible = false
		_height_label.text = str(_height)
	else:
		_height_row.visible = false
		_orientation_row.visible = false
		_kind_row.visible = false

	if _build_mode == "strut":
		_strut_endpoint_b = _drag_current

	_confirm_btn.text = "Carve" if _build_mode == "carve" else "Confirm"

	# Resolve auto-orientation for ladders.
	_resolve_ladder_orientation()

	_invalidate_preview_cache()
	_update_ghost()


func _cancel_to_hover() -> void:
	_cancel_any_drag()
	_enter_hover()


func _cancel_any_drag() -> void:
	_ghost.visible = false
	_strut_multimesh.visible = false
	_placing_controls.visible = false
	_message_label.visible = false
	_message_timer = 0.0


func _exit_to_active() -> void:
	_cancel_any_drag()
	_state = State.ACTIVE
	_set_grid_visible(false)
	if _camera_pivot and _camera_pivot.has_method("set_vertical_snap"):
		_camera_pivot.set_vertical_snap(false)


# -----------------------------------------------------------------------
# Mouse-to-voxel projection
# -----------------------------------------------------------------------


## Project mouse ray onto horizontal plane at Y = y_level + 0.5.
## Returns Vector3i or null.
func _project_height_slice(y_level: int) -> Variant:
	var mouse_pos := get_viewport().get_mouse_position()
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	if abs(ray_dir.y) < 0.001:
		return null
	var plane_y := float(y_level) + 0.5
	var t := (plane_y - ray_origin.y) / ray_dir.y
	if t < 0.0:
		return null

	var hit := ray_origin + ray_dir * t
	var vx := int(floor(hit.x))
	var vz := int(floor(hit.z))

	# Clamp to world bounds.
	vx = clampi(vx, 0, _world_size.x - 1)
	vz = clampi(vz, 0, _world_size.z - 1)

	return Vector3i(vx, y_level, vz)


## Raycast to find the first solid surface. Returns Dictionary with
## {voxel: Vector3i, face: int} or null.
## Blueprint-aware: designated blueprints are treated as their target voxel
## types, so the player can click on blueprint surfaces (e.g. a designated
## platform reads as solid).
func _project_surface() -> Variant:
	var mouse_pos := get_viewport().get_mouse_position()
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	var result := _bridge.raycast_solid_with_blueprints(ray_origin, ray_dir)
	if not result.get("hit", false):
		return null
	return result


# -----------------------------------------------------------------------
# Hover and drag updates (_process)
# -----------------------------------------------------------------------


func _process(delta: float) -> void:
	# Fade out post-confirm messages.
	if _message_timer > 0.0:
		_message_timer -= delta
		if _message_timer <= 0.0:
			_message_label.visible = false

	if _state == State.HOVER:
		_update_hover()
	elif _state == State.DRAGGING:
		_update_drag()
	elif _state == State.PREVIEW:
		_update_preview_validation()


func _update_hover() -> void:
	# Suppress hover when mouse is over UI.
	if get_viewport().gui_get_hovered_control() != null:
		_has_valid_hover = false
		_ghost.visible = false
		return

	var result = _project_for_mode()
	if result == null:
		_has_valid_hover = false
		_ghost.visible = false
		return

	if result is Vector3i:
		_hover_voxel = result
		_has_valid_hover = true
	elif result is Dictionary:
		# Surface raycast result — compute the placement voxel.
		var voxel: Vector3i = result["voxel"]
		var face: int = result["face"]
		if _build_mode == "building":
			# Building: only accept top face (PosY = 2). Anchor is the solid voxel.
			if face != 2:
				_has_valid_hover = false
				_ghost.visible = false
				return
			_hover_voxel = voxel
		else:
			# Ladder: accept top (2) or horizontal faces. Place in air adjacent to face.
			_hover_voxel = (
				voxel
				+ Vector3i(
					DIRECTION_OFFSETS[face].x, DIRECTION_OFFSETS[face].y, DIRECTION_OFFSETS[face].z
				)
			)
		_has_valid_hover = true
		_drag_start_face = face

	# Show single-voxel ghost at hover position.
	_ghost.visible = true
	_ghost.basis = Basis.IDENTITY
	_ghost.mesh.size = Vector3(1.0, 1.0, 1.0)
	_ghost.global_position = Vector3(
		_hover_voxel.x + 0.5, _hover_voxel.y + 0.5, _hover_voxel.z + 0.5
	)
	_ghost_material.albedo_color = Color(0.3, 0.5, 1.0, 0.3)


func _update_drag() -> void:
	# Suppress when mouse is over UI.
	if get_viewport().gui_get_hovered_control() != null:
		return

	# Update carve Y from camera height.
	if _build_mode == "carve":
		var focus: Vector3i = _camera_pivot.get_focus_voxel()
		_drag_y_current = focus.y

	var result = _project_for_mode_drag()
	if result != null:
		var new_pos: Vector3i
		if result is Vector3i:
			new_pos = result
		elif result is Dictionary:
			var voxel: Vector3i = result["voxel"]
			var face: int = result["face"]
			if _build_mode == "building":
				if face != 2 or voxel.y != _drag_start.y:
					# Invalid surface — keep last valid.
					new_pos = _last_valid_drag
				else:
					new_pos = voxel
			else:
				# Ladder: only Y matters from the endpoint.
				new_pos = Vector3i(
					_drag_start.x,
					voxel.y + int(DIRECTION_OFFSETS[face].y),
					_drag_start.z,
				)
		_drag_current = new_pos
		_last_valid_drag = new_pos

	_update_drag_dimensions()
	_update_ghost()


## Project for current mode (hover — fresh position).
func _project_for_mode() -> Variant:
	if _build_mode in ["platform", "carve", "strut"]:
		var focus: Vector3i = _camera_pivot.get_focus_voxel()
		return _project_height_slice(focus.y)
	return _project_surface()


## Project for current mode during drag (may use locked Y).
func _project_for_mode_drag() -> Variant:
	if _build_mode == "platform":
		return _project_height_slice(_drag_start.y)
	if _build_mode in ["carve", "strut"]:
		# Strut: endpoint B Y changes with camera height (unlike platform
		# which locks Y to drag start).
		var focus: Vector3i = _camera_pivot.get_focus_voxel()
		return _project_height_slice(focus.y)
	return _project_surface()


# -----------------------------------------------------------------------
# Dimension computation
# -----------------------------------------------------------------------


func _update_drag_dimensions() -> void:
	if _build_mode == "ladder":
		# Ladder: vertical column at _drag_start.x/z.
		_width = 1
		_depth = 1
		_height = mini(absi(_drag_current.y - _drag_start.y) + 1, 10)
	else:
		var min_x := mini(_drag_start.x, _drag_current.x)
		var max_x := maxi(_drag_start.x, _drag_current.x)
		var min_z := mini(_drag_start.z, _drag_current.z)
		var max_z := maxi(_drag_start.z, _drag_current.z)
		_width = mini(max_x - min_x + 1, 10)
		_depth = mini(max_z - min_z + 1, 10)

		if _build_mode == "carve":
			var min_y := mini(_drag_y_start, _drag_y_current)
			var max_y := maxi(_drag_y_start, _drag_y_current)
			_height = mini(max_y - min_y + 1, 10)
		elif _build_mode == "building":
			if _height < 2:
				_height = 2
		else:
			_height = 1


## Compute the min-corner AABB anchor from drag start/current.
func _get_anchor() -> Vector3i:
	if _build_mode == "ladder":
		return Vector3i(_drag_start.x, mini(_drag_start.y, _drag_current.y), _drag_start.z)
	var min_x := mini(_drag_start.x, _drag_current.x)
	var min_z := mini(_drag_start.z, _drag_current.z)
	if _build_mode == "carve":
		var min_y := mini(_drag_y_start, _drag_y_current)
		return Vector3i(min_x, min_y, min_z)
	return Vector3i(min_x, _drag_start.y, min_z)


# -----------------------------------------------------------------------
# Ghost mesh updates
# -----------------------------------------------------------------------


func _update_ghost() -> void:
	if _build_mode == "strut":
		_update_strut_ghost()
		return

	var anchor := _get_anchor()

	if _build_mode == "ladder":
		# In auto mode, always resolve the real orientation so the ghost and
		# validation agree — even while dragging.
		if _state == State.DRAGGING:
			_resolve_ladder_orientation()
		var orient := _effective_orientation

		_ghost.mesh.size = Vector3(0.9, _height, 0.05)
		_ghost.basis = _face_rotations[orient]
		var face_offset := DIRECTION_OFFSETS[orient] * (0.5 - FACE_INSET)
		_ghost.global_position = Vector3(
			anchor.x + 0.5 + face_offset.x,
			anchor.y + _height / 2.0,
			anchor.z + 0.5 + face_offset.z,
		)
	elif _build_mode == "building":
		var ghost_h := _height + 1
		_ghost.mesh.size = Vector3(_width, ghost_h, _depth)
		_ghost.basis = Basis.IDENTITY
		_ghost.global_position = Vector3(
			anchor.x + _width / 2.0,
			anchor.y + ghost_h / 2.0,
			anchor.z + _depth / 2.0,
		)
	elif _build_mode == "carve":
		_ghost.mesh.size = Vector3(_width, _height, _depth)
		_ghost.basis = Basis.IDENTITY
		_ghost.global_position = Vector3(
			anchor.x + _width / 2.0,
			anchor.y + _height / 2.0,
			anchor.z + _depth / 2.0,
		)
	else:
		# Platform.
		_ghost.mesh.size = Vector3(_width, 1.0, _depth)
		_ghost.basis = Basis.IDENTITY
		_ghost.global_position = Vector3(
			anchor.x + _width / 2.0,
			anchor.y + 0.5,
			anchor.z + _depth / 2.0,
		)

	_ghost.visible = true
	_run_validation()


# -----------------------------------------------------------------------
# Validation
# -----------------------------------------------------------------------


func _run_validation() -> void:
	var anchor: Vector3i
	if _build_mode == "strut":
		anchor = _strut_endpoint_a
	else:
		anchor = _get_anchor()

	var needs_revalidate: bool
	if _build_mode == "strut":
		# Strut: cache on both endpoints.
		needs_revalidate = (
			anchor != _last_preview_voxel
			or _strut_endpoint_b.x != _last_preview_width
			or _strut_endpoint_b.y != _last_preview_depth
			or _strut_endpoint_b.z != _last_preview_height
		)
	else:
		needs_revalidate = (
			anchor != _last_preview_voxel
			or _width != _last_preview_width
			or _depth != _last_preview_depth
			or _height != _last_preview_height
			or _effective_orientation != _last_preview_orientation
			or _ladder_kind != _last_preview_kind
		)
	if not needs_revalidate:
		return

	_last_preview_voxel = anchor
	if _build_mode == "strut":
		_last_preview_width = _strut_endpoint_b.x
		_last_preview_depth = _strut_endpoint_b.y
		_last_preview_height = _strut_endpoint_b.z
	else:
		_last_preview_width = _width
		_last_preview_depth = _depth
		_last_preview_height = _height
		_last_preview_orientation = _effective_orientation
		_last_preview_kind = _ladder_kind

	var result: Dictionary
	if _build_mode == "strut":
		result = (
			_bridge
			. validate_strut_preview(
				_strut_endpoint_a.x,
				_strut_endpoint_a.y,
				_strut_endpoint_a.z,
				_strut_endpoint_b.x,
				_strut_endpoint_b.y,
				_strut_endpoint_b.z,
			)
		)
		_strut_ghost_voxels = result.get("voxels", PackedInt32Array())
	elif _build_mode == "ladder":
		result = _bridge.validate_ladder_preview(
			anchor.x, anchor.y, anchor.z, _height, _effective_orientation, _ladder_kind
		)
	elif _build_mode == "building":
		result = _bridge.validate_building_preview(
			anchor.x, anchor.y, anchor.z, _width, _depth, _height
		)
	elif _build_mode == "carve":
		result = _bridge.validate_carve_preview(
			anchor.x, anchor.y, anchor.z, _width, _depth, _height
		)
	else:
		result = _bridge.validate_platform_preview(anchor.x, anchor.y, anchor.z, _width, _depth)

	_validation_tier = result.get("tier", "Blocked")
	_focus_valid = _validation_tier != "Blocked"

	# Update validation message.
	var msg: String = result.get("message", "")
	if _validation_tier == "Ok" or msg == "":
		if _message_timer <= 0.0:
			_message_label.visible = false
	else:
		_message_label.text = msg
		_message_label.visible = true
		_message_timer = 0.0
		if _validation_tier == "Blocked":
			_message_label.add_theme_color_override("font_color", Color(1.0, 0.4, 0.4))
		else:
			_message_label.add_theme_color_override("font_color", Color(1.0, 0.85, 0.3))

	# Update ghost color.
	if _state == State.PREVIEW:
		_confirm_btn.disabled = not _focus_valid
	if _validation_tier == "Ok":
		if _build_mode == "carve":
			_ghost_material.albedo_color = Color(0.9, 0.4, 0.2, 0.4)
		else:
			_ghost_material.albedo_color = Color(0.3, 0.5, 1.0, 0.4)
	elif _validation_tier == "Warning":
		_ghost_material.albedo_color = Color(1.0, 0.85, 0.3, 0.4)
	else:
		_ghost_material.albedo_color = Color(1.0, 0.2, 0.2, 0.4)


func _update_preview_validation() -> void:
	# In PREVIEW, ghost is frozen but we still need to revalidate if
	# height/orientation/kind changed via panel controls.
	_update_ghost()


func _invalidate_preview_cache() -> void:
	_last_preview_voxel = Vector3i(999999, 999999, 999999)
	_last_preview_width = -1
	_last_preview_depth = -1
	_last_preview_height = -1
	_last_preview_orientation = -1
	_last_preview_kind = -1


# -----------------------------------------------------------------------
# Input handling
# -----------------------------------------------------------------------


func _unhandled_input(event: InputEvent) -> void:
	if _state == State.INACTIVE:
		return

	if event is InputEventKey:
		var key := event as InputEventKey
		if not key.pressed:
			return
		_handle_key(key)

	elif event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		_handle_mouse_button(mb)


func _handle_key(key: InputEventKey) -> void:
	match _state:
		State.ACTIVE:
			if _try_mode_key(key):
				return
			if key.keycode == KEY_ESCAPE:
				_deactivate()
				get_viewport().set_input_as_handled()

		State.HOVER:
			if _try_mode_key(key):
				return
			if key.keycode == KEY_ESCAPE:
				_exit_to_active()
				get_viewport().set_input_as_handled()

		State.DRAGGING:
			if _try_mode_key(key):
				return
			if key.keycode == KEY_ESCAPE:
				_cancel_to_hover()
				get_viewport().set_input_as_handled()

		State.PREVIEW:
			if key.keycode == KEY_ENTER and _focus_valid:
				_confirm_placement()
				get_viewport().set_input_as_handled()
			elif _try_mode_key(key):
				return
			elif key.keycode == KEY_ESCAPE:
				_cancel_to_hover()
				get_viewport().set_input_as_handled()


## Try to handle P/G/L/C mode keys. Returns true if consumed.
func _try_mode_key(key: InputEventKey) -> bool:
	var mode := ""
	if key.keycode == KEY_P:
		mode = "platform"
	elif key.keycode == KEY_G:
		mode = "building"
	elif key.keycode == KEY_L:
		mode = "ladder"
	elif key.keycode == KEY_C:
		mode = "carve"

	if mode == "":
		return false

	_switch_mode(mode)
	get_viewport().set_input_as_handled()
	return true


func _handle_mouse_button(mb: InputEventMouseButton) -> void:
	match _state:
		State.HOVER:
			if mb.pressed and mb.button_index == MOUSE_BUTTON_LEFT and _has_valid_hover:
				_enter_dragging()
				get_viewport().set_input_as_handled()
			elif mb.pressed and mb.button_index == MOUSE_BUTTON_RIGHT:
				_exit_to_active()
				get_viewport().set_input_as_handled()

		State.DRAGGING:
			if not mb.pressed and mb.button_index == MOUSE_BUTTON_LEFT:
				_enter_preview()
				get_viewport().set_input_as_handled()
			elif mb.pressed and mb.button_index == MOUSE_BUTTON_RIGHT:
				_cancel_to_hover()
				get_viewport().set_input_as_handled()

		State.PREVIEW:
			if mb.pressed and mb.button_index == MOUSE_BUTTON_RIGHT:
				_cancel_to_hover()
				get_viewport().set_input_as_handled()


# -----------------------------------------------------------------------
# Confirm placement
# -----------------------------------------------------------------------


func _confirm_placement() -> void:
	var anchor := _get_anchor()
	if _build_mode == "strut":
		(
			_bridge
			. designate_strut(
				_strut_endpoint_a.x,
				_strut_endpoint_a.y,
				_strut_endpoint_a.z,
				_strut_endpoint_b.x,
				_strut_endpoint_b.y,
				_strut_endpoint_b.z,
			)
		)
	elif _build_mode == "ladder":
		_bridge.designate_ladder(
			anchor.x, anchor.y, anchor.z, _height, _effective_orientation, _ladder_kind
		)
	elif _build_mode == "building":
		_bridge.designate_building(anchor.x, anchor.y, anchor.z, _width, _depth, _height)
	elif _build_mode == "carve":
		_bridge.designate_carve(anchor.x, anchor.y, anchor.z, _width, _depth, _height)
	else:
		_bridge.designate_build_rect(anchor.x, anchor.y, anchor.z, _width, _depth)
	_invalidate_preview_cache()
	blueprint_placed.emit()
	# Return to HOVER for rapid repeated placement.
	_enter_hover()


# -----------------------------------------------------------------------
# Ladder helpers
# -----------------------------------------------------------------------


func _resolve_ladder_orientation() -> void:
	if _ladder_orientation == -1:
		# Auto: query the bridge for the best orientation.
		var anchor := _get_anchor()
		_effective_orientation = _bridge.auto_ladder_orientation(
			anchor.x, anchor.y, anchor.z, _height
		)
	else:
		_effective_orientation = _ladder_orientation


func _rotate_ladder() -> void:
	if _ladder_orientation == -1:
		# Auto → first manual orientation.
		_ladder_orientation = LADDER_ORIENTATIONS[0]
	else:
		var idx := LADDER_ORIENTATIONS.find(_ladder_orientation)
		idx = (idx + 1) % LADDER_ORIENTATIONS.size()
		if idx == 0:
			# Wrap back to Auto.
			_ladder_orientation = -1
		else:
			_ladder_orientation = LADDER_ORIENTATIONS[idx]
	_orientation_label.text = ORIENTATION_NAMES.get(_ladder_orientation, "?")
	_resolve_ladder_orientation()
	_invalidate_preview_cache()
	_update_ghost()


func _set_ladder_wood() -> void:
	_ladder_kind = 0
	_kind_label.text = "[Wood]"
	_invalidate_preview_cache()


func _set_ladder_rope() -> void:
	_ladder_kind = 1
	_kind_label.text = "[Rope]"
	_invalidate_preview_cache()


# -----------------------------------------------------------------------
# Height adjustment (PREVIEW only)
# -----------------------------------------------------------------------


func _height_decrease() -> void:
	_set_height(_height - 1)


func _height_increase() -> void:
	_set_height(_height + 1)


func _set_height(value: int) -> void:
	var max_h := 10 if _build_mode == "ladder" or _build_mode == "carve" else 5
	_height = clampi(value, 1, max_h)
	_height_label.text = str(_height)
	if _build_mode == "ladder" and _ladder_orientation == -1:
		_resolve_ladder_orientation()
	_invalidate_preview_cache()
	_update_ghost()


# -----------------------------------------------------------------------
# Strut ghost
# -----------------------------------------------------------------------


func _update_strut_ghost() -> void:
	# Determine endpoints based on state.
	if _state == State.DRAGGING:
		_strut_endpoint_b = _drag_current
	# In PREVIEW, endpoints are already set.

	# Hide the single-voxel ghost — strut uses the multimesh.
	_ghost.visible = false

	# Run validation to get the voxel line and tier.
	_run_validation()

	# Update MultiMesh instances from the validated voxel line.
	var mm := _strut_multimesh.multimesh
	var voxel_count := _strut_ghost_voxels.size() / 3
	if voxel_count == 0:
		_strut_multimesh.visible = false
		return

	# Grow the pool if needed (rare — struts > 100 voxels).
	if voxel_count > mm.instance_count:
		mm.instance_count = voxel_count

	mm.visible_instance_count = voxel_count

	# Choose color based on validation tier.
	var color: Color
	if _validation_tier == "Ok":
		color = Color(0.55, 0.30, 0.15, 0.5)  # Brown (strut material color).
	elif _validation_tier == "Warning":
		color = Color(1.0, 0.85, 0.3, 0.5)
	else:
		color = Color(1.0, 0.2, 0.2, 0.5)

	# Brighter color for endpoint A to distinguish it from the line.
	var color_a := Color(0.7, 0.5, 0.3, 0.6) if _validation_tier == "Ok" else color

	for i in range(voxel_count):
		var vx: int = _strut_ghost_voxels[i * 3]
		var vy: int = _strut_ghost_voxels[i * 3 + 1]
		var vz: int = _strut_ghost_voxels[i * 3 + 2]
		var xf := Transform3D(Basis.IDENTITY, Vector3(vx + 0.5, vy + 0.5, vz + 0.5))
		mm.set_instance_transform(i, xf)
		mm.set_instance_color(i, color_a if i == 0 else color)

	_strut_multimesh.visible = true

	# Update confirm button state.
	if _state == State.PREVIEW:
		_confirm_btn.disabled = not _focus_valid


# -----------------------------------------------------------------------
# Grid visibility helper
# -----------------------------------------------------------------------


func _set_grid_visible(vis: bool) -> void:
	if _height_grid:
		_height_grid.visible = vis
