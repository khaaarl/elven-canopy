## Handles click-to-select, box-select, and right-click commands for creatures,
## structures, and ground piles.
##
## Supports single-click selection (ray-cast to closest creature sprite),
## click-and-drag box selection (2D rectangle selecting all creatures whose
## screen-space positions fall inside), and Shift modifier for additive
## selection. Creatures are identified by stable CreatureId strings (UUID),
## not ephemeral per-species indices.
##
## On right-click (when a creature is selected), issues context-sensitive
## commands: attack if the target is hostile, move-to if it's a friendly
## creature or ground location. Uses bridge.is_hostile_by_id(),
## bridge.attack_creature(), and bridge.group_directed_goto() with UUID strings.
## Multi-creature moves use group commands (group_directed_goto,
## group_attack_move) that spread destinations across nearby nav nodes so
## creatures don't stack on the same voxel.
##
## F key toggles attack-move mode: the next left-click dispatches a
## GroupAttackMove command (walk to destination, fight hostiles en route) for
## all selected creatures. ESC or F again cancels the mode.
##
## Selection state: tracks a set of selected creature IDs (stable UUIDs), or
## a single structure_id, or a single ground pile position. Selecting creatures
## deselects structures/piles and vice versa. ESC deselects all.
##
## Box selection draws a translucent rectangle overlay via a CanvasLayer
## ColorRect during the drag.
##
## Uses a data-driven SPECIES_Y_OFFSETS dict so adding new species doesn't
## require code changes here — just add the entry.
##
## See also: tooltip_controller.gd for hover tooltips (shares the same
## ray-cast pattern, SPECIES_Y_OFFSETS, and SNAP_THRESHOLD),
## creature_info_panel.gd for the creature UI panel,
## structure_info_panel.gd for the structure UI panel,
## ground_pile_info_panel.gd for the pile UI panel, orbital_camera.gd
## for follow mode, placement_controller.gd for the ray-snap algorithm
## origin, construction_controller.gd for construction placement suppression,
## elf_renderer.gd / capybara_renderer.gd / creature_renderer.gd for
## sprite position offsets.

extends Node3D

## Emitted when one or more creatures are selected. The ids array contains
## stable CreatureId strings (UUIDs). For single-click, the array has one
## element. For box-select, it may have many.
signal creatures_selected(ids: Array)
signal creature_deselected
signal structure_selected(structure_id: int)
signal structure_deselected
signal pile_selected(x: int, y: int, z: int)
signal pile_deselected

## Maximum perpendicular distance (world units) from the mouse ray to a
## creature sprite center for it to count as a click hit. Tighter than
## placement_controller's 5.0 since sprites are small.
const SNAP_THRESHOLD := 1.5

## Minimum drag distance (pixels) before a click becomes a box-select drag.
const DRAG_THRESHOLD := 5.0

## Y offsets per species — must match the renderers.
const SPECIES_Y_OFFSETS = {
	"Elf": 0.48,
	"Capybara": 0.32,
	"Boar": 0.38,
	"Deer": 0.46,
	"Elephant": 0.8,
	"Goblin": 0.36,
	"Monkey": 0.44,
	"Orc": 0.48,
	"Squirrel": 0.28,
	"Troll": 0.8,
}

var _bridge: SimBridge
var _camera: Camera3D
var _placement_controller: Node3D
var _construction_controller: Node
var _render_tick: float = 0.0

## Currently selected creature IDs (stable UUID strings). Empty = no selection.
var _selected_creature_ids: Array = []
var _selected_structure_id: int = -1
var _selected_pile_pos: Vector3i = Vector3i(-1, -1, -1)

## Box selection drag state.
var _drag_active: bool = false
var _drag_start: Vector2 = Vector2.ZERO
var _box_rect: ColorRect = null
var _box_layer: CanvasLayer = null

## When true, ignore the next mouse-button release. Set by select_creature_by_id()
## to prevent programmatic selections (e.g., from group panel clicks) from being
## immediately undone by the release event falling through to _try_select().
var _ignore_next_release: bool = false

## When true, the next left-click dispatches attack-move instead of selection.
## Toggled by pressing F with creatures selected; cancelled by ESC or F again.
var _attack_move_mode: bool = false


