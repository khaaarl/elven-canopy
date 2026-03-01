## Construction mode controller with platform, building, ladder, and carve placement.
##
## Manages the construction mode lifecycle: toggling on/off, showing a
## right-side panel with build options, enabling voxel-snap on the orbital
## camera, and handling multi-voxel rectangular blueprint placement for
## platforms (solid voxels), buildings (paper-thin walls with per-face
## restrictions), and ladders (vertical thin-panel climbers).
##
## State machine:
##   _active=false                  → INACTIVE (panel hidden, no ghost)
##   _active=true,  _placing=false  → ACTIVE   (panel shown, no ghost)
##   _active=true,  _placing=true   → PLACING  (panel shown, ghost visible)
##
## Three build modes:
##   "platform" — flat rectangular platform (Width x Depth, 1 voxel high).
##     Validation: all air + at least one face-adjacent solid.
##   "building" — enclosed room (Width x Depth x Height, min 3x3x1).
##     Foundation row is solid, interior has BuildingInterior voxels with
##     per-face restrictions (windows, door, ceiling, floor).
##     Validation: solid foundation + air interior.
##   "ladder" — vertical column of thin panels (Height x 1, oriented).
##     Wood ladders must be adjacent to solid; rope ladders hang from top.
##     Orientation selectable via panel button.
##     Validation: air/convertible column + anchoring check.
##   "carve" — remove solid voxels to Air (Width x Depth x Height).
##     Validation: at least one carvable solid voxel (not Air/ForestFloor).
##
## In PLACING mode, a translucent ghost follows the camera's focus voxel.
## For platforms/buildings it's a rectangle; for ladders it's a thin panel
## column. Dimension controls and mode-specific options (orientation, type)
## are shown. The ghost is blue when valid and red when invalid. The
## Construct button (or Enter/left-click) confirms placement; ESC/right-click
## cancels back to ACTIVE mode and resets dimensions.
##
## Input handling: ESC exits the current sub-mode first (PLACING → ACTIVE),
## then exits construction mode entirely (ACTIVE → INACTIVE). This sits
## between placement_controller.gd and selection_controller.gd in the ESC
## precedence chain (see main.gd docstring for the full chain).
##
## See also: action_toolbar.gd which emits the "Build" action,
## orbital_camera.gd which provides set_voxel_snap() / get_focus_voxel(),
## main.gd which wires this controller into the scene,
## blueprint_renderer.gd for rendering designated blueprints and ladder ghosts,
## building_renderer.gd for rendering building faces (walls/windows/doors),
## ladder_renderer.gd for rendering completed ladder panels,
## sim_bridge.rs for designate_build_rect(), designate_building(),
## designate_ladder(), and validate_ladder_preview(),
## placement_controller.gd for the ESC precedence pattern.

extends Node

signal construction_mode_entered
signal construction_mode_exited
signal blueprint_placed

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
## Order: East(+X) → South(+Z) → West(-X) → North(-Z).
const LADDER_ORIENTATIONS: Array[int] = [0, 4, 1, 5]

## Human-readable names for ladder orientations, indexed by face direction.
const ORIENTATION_NAMES = {0: "East (+X)", 1: "West (-X)", 4: "South (+Z)", 5: "North (-Z)"}

var _active: bool = false
var _placing: bool = false
var _bridge: SimBridge
var _camera_pivot: Node3D
var _panel: PanelContainer
var _ghost: MeshInstance3D
var _ghost_material: StandardMaterial3D
## Cached focus voxel (integer coordinates) used for placement validation
## and ghost positioning. Updated each _process frame while placing.
var _focus_voxel: Vector3i = Vector3i.ZERO
var _focus_valid: bool = false
## Structural preview tier: "Ok", "Warning", or "Blocked".
var _validation_tier: String = "Ok"
## Cached inputs for change-detection — only re-validate when these change.
var _last_preview_voxel: Vector3i = Vector3i(999999, 999999, 999999)
var _last_preview_width: int = -1
var _last_preview_depth: int = -1
var _last_preview_height: int = -1
var _last_preview_orientation: int = -1
var _last_preview_kind: int = -1

