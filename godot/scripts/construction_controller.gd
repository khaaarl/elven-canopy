## Construction mode controller with adjustable-size platform placement.
##
## Manages the construction mode lifecycle: toggling on/off, showing a
## right-side panel with build options, enabling voxel-snap on the orbital
## camera, and handling multi-voxel rectangular platform blueprint placement.
##
## State machine:
##   _active=false                  → INACTIVE (panel hidden, no ghost)
##   _active=true,  _placing=false  → ACTIVE   (panel shown, no ghost)
##   _active=true,  _placing=true   → PLACING  (panel shown, ghost visible)
##
## In PLACING mode, a translucent ghost rectangle follows the camera's focus
## voxel (centered). Width/Depth dimension controls (with +/- buttons) let
## the player size the rectangle from 1x1 up to 10x10. The ghost is blue
## when ALL voxels in the rectangle are valid (air + adjacent to solid) and
## red when ANY voxel is invalid. The Construct button (or Enter/left-click)
## confirms placement at valid positions; ESC/right-click cancels back to
## ACTIVE mode and resets dimensions to 1x1.
##
## Input handling: ESC exits the current sub-mode first (PLACING → ACTIVE),
## then exits construction mode entirely (ACTIVE → INACTIVE). This sits
## between placement_controller.gd and selection_controller.gd in the ESC
## precedence chain (see main.gd docstring for the full chain).
##
## See also: spawn_toolbar.gd which emits the "Build" action,
## orbital_camera.gd which provides set_voxel_snap() / get_focus_voxel(),
## main.gd which wires this controller into the scene,
## blueprint_renderer.gd for rendering designated blueprints,
## sim_bridge.rs for designate_build_rect() which handles the multi-voxel
## command, placement_controller.gd for the ESC precedence pattern.

extends Node

signal construction_mode_entered
signal construction_mode_exited
signal blueprint_placed

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

## Platform dimensions (X and Z axes), range [1, 10].
var _width: int = 1
var _depth: int = 1

## UI references for dimension controls (created in _build_panel).
var _placing_controls: VBoxContainer
var _width_label: Label
var _depth_label: Label
var _construct_btn: Button


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
	_build_panel()
	_build_ghost()


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
	platform_btn.pressed.connect(_enter_placing)
	vbox.add_child(platform_btn)

	# Placing controls (dimension spinners + Construct button).
	# Hidden by default, shown when entering PLACING mode.
	_placing_controls = VBoxContainer.new()
	_placing_controls.add_theme_constant_override("separation", 6)
	_placing_controls.visible = false
	vbox.add_child(_placing_controls)

	# Width row: Label "Width:" + [-] + value label + [+]
	var width_row := HBoxContainer.new()
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
	var depth_row := HBoxContainer.new()
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

	# Construct button.
	_construct_btn = Button.new()
	_construct_btn.text = "Construct"
	_construct_btn.disabled = true
	_construct_btn.pressed.connect(_confirm_placement)
	_placing_controls.add_child(_construct_btn)

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


func _enter_placing() -> void:
	_placing = true
	_ghost.visible = true
	_placing_controls.visible = true
	_update_ghost_size()


func _exit_placing() -> void:
	_placing = false
	_ghost.visible = false
	_placing_controls.visible = false
	_width = 1
	_depth = 1
	_width_label.text = "1"
	_depth_label.text = "1"


func _width_decrease() -> void:
	_set_width(_width - 1)

func _width_increase() -> void:
	_set_width(_width + 1)

func _depth_decrease() -> void:
	_set_depth(_depth - 1)

func _depth_increase() -> void:
	_set_depth(_depth + 1)


func _set_width(value: int) -> void:
	_width = clampi(value, 1, 10)
	_width_label.text = str(_width)
	_update_ghost_size()


func _set_depth(value: int) -> void:
	_depth = clampi(value, 1, 10)
	_depth_label.text = str(_depth)
	_update_ghost_size()


func _update_ghost_size() -> void:
	if _ghost and _ghost.mesh:
		_ghost.mesh.size = Vector3(_width, 1.0, _depth)


## Compute the min-corner of the rectangle from the focus voxel (center).
func _get_min_corner() -> Vector3i:
	var min_x: int = _focus_voxel.x - (_width - 1) / 2
	var min_z: int = _focus_voxel.z - (_depth - 1) / 2
	return Vector3i(min_x, _focus_voxel.y, min_z)


func _process(_delta: float) -> void:
	if not _placing:
		return
	if not _camera_pivot or not _camera_pivot.has_method("get_focus_voxel"):
		return

	var voxel: Vector3 = _camera_pivot.get_focus_voxel()
	_focus_voxel = Vector3i(int(voxel.x), int(voxel.y), int(voxel.z))

	# Compute min-corner and position the ghost mesh centered on the rect.
	var min_corner := _get_min_corner()
	_ghost.global_position = Vector3(
		min_corner.x + _width / 2.0,
		_focus_voxel.y + 0.5,
		min_corner.z + _depth / 2.0
	)

	# Validate the rectangle: ALL voxels must be in-bounds + Air, and
	# AT LEAST ONE voxel must have a face-adjacent solid (tree contact).
	var all_air := true
	var any_adjacent := false
	for dx in range(_width):
		for dz in range(_depth):
			var vx: int = min_corner.x + dx
			var vz: int = min_corner.z + dz
			if not _bridge.validate_build_air(vx, min_corner.y, vz):
				all_air = false
				break
			if not any_adjacent and _bridge.has_solid_neighbor(vx, min_corner.y, vz):
				any_adjacent = true
		if not all_air:
			break

	_focus_valid = all_air and any_adjacent
	_construct_btn.disabled = not _focus_valid
	if _focus_valid:
		_ghost_material.albedo_color = Color(0.3, 0.5, 1.0, 0.4)
	else:
		_ghost_material.albedo_color = Color(1.0, 0.2, 0.2, 0.4)


func _unhandled_input(event: InputEvent) -> void:
	if not _active:
		return

	if event is InputEventKey:
		var key := event as InputEventKey
		if not key.pressed:
			return

		# P shortcut to enter placing mode (only when active but not placing).
		if key.keycode == KEY_P and not _placing:
			_enter_placing()
			get_viewport().set_input_as_handled()
			return

		# ESC: if placing, exit placing; if just active, exit construction.
		if key.keycode == KEY_ESCAPE:
			if _placing:
				_exit_placing()
			else:
				_deactivate()
			get_viewport().set_input_as_handled()
			return

		# Enter to confirm placement.
		if key.keycode == KEY_ENTER and _placing and _focus_valid:
			_confirm_placement()
			get_viewport().set_input_as_handled()
			return

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
	_bridge.designate_build_rect(
		min_corner.x, min_corner.y, min_corner.z, _width, _depth)
	blueprint_placed.emit()