func setup(bridge: SimBridge, camera: Camera3D) -> void:
	_bridge = bridge
	_camera = camera
	_setup_box_overlay()


## Set the fractional render tick for smooth movement interpolation.
## Called by main.gd each frame after stepping the sim.
func set_render_tick(tick: float) -> void:
	_render_tick = tick


func set_placement_controller(controller: Node3D) -> void:
	_placement_controller = controller


func set_construction_controller(controller: Node) -> void:
	_construction_controller = controller


## Return the single selected creature ID, or "" if none or multiple selected.
func get_selected_creature_id() -> String:
	if _selected_creature_ids.size() == 1:
		return _selected_creature_ids[0]
	return ""


## Return all selected creature IDs.
func get_selected_creature_ids() -> Array:
	return _selected_creature_ids


## Return true if any creatures are selected.
func has_creature_selection() -> bool:
	return not _selected_creature_ids.is_empty()


func get_selected_structure_id() -> int:
	return _selected_structure_id


func get_selected_pile_pos() -> Vector3i:
	return _selected_pile_pos


## Remove a creature ID from the selection (e.g., when it dies).
## Emits appropriate signals if the selection changes.
func remove_creature_id(creature_id: String) -> void:
	var idx := _selected_creature_ids.find(creature_id)
	if idx >= 0:
		_selected_creature_ids.remove_at(idx)
		if _selected_creature_ids.is_empty():
			creature_deselected.emit()
		else:
			creatures_selected.emit(_selected_creature_ids)


## Programmatically select a creature by its stable ID.
func select_creature_by_id(creature_id: String) -> void:
	_deselect_structure_only()
	_deselect_pile_only()
	_selected_creature_ids = [creature_id]
	creatures_selected.emit(_selected_creature_ids)
	_ignore_next_release = true


## Programmatically select a structure by ID. Used by the structure list
## panel's zoom button to open the info panel alongside camera movement.
func select_structure(id: int) -> void:
	_deselect_creature_only()
	_deselect_pile_only()
	_selected_structure_id = id
	structure_selected.emit(id)


func deselect() -> void:
	_attack_move_mode = false
	if not _selected_creature_ids.is_empty():
		_selected_creature_ids = []
		creature_deselected.emit()
	if _selected_structure_id >= 0:
		_selected_structure_id = -1
		structure_deselected.emit()
	if _selected_pile_pos != Vector3i(-1, -1, -1):
		_selected_pile_pos = Vector3i(-1, -1, -1)
		pile_deselected.emit()


func _unhandled_input(event: InputEvent) -> void:
	# Don't process selection clicks during placement or construction mode.
	if _placement_controller and _placement_controller.is_placing():
		_attack_move_mode = false
		return
	if _construction_controller and _construction_controller.is_placing():
		_attack_move_mode = false
		return

	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_LEFT:
			if mb.pressed:
				if _attack_move_mode:
					_execute_attack_move(mb.position)
					_attack_move_mode = false
					_ignore_next_release = true
					get_viewport().set_input_as_handled()
					return
				_drag_start = mb.position
				_drag_active = false
			else:
				# Mouse released — either complete a box-select or do a click.
				if _ignore_next_release:
					_ignore_next_release = false
					return
				if _drag_active:
					_finish_box_select(mb.position, mb.shift_pressed)
					_drag_active = false
					_box_rect.visible = false
					get_viewport().set_input_as_handled()
				else:
					_try_select(mb.position, mb.shift_pressed)
		elif mb.pressed and mb.button_index == MOUSE_BUTTON_RIGHT:
			if _attack_move_mode:
				_execute_attack_move(mb.position)
				_attack_move_mode = false
				get_viewport().set_input_as_handled()
			else:
				_try_right_click_command(mb.position)

	if event is InputEventMouseMotion and Input.is_mouse_button_pressed(MOUSE_BUTTON_LEFT):
		var mm := event as InputEventMouseMotion
		var dist := mm.position.distance_to(_drag_start)
		if dist >= DRAG_THRESHOLD:
			if not _drag_active:
				_drag_active = true
			_update_box_rect(mm.position)

	if event is InputEventKey:
		var key := event as InputEventKey
		if key.pressed and key.keycode == KEY_F and has_creature_selection():
			_attack_move_mode = not _attack_move_mode
			get_viewport().set_input_as_handled()
		elif key.pressed and key.keycode == KEY_ESCAPE and _attack_move_mode:
			_attack_move_mode = false
			get_viewport().set_input_as_handled()
		elif (
			key.pressed
			and key.keycode == KEY_ESCAPE
			and (
				not _selected_creature_ids.is_empty()
				or _selected_structure_id >= 0
				or _selected_pile_pos != Vector3i(-1, -1, -1)
			)
		):
			deselect()
			get_viewport().set_input_as_handled()