## Current build mode: "platform", "building", "ladder", or "carve".
var _build_mode: String = "platform"

## Platform/building/carve dimensions. Width and Depth range depends on mode:
## platform [1, 10], building [3, 10], carve [1, 10]. Height is
## building/ladder/carve [1, 5/10].
var _width: int = 1
var _depth: int = 1
var _height: int = 1

## Ladder-specific state.
var _ladder_orientation: int = 0  # Face direction index (0=PosX, 1=NegX, 4=PosZ, 5=NegZ).
var _ladder_kind: int = 0  # 0=Wood, 1=Rope.

## Pre-computed face rotations for ladder ghost orientation.
var _face_rotations: Array[Basis] = []

## UI references for dimension controls (created in _build_panel).
var _placing_controls: VBoxContainer
var _width_row: HBoxContainer
var _depth_row: HBoxContainer
var _width_label: Label
var _depth_label: Label
var _height_row: HBoxContainer
var _height_label: Label
var _orientation_row: HBoxContainer
var _orientation_label: Label
var _kind_row: HBoxContainer
var _kind_label: Label
var _construct_btn: Button

## Temporary label for build validation messages (warnings / block reasons).
var _message_label: Label
var _message_timer: float = 0.0


func setup(bridge: SimBridge, camera_pivot: Node3D) -> void:
	_bridge = bridge
	_camera_pivot = camera_pivot


func is_active() -> bool:
	return _active


## Returns the panel node so main.gd can parent it on a CanvasLayer.
func get_panel() -> PanelContainer:
	return _panel


func connect_toolbar(toolbar: Node) -> void:
	toolbar.action_requested.connect(_on_action_requested)


func _ready() -> void:
	_build_rotations()
	_build_panel()
	_build_ghost()


func _build_rotations() -> void:
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(90)))  # PosX
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(-90)))  # NegX
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(-90)))  # PosY
	_face_rotations.append(Basis(Vector3.RIGHT, deg_to_rad(90)))  # NegY
	_face_rotations.append(Basis(Vector3.UP, deg_to_rad(180)))  # PosZ
	_face_rotations.append(Basis.IDENTITY)  # NegZ


func _build_panel() -> void:
	_panel = PanelContainer.new()
	# Set anchors/offsets explicitly rather than using set_anchors_preset(),
	# because the panel is created as an orphan node (not yet in the tree).
	# set_anchors_preset with keep_offsets=false resets offsets to maintain
	# the current size, which is 0 for an orphan — resulting in zero width.
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

	# Header with title and close button.
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

	# Separator.
	vbox.add_child(HSeparator.new())

	# Platform build button.
	var platform_btn := Button.new()
	platform_btn.text = "Platform [P]"
	platform_btn.pressed.connect(_enter_placing_platform)
	vbox.add_child(platform_btn)

	# Building build button.
	var building_btn := Button.new()
	building_btn.text = "Building [G]"
	building_btn.pressed.connect(_enter_placing_building)
	vbox.add_child(building_btn)

	# Ladder build button.
	var ladder_btn := Button.new()
	ladder_btn.text = "Ladder [L]"
	ladder_btn.pressed.connect(_enter_placing_ladder)
	vbox.add_child(ladder_btn)

	# Carve button.
	var carve_btn := Button.new()
	carve_btn.text = "Carve [C]"
	carve_btn.pressed.connect(_enter_placing_carve)
	vbox.add_child(carve_btn)

	# Placing controls (dimension spinners + Construct button).
	# Hidden by default, shown when entering PLACING mode.
	_placing_controls = VBoxContainer.new()
	_placing_controls.add_theme_constant_override("separation", 6)
	_placing_controls.visible = false
	vbox.add_child(_placing_controls)

	# Width row: Label "Width:" + [-] + value label + [+]
	_width_row = HBoxContainer.new()
	var width_row := _width_row
	width_row.add_theme_constant_override("separation", 4)
	_placing_controls.add_child(width_row)

	var width_text := Label.new()
	width_text.text = "Width:"
	width_text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	width_row.add_child(width_text)

	var width_minus := Button.new()
	width_minus.text = "-"
	width_minus.custom_minimum_size = Vector2(30, 0)
	width_minus.pressed.connect(_width_decrease)
	width_row.add_child(width_minus)

	_width_label = Label.new()
	_width_label.text = "1"
	_width_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_width_label.custom_minimum_size = Vector2(24, 0)
	width_row.add_child(_width_label)

	var width_plus := Button.new()
	width_plus.text = "+"
	width_plus.custom_minimum_size = Vector2(30, 0)
	width_plus.pressed.connect(_width_increase)
	width_row.add_child(width_plus)

	# Depth row: Label "Depth:" + [-] + value label + [+]
	_depth_row = HBoxContainer.new()
	var depth_row := _depth_row
	depth_row.add_theme_constant_override("separation", 4)
	_placing_controls.add_child(depth_row)

	var depth_text := Label.new()
	depth_text.text = "Depth:"
	depth_text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	depth_row.add_child(depth_text)

	var depth_minus := Button.new()
	depth_minus.text = "-"
	depth_minus.custom_minimum_size = Vector2(30, 0)
	depth_minus.pressed.connect(_depth_decrease)
	depth_row.add_child(depth_minus)

	_depth_label = Label.new()
	_depth_label.text = "1"
	_depth_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_depth_label.custom_minimum_size = Vector2(24, 0)
	depth_row.add_child(_depth_label)

	var depth_plus := Button.new()
	depth_plus.text = "+"
	depth_plus.custom_minimum_size = Vector2(30, 0)
	depth_plus.pressed.connect(_depth_increase)
	depth_row.add_child(depth_plus)

	# Height row (building mode only): Label "Height:" + [-] + value label + [+]
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

	# Orientation row (ladder mode only): "Face:" + label + [Rotate R] button.
	_orientation_row = HBoxContainer.new()
	_orientation_row.add_theme_constant_override("separation", 4)
	_orientation_row.visible = false
	_placing_controls.add_child(_orientation_row)

	var orient_text := Label.new()
	orient_text.text = "Face:"
	orient_text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_orientation_row.add_child(orient_text)

	_orientation_label = Label.new()
	_orientation_label.text = ORIENTATION_NAMES.get(0, "East (+X)")
	_orientation_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_orientation_label.custom_minimum_size = Vector2(90, 0)
	_orientation_row.add_child(_orientation_label)

	var rotate_btn := Button.new()
	rotate_btn.text = "R"
	rotate_btn.custom_minimum_size = Vector2(30, 0)
	rotate_btn.pressed.connect(_rotate_ladder)
	_orientation_row.add_child(rotate_btn)

	# Kind row (ladder mode only): "Type:" + [Wood] + [Rope] toggle buttons.
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

	# Construct button.
	_construct_btn = Button.new()
	_construct_btn.text = "Construct"
	_construct_btn.disabled = true
	_construct_btn.pressed.connect(_confirm_placement)
	_placing_controls.add_child(_construct_btn)

	# Build validation message label (shown temporarily after placement).
	_message_label = Label.new()
	_message_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_message_label.add_theme_font_size_override("font_size", 14)
	_message_label.add_theme_color_override("font_color", Color(1.0, 0.85, 0.3))
	_message_label.visible = false
	_placing_controls.add_child(_message_label)

	_panel.visible = false


## Create the ghost preview mesh (translucent cube, hidden by default).
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


func _on_action_requested(action_name: String) -> void:
	if action_name != "Build":
		return
	if _active:
		_deactivate()
	else:
		_activate()


func _activate() -> void:
	_active = true
	_panel.visible = true
	if _camera_pivot and _camera_pivot.has_method("set_voxel_snap"):
		_camera_pivot.set_voxel_snap(true)
	construction_mode_entered.emit()


func _deactivate() -> void:
	_exit_placing()
	_active = false
	_panel.visible = false
	if _camera_pivot and _camera_pivot.has_method("set_voxel_snap"):
		_camera_pivot.set_voxel_snap(false)
	construction_mode_exited.emit()