func _try_select(mouse_pos: Vector2, shift: bool) -> void:
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	# First, try to select a creature (closest sprite within snap threshold).
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var best_id := ""

	for species_name in SPECIES_Y_OFFSETS:
		var data := _bridge.get_creature_positions_with_ids(species_name, _render_tick)
		var ids: Array = data.get("ids", [])
		var positions: PackedVector3Array = data.get("positions", PackedVector3Array())
		var y_off: float = SPECIES_Y_OFFSETS[species_name]
		for i in positions.size():
			var pos := positions[i]
			var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
			var dist_sq := _point_to_ray_dist_sq(world_pos, ray_origin, ray_dir)
			if dist_sq < best_dist_sq:
				best_dist_sq = dist_sq
				best_id = ids[i]

	if best_id != "":
		_deselect_structure_only()
		_deselect_pile_only()
		if shift:
			# Toggle: add if not present, remove if already selected.
			var idx := _selected_creature_ids.find(best_id)
			if idx >= 0:
				_selected_creature_ids.remove_at(idx)
				if _selected_creature_ids.is_empty():
					creature_deselected.emit()
				else:
					creatures_selected.emit(_selected_creature_ids)
			else:
				_selected_creature_ids.append(best_id)
				creatures_selected.emit(_selected_creature_ids)
		else:
			_selected_creature_ids = [best_id]
			creatures_selected.emit(_selected_creature_ids)
		get_viewport().set_input_as_handled()
		return

	# No creature hit — try structure raycast.
	var sid := _bridge.raycast_structure(ray_origin, ray_dir)
	if sid >= 0:
		_deselect_creature_only()
		_deselect_pile_only()
		_selected_structure_id = sid
		structure_selected.emit(sid)
		get_viewport().set_input_as_handled()
		return

	# No structure hit — try ground piles (point-to-ray distance check).
	var piles := _bridge.get_ground_piles()
	var pile_best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var pile_best_pos := Vector3i(-1, -1, -1)
	for pile_entry in piles:
		var px: int = pile_entry.get("x", 0)
		var py: int = pile_entry.get("y", 0)
		var pz: int = pile_entry.get("z", 0)
		# Pile center: offset +0.5 in x/z, +0.1 in y (half of ~0.2 box height).
		var pile_world := Vector3(px + 0.5, py + 0.1, pz + 0.5)
		var pdist_sq := _point_to_ray_dist_sq(pile_world, ray_origin, ray_dir)
		if pdist_sq < pile_best_dist_sq:
			pile_best_dist_sq = pdist_sq
			pile_best_pos = Vector3i(px, py, pz)

	if pile_best_pos != Vector3i(-1, -1, -1):
		_deselect_creature_only()
		_deselect_structure_only()
		_selected_pile_pos = pile_best_pos
		pile_selected.emit(pile_best_pos.x, pile_best_pos.y, pile_best_pos.z)
		get_viewport().set_input_as_handled()
		return

	# Clicked on nothing — deselect whatever was active (unless Shift held).
	if not shift:
		if (
			not _selected_creature_ids.is_empty()
			or _selected_structure_id >= 0
			or _selected_pile_pos != Vector3i(-1, -1, -1)
		):
			deselect()