func _enter_placing_platform() -> void:
	_enter_placing("platform")


func _enter_placing_building() -> void:
	_enter_placing("building")


func _enter_placing_ladder() -> void:
	_enter_placing("ladder")


func _enter_placing_carve() -> void:
	_enter_placing("carve")


func _enter_placing(mode: String) -> void:
	_build_mode = mode
	_placing = true
	_ghost.visible = true
	_placing_controls.visible = true
	if _build_mode == "building":
		_width_row.visible = true
		_depth_row.visible = true
		_height_row.visible = true
		_orientation_row.visible = false
		_kind_row.visible = false
		_set_width(3)
		_set_depth(3)
		_set_height(1)
	elif _build_mode == "ladder":
		_width_row.visible = false
		_depth_row.visible = false
		_height_row.visible = true
		_orientation_row.visible = true
		_kind_row.visible = true
		_set_height(1)
		_ladder_orientation = 0
		_ladder_kind = 0
		_orientation_label.text = ORIENTATION_NAMES.get(0, "East (+X)")
		_kind_label.text = "[Wood]"
	elif _build_mode == "carve":
		_width_row.visible = true
		_depth_row.visible = true
		_height_row.visible = true
		_orientation_row.visible = false
		_kind_row.visible = false
		_set_width(1)
		_set_depth(1)
		_set_height(1)
	else:
		_width_row.visible = true
		_depth_row.visible = true
		_height_row.visible = false
		_orientation_row.visible = false
		_kind_row.visible = false
		_set_width(1)
		_set_depth(1)
	_construct_btn.text = "Carve" if _build_mode == "carve" else "Construct"
	_update_ghost_size()


func _exit_placing() -> void:
	_placing = false
	_ghost.visible = false
	_placing_controls.visible = false
	_width_row.visible = true
	_depth_row.visible = true
	_height_row.visible = false
	_orientation_row.visible = false
	_kind_row.visible = false
	_message_label.visible = false
	_message_timer = 0.0
	_width = 1
	_depth = 1
	_height = 1
	_width_label.text = "1"
	_depth_label.text = "1"
	_height_label.text = "1"
	_ladder_orientation = 0
	_ladder_kind = 0
	_validation_tier = "Ok"
	_last_preview_voxel = Vector3i(999999, 999999, 999999)
	_last_preview_width = -1
	_last_preview_depth = -1
	_last_preview_height = -1
	_last_preview_orientation = -1
	_last_preview_kind = -1


func _width_decrease() -> void:
	_set_width(_width - 1)


func _width_increase() -> void:
	_set_width(_width + 1)


func _depth_decrease() -> void:
	_set_depth(_depth - 1)


func _depth_increase() -> void:
	_set_depth(_depth + 1)


func _set_width(value: int) -> void:
	var min_val := 3 if _build_mode == "building" else 1
	_width = clampi(value, min_val, 10)
	_width_label.text = str(_width)
	_update_ghost_size()


func _set_depth(value: int) -> void:
	var min_val := 3 if _build_mode == "building" else 1
	_depth = clampi(value, min_val, 10)
	_depth_label.text = str(_depth)
	_update_ghost_size()


func _height_decrease() -> void:
	_set_height(_height - 1)


func _height_increase() -> void:
	_set_height(_height + 1)


func _set_height(value: int) -> void:
	var max_h := 10 if _build_mode == "ladder" else 5
	_height = clampi(value, 1, max_h)
	_height_label.text = str(_height)
	_update_ghost_size()


func _rotate_ladder() -> void:
	var idx := LADDER_ORIENTATIONS.find(_ladder_orientation)
	idx = (idx + 1) % LADDER_ORIENTATIONS.size()
	_ladder_orientation = LADDER_ORIENTATIONS[idx]
	_orientation_label.text = ORIENTATION_NAMES.get(_ladder_orientation, "?")
	_update_ghost_size()


func _set_ladder_wood() -> void:
	_ladder_kind = 0
	_kind_label.text = "[Wood]"
	# Invalidate preview cache so validation re-runs.
	_last_preview_kind = -1