## Complete a box-select drag. Projects all creature world positions to screen
## space and selects those inside the rectangle. Prefers player-civ creatures:
## if the box contains any, only those are selected (RTS convention).
func _finish_box_select(end_pos: Vector2, shift: bool) -> void:
	var rect := _make_screen_rect(_drag_start, end_pos)
	var player_ids: Array = []
	var all_ids: Array = []

	for species_name in SPECIES_Y_OFFSETS:
		var data := _bridge.get_creature_positions_with_ids(species_name, _render_tick)
		var ids: Array = data.get("ids", [])
		var positions: PackedVector3Array = data.get("positions", PackedVector3Array())
		var civ_flags: Array = data.get("is_player_civ", [])
		var y_off: float = SPECIES_Y_OFFSETS[species_name]
		for i in positions.size():
			var pos := positions[i]
			var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
			if not _camera.is_position_behind(world_pos):
				var screen_pos := _camera.unproject_position(world_pos)
				if rect.has_point(screen_pos):
					all_ids.append(ids[i])
					if i < civ_flags.size() and civ_flags[i]:
						player_ids.append(ids[i])

	# Prefer player-civ creatures; fall back to all if none are player-owned.
	var new_ids: Array = player_ids if not player_ids.is_empty() else all_ids

	if new_ids.is_empty():
		if not shift:
			deselect()
		return

	_deselect_structure_only()
	_deselect_pile_only()

	if shift:
		# Additive: merge new IDs into existing selection (no duplicates).
		for cid in new_ids:
			if _selected_creature_ids.find(cid) < 0:
				_selected_creature_ids.append(cid)
	else:
		_selected_creature_ids = new_ids

	creatures_selected.emit(_selected_creature_ids)


## Clear creature selection without touching structure/pile state. Emits
## creature_deselected so main.gd can hide the creature info panel.
func _deselect_creature_only() -> void:
	if not _selected_creature_ids.is_empty():
		_selected_creature_ids = []
		creature_deselected.emit()


## Clear structure selection without touching creature/pile state. Emits
## structure_deselected so main.gd can hide the structure info panel.
func _deselect_structure_only() -> void:
	if _selected_structure_id >= 0:
		_selected_structure_id = -1
		structure_deselected.emit()


## Clear pile selection without touching creature/structure state. Emits
## pile_deselected so main.gd can hide the pile info panel.
func _deselect_pile_only() -> void:
	if _selected_pile_pos != Vector3i(-1, -1, -1):
		_selected_pile_pos = Vector3i(-1, -1, -1)
		pile_deselected.emit()


## Perpendicular distance squared from a point to an infinite ray.
## Clamps t >= 0 so points behind the camera are handled correctly.
func _point_to_ray_dist_sq(point: Vector3, ray_origin: Vector3, ray_dir: Vector3) -> float:
	var to_point := point - ray_origin
	var t := maxf(0.0, to_point.dot(ray_dir))
	var closest := ray_origin + ray_dir * t
	return (point - closest).length_squared()


## Right-click command: if a creature is selected, right-clicking on the world
## issues a context-sensitive command (attack hostile, move to ground). Uses
## UUID-based creature IDs from the stable selection system. When multiple
## creatures are selected, commands are issued to all of them.
func _try_right_click_command(mouse_pos: Vector2) -> void:
	# Only works when creatures are selected.
	if _selected_creature_ids.is_empty():
		return

	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	# Check if we clicked on a creature (potential attack target).
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var target_id := ""
	var target_pos := Vector3.ZERO

	for species_name in SPECIES_Y_OFFSETS:
		var data := _bridge.get_creature_positions_with_ids(species_name, _render_tick)
		var ids: Array = data.get("ids", [])
		var positions: PackedVector3Array = data.get("positions", PackedVector3Array())
		var y_off: float = SPECIES_Y_OFFSETS[species_name]
		for i in positions.size():
			var pos := positions[i]
			var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
			var dist_sq := _point_to_ray_dist_sq(world_pos, ray_origin, ray_dir)
			if dist_sq < best_dist_sq:
				best_dist_sq = dist_sq
				target_id = ids[i]
				target_pos = pos

	# If we clicked on a creature, issue commands to each selected creature.
	if target_id != "":
		var move_ids: Array = []
		for attacker_uuid in _selected_creature_ids:
			if attacker_uuid == target_id:
				continue
			if _bridge.is_hostile_by_id(attacker_uuid, target_id):
				_bridge.attack_creature(attacker_uuid, target_id)
			else:
				# Friendly creature — collect for group move.
				move_ids.append(attacker_uuid)
		if not move_ids.is_empty():
			_bridge.group_directed_goto(
				move_ids, int(target_pos.x), int(target_pos.y), int(target_pos.z)
			)
		get_viewport().set_input_as_handled()
		return

	# No creature clicked — snap to nearest nav node and issue directed goto.
	var cam_pos := _camera.global_position
	var nav_nodes := _bridge.get_visible_nav_nodes(cam_pos)
	var nav_best_dist_sq := 25.0  # 5.0 squared — generous threshold for ground clicks
	var nav_best_pos := Vector3.ZERO
	var nav_found := false

	for i in nav_nodes.size():
		var pos := nav_nodes[i]
		var to_pos := pos - ray_origin
		var t := maxf(0.0, to_pos.dot(ray_dir))
		var closest_on_ray := ray_origin + ray_dir * t
		var diff := pos - closest_on_ray
		var dist_sq := diff.length_squared()
		if dist_sq < nav_best_dist_sq:
			nav_best_dist_sq = dist_sq
			nav_best_pos = pos
			nav_found = true

	if nav_found:
		_bridge.group_directed_goto(
			_selected_creature_ids, int(nav_best_pos.x), int(nav_best_pos.y), int(nav_best_pos.z)
		)
		get_viewport().set_input_as_handled()


## Execute attack-move: dispatch a GroupAttackMove command for all selected
## creatures to the clicked location (ground or creature position).
func _execute_attack_move(mouse_pos: Vector2) -> void:
	if _selected_creature_ids.is_empty():
		return

	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	# Check if we clicked on a creature — use their position as the destination.
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var target_pos := Vector3.ZERO
	var found_creature := false

	for species_name in SPECIES_Y_OFFSETS:
		var data := _bridge.get_creature_positions_with_ids(species_name, _render_tick)
		var positions: PackedVector3Array = data.get("positions", PackedVector3Array())
		var y_off: float = SPECIES_Y_OFFSETS[species_name]
		for i in positions.size():
			var pos := positions[i]
			var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
			var dist_sq := _point_to_ray_dist_sq(world_pos, ray_origin, ray_dir)
			if dist_sq < best_dist_sq:
				best_dist_sq = dist_sq
				target_pos = pos
				found_creature = true

	if found_creature:
		_bridge.group_attack_move(
			_selected_creature_ids, int(target_pos.x), int(target_pos.y), int(target_pos.z)
		)
		return

	# No creature clicked — snap to nearest nav node.
	var cam_pos := _camera.global_position
	var nav_nodes := _bridge.get_visible_nav_nodes(cam_pos)
	var nav_best_dist_sq := 25.0
	var nav_best_pos := Vector3.ZERO
	var nav_found := false

	for i in nav_nodes.size():
		var pos := nav_nodes[i]
		var to_pos := pos - ray_origin
		var t := maxf(0.0, to_pos.dot(ray_dir))
		var closest_on_ray := ray_origin + ray_dir * t
		var diff := pos - closest_on_ray
		var dist_sq := diff.length_squared()
		if dist_sq < nav_best_dist_sq:
			nav_best_dist_sq = dist_sq
			nav_best_pos = pos
			nav_found = true

	if nav_found:
		_bridge.group_attack_move(
			_selected_creature_ids, int(nav_best_pos.x), int(nav_best_pos.y), int(nav_best_pos.z)
		)


## Create the CanvasLayer + ColorRect used for the box selection overlay.
func _setup_box_overlay() -> void:
	_box_layer = CanvasLayer.new()
	_box_layer.layer = 100
	add_child(_box_layer)

	_box_rect = ColorRect.new()
	_box_rect.color = Color(0.3, 0.6, 1.0, 0.2)
	_box_rect.visible = false
	_box_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_box_layer.add_child(_box_rect)


## Update the box selection rectangle to span from _drag_start to current_pos.
func _update_box_rect(current_pos: Vector2) -> void:
	var rect := _make_screen_rect(_drag_start, current_pos)
	_box_rect.position = rect.position
	_box_rect.size = rect.size
	_box_rect.visible = true


## Build a Rect2 from two corner points, handling any drag direction.
func _make_screen_rect(a: Vector2, b: Vector2) -> Rect2:
	var top_left := Vector2(minf(a.x, b.x), minf(a.y, b.y))
	var bottom_right := Vector2(maxf(a.x, b.x), maxf(a.y, b.y))
	return Rect2(top_left, bottom_right - top_left)