func _set_ladder_rope() -> void:
	_ladder_kind = 1
	_kind_label.text = "[Rope]"
	_last_preview_kind = -1


func _update_ghost_size() -> void:
	if _ghost and _ghost.mesh:
		if _build_mode == "ladder":
			# Ladder ghost: thin panel (0.9 wide, height tall, 0.05 thick).
			# The thin axis (Z) gets rotated to face the selected orientation.
			_ghost.mesh.size = Vector3(0.9, _height, 0.05)
			_ghost.basis = _face_rotations[_ladder_orientation]
		elif _build_mode == "building":
			# Building ghost: width x (height + 1 for foundation) x depth.
			_ghost.mesh.size = Vector3(_width, _height + 1, _depth)
			_ghost.basis = Basis.IDENTITY
		elif _build_mode == "carve":
			_ghost.mesh.size = Vector3(_width, _height, _depth)
			_ghost.basis = Basis.IDENTITY
		else:
			_ghost.mesh.size = Vector3(_width, 1.0, _depth)
			_ghost.basis = Basis.IDENTITY


## Compute the min-corner of the rectangle from the focus voxel (center).
func _get_min_corner() -> Vector3i:
	var min_x: int = _focus_voxel.x - (_width - 1) / 2
	var min_z: int = _focus_voxel.z - (_depth - 1) / 2
	return Vector3i(min_x, _focus_voxel.y, min_z)


func _process(_delta: float) -> void:
	# Fade out post-confirm messages after timeout.
	if _message_timer > 0.0:
		_message_timer -= _delta
		if _message_timer <= 0.0:
			_message_label.visible = false

	if not _placing:
		return
	if not _camera_pivot or not _camera_pivot.has_method("get_focus_voxel"):
		return

	var voxel: Vector3 = _camera_pivot.get_focus_voxel()
	_focus_voxel = Vector3i(int(voxel.x), int(voxel.y), int(voxel.z))

	# Position the ghost mesh based on mode.
	var min_corner := _get_min_corner()
	if _build_mode == "ladder":
		# Ladder ghost: thin panel column at focus voxel, offset to face.
		var face_offset := DIRECTION_OFFSETS[_ladder_orientation] * (0.5 - FACE_INSET)
		_ghost.global_position = Vector3(
			_focus_voxel.x + 0.5 + face_offset.x,
			_focus_voxel.y + _height / 2.0,
			_focus_voxel.z + 0.5 + face_offset.z,
		)
	elif _build_mode == "building":
		# Building ghost includes foundation row below + height rooms above.
		var ghost_h := _height + 1
		_ghost.global_position = Vector3(
			min_corner.x + _width / 2.0,
			_focus_voxel.y + ghost_h / 2.0,
			min_corner.z + _depth / 2.0,
		)
	elif _build_mode == "carve":
		_ghost.global_position = Vector3(
			min_corner.x + _width / 2.0,
			_focus_voxel.y + _height / 2.0,
			min_corner.z + _depth / 2.0,
		)
	else:
		_ghost.global_position = Vector3(
			min_corner.x + _width / 2.0,
			_focus_voxel.y + 0.5,
			min_corner.z + _depth / 2.0,
		)

	# Only re-validate when relevant inputs change.
	var needs_revalidate := (
		min_corner != _last_preview_voxel
		or _width != _last_preview_width
		or _depth != _last_preview_depth
		or _height != _last_preview_height
		or _ladder_orientation != _last_preview_orientation
		or _ladder_kind != _last_preview_kind
	)
	if needs_revalidate:
		_last_preview_voxel = min_corner
		_last_preview_width = _width
		_last_preview_depth = _depth
		_last_preview_height = _height
		_last_preview_orientation = _ladder_orientation
		_last_preview_kind = _ladder_kind

		var result: Dictionary
		if _build_mode == "ladder":
			result = _bridge.validate_ladder_preview(
				_focus_voxel.x,
				_focus_voxel.y,
				_focus_voxel.z,
				_height,
				_ladder_orientation,
				_ladder_kind
			)
		elif _build_mode == "building":
			result = _bridge.validate_building_preview(
				min_corner.x, _focus_voxel.y, min_corner.z, _width, _depth, _height
			)
		elif _build_mode == "carve":
			result = _bridge.validate_carve_preview(
				min_corner.x, min_corner.y, min_corner.z, _width, _depth, _height
			)
		else:
			result = _bridge.validate_platform_preview(
				min_corner.x, min_corner.y, min_corner.z, _width, _depth
			)
		_validation_tier = result.get("tier", "Blocked")
		_focus_valid = _validation_tier != "Blocked"

		# Show/hide the preview validation message.
		var msg: String = result.get("message", "")
		if _validation_tier == "Ok" or msg == "":
			# Only hide if not showing a post-confirm message.
			if _message_timer <= 0.0:
				_message_label.visible = false
		else:
			_message_label.text = msg
			_message_label.visible = true
			# Override any post-confirm timer while hovering.
			_message_timer = 0.0
			if _validation_tier == "Blocked":
				_message_label.add_theme_color_override("font_color", Color(1.0, 0.4, 0.4))
			else:
				_message_label.add_theme_color_override("font_color", Color(1.0, 0.85, 0.3))

	_construct_btn.disabled = not _focus_valid
	if _validation_tier == "Ok":
		if _build_mode == "carve":
			_ghost_material.albedo_color = Color(0.9, 0.4, 0.2, 0.4)
		else:
			_ghost_material.albedo_color = Color(0.3, 0.5, 1.0, 0.4)
	elif _validation_tier == "Warning":
		_ghost_material.albedo_color = Color(1.0, 0.85, 0.3, 0.4)
	else:
		_ghost_material.albedo_color = Color(1.0, 0.2, 0.2, 0.4)


func _unhandled_input(event: InputEvent) -> void:
	if not _active:
		return

	if event is InputEventKey:
		var key := event as InputEventKey
		if not key.pressed:
			return

		# Dispatch based on placing state.
		if not _placing:
			# Mode-entry shortcuts: P, G, L, C, ESC.
			if key.keycode == KEY_P:
				_enter_placing_platform()
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_G:
				_enter_placing_building()
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_L:
				_enter_placing_ladder()
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_C:
				_enter_placing_carve()
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_ESCAPE:
				_deactivate()
				get_viewport().set_input_as_handled()
		else:
			# While placing: ESC, Enter.
			if key.keycode == KEY_ESCAPE:
				_exit_placing()
				get_viewport().set_input_as_handled()
			elif key.keycode == KEY_ENTER and _focus_valid:
				_confirm_placement()
				get_viewport().set_input_as_handled()

	# Left-click to confirm, right-click to cancel (while placing).
	if _placing and event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.pressed:
			if mb.button_index == MOUSE_BUTTON_LEFT and _focus_valid:
				_confirm_placement()
				get_viewport().set_input_as_handled()
			elif mb.button_index == MOUSE_BUTTON_RIGHT:
				_exit_placing()
				get_viewport().set_input_as_handled()


func _confirm_placement() -> void:
	var min_corner := _get_min_corner()
	var msg: String
	if _build_mode == "ladder":
		msg = _bridge.designate_ladder(
			_focus_voxel.x,
			_focus_voxel.y,
			_focus_voxel.z,
			_height,
			_ladder_orientation,
			_ladder_kind
		)
	elif _build_mode == "building":
		msg = _bridge.designate_building(
			min_corner.x, _focus_voxel.y, min_corner.z, _width, _depth, _height
		)
	elif _build_mode == "carve":
		msg = _bridge.designate_carve(
			min_corner.x, min_corner.y, min_corner.z, _width, _depth, _height
		)
	else:
		msg = _bridge.designate_build_rect(min_corner.x, min_corner.y, min_corner.z, _width, _depth)
	if msg != "":
		_show_build_message(msg)
	# Invalidate preview cache so the next frame re-validates against
	# the changed world state.
	_last_preview_voxel = Vector3i(999999, 999999, 999999)
	blueprint_placed.emit()


func _show_build_message(msg: String) -> void:
	_message_label.text = msg
	_message_label.visible = true
	_message_timer = 3.0
