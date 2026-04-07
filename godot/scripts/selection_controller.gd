## Handles click-to-select, box-select, double-click-select, right-click
## commands, and SC2-style selection groups (Ctrl+1–9) for creatures,
## structures, and ground piles.
##
## Supports single-click selection (ray-cast to closest creature sprite),
## click-and-drag box selection (2D rectangle selecting all creatures whose
## screen-space positions fall inside), double-click group selection (selects
## all on-screen creatures in the same military group as the clicked creature),
## Shift modifier for additive selection, and Alt modifier for subtractive
## selection (remove from current selection without toggling). Alt also works
## when clicking a creature row in the units panel or group info panel — the
## programmatic select_creature_by_id() checks Alt state and removes instead of
## solo-selecting. Creatures are identified by stable CreatureId strings
## (UUID), not ephemeral per-species indices.
##
## Selection groups (F-selection-groups): Ctrl+1–9 saves the current selection
## as a numbered group. Shift+1–9 adds the current selection to the group.
## Pressing 1–9 (plain) recalls the group. Double-tapping 1–9 recalls the
## group and centers the camera on the group centroid. Groups can contain both
## creatures and structures. GDScript keeps an authoritative local copy for
## instant recall; mutations are also sent to the sim for persistence across
## save/load. On load, hydrate_selection_groups() fetches from the sim.
##
## Double-click group select (F-dblclick-select): double-clicking a player-civ
## creature selects all visible (on-screen, not behind camera) player-civ
## creatures in the same military group. Civilians (no military group) are
## treated as their own implicit group. Non-player creatures are not expanded.
## Shift+double-click adds the group to the existing selection. Uses
## GeometryUtils.matches_double_click_group() for the matching predicate.
##
## Roof click-shield: when the ray hits a building/enclosure roof voxel,
## creatures inside the building (below the roof Y level) are excluded from
## selection. Creatures standing on top of the roof remain selectable. If no
## creature above the roof is near the click, the building itself is selected.
## Pairs with F-bldg-transparency which lets the player hide roofs to reach
## the creatures inside.
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
## See also: tooltip_controller.gd for hover tooltips (shares the same
## ray-cast pattern and SNAP_THRESHOLD),
## creature_info_panel.gd for the creature UI panel,
## structure_info_panel.gd for the structure UI panel,
## ground_pile_info_panel.gd for the pile UI panel, orbital_camera.gd
## for follow mode, placement_controller.gd for the ray-snap algorithm
## origin, construction_controller.gd for construction placement suppression,
## creature_renderer.gd for sprite position offsets.

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
## Emitted when a selection group is double-tap recalled, requesting the camera
## center on the given world position (centroid of group members).
signal group_center_requested(position: Vector3)

const CreatureRenderer = preload("res://scripts/creature_renderer.gd")

## Maximum perpendicular distance (world units) from the mouse ray to a
## creature sprite center for it to count as a click hit. Tighter than
## placement_controller's 5.0 since sprites are small.
const SNAP_THRESHOLD := 1.5

## Minimum drag distance (pixels) before a click becomes a box-select drag.
const DRAG_THRESHOLD := 5.0

## Maximum time (seconds) between two presses of the same number key to count
## as a double-tap (recall + center camera).
const DOUBLE_TAP_THRESHOLD := 0.4

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

## When true, building roof voxels are skipped during raycasts so clicks pass
## through to creatures inside. Set by main.gd when the roof-hide toggle is active.
var _roofs_hidden: bool = false

## SC2-style selection groups (1–9). Keys are group numbers (int),
## values are Dictionaries with "creature_ids" (Array) and "structure_ids" (Array).
## This is the authoritative runtime copy — mutations also write to sim for persistence.
var _selection_groups: Dictionary = {}

## Double-tap detection for selection group recall. Tracks the last number key
## pressed and when, so a second press within the threshold centers the camera.
var _last_group_key: int = -1
var _last_group_key_time: float = 0.0


func set_roofs_hidden(hidden: bool) -> void:
	_roofs_hidden = hidden


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
## When Alt is held, removes the creature from the current multi-selection
## instead of solo-selecting it (mirrors Alt+click in the viewport).
func select_creature_by_id(creature_id: String) -> void:
	if Input.is_key_pressed(KEY_ALT):
		remove_creature_id(creature_id)
		_ignore_next_release = true
		return
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
					_execute_attack_move(mb.position, mb.shift_pressed)
					_attack_move_mode = false
					_ignore_next_release = true
					get_viewport().set_input_as_handled()
					return
				if mb.double_click:
					_try_double_click_select(mb.position, mb.shift_pressed)
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
					_finish_box_select(mb.position, mb.shift_pressed, mb.alt_pressed)
					_drag_active = false
					_box_rect.visible = false
					get_viewport().set_input_as_handled()
				else:
					_try_select(mb.position, mb.shift_pressed, mb.alt_pressed)
		elif mb.pressed and mb.button_index == MOUSE_BUTTON_RIGHT:
			if _attack_move_mode:
				_execute_attack_move(mb.position, mb.shift_pressed)
				_attack_move_mode = false
				get_viewport().set_input_as_handled()
			else:
				_try_right_click_command(mb.position, mb.shift_pressed)

	if event is InputEventMouseMotion and Input.is_mouse_button_pressed(MOUSE_BUTTON_LEFT):
		var mm := event as InputEventMouseMotion
		var dist := mm.position.distance_to(_drag_start)
		if dist >= DRAG_THRESHOLD:
			if not _drag_active:
				_drag_active = true
			_update_box_rect(mm.position)

	if event is InputEventKey:
		var key := event as InputEventKey
		if key.pressed and not key.echo:
			var group_num := _keycode_to_group_number(key.keycode)
			if group_num > 0:
				if key.ctrl_pressed:
					_save_selection_group(group_num)
				elif key.shift_pressed:
					_add_to_selection_group(group_num)
				else:
					_recall_selection_group(group_num)
				get_viewport().set_input_as_handled()
				return
		if (
			key.pressed
			and key.keycode == KEY_F
			and not key.ctrl_pressed
			and not key.alt_pressed
			and has_creature_selection()
		):
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


func _try_select(mouse_pos: Vector2, shift: bool, alt: bool) -> void:
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	# Check structure raycast first to detect roof hits. A building roof
	# acts as a click shield: creatures inside the building (below the roof)
	# are not selectable, but creatures on top of the roof still are.
	var struct_hit := _bridge.raycast_structure_detailed(ray_origin, ray_dir, _roofs_hidden)
	var hit_sid: int = struct_hit.get("sid", -1)
	var hit_is_roof: bool = struct_hit.get("is_roof", false)
	var roof_y: int = struct_hit.get("roof_y", -1)

	# Try to select a creature. When a roof was hit, only consider creatures
	# whose voxel Y position is at or above the roof — they're standing on
	# top of the building, not inside it.
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var best_id := ""

	var sel_data := _bridge.get_creature_selection_data(_render_tick)
	var sel_ids: PackedStringArray = sel_data.get("ids", PackedStringArray())
	var sel_species: PackedStringArray = sel_data.get("species", PackedStringArray())
	var sel_positions: PackedVector3Array = sel_data.get("positions", PackedVector3Array())
	for i in sel_positions.size():
		var pos := sel_positions[i]
		# Roof shield: skip creatures inside the building (below roof).
		if GeometryUtils.is_shielded_by_roof(int(pos.y), hit_is_roof, roof_y):
			continue
		var y_off: float = CreatureRenderer.SPECIES_Y_OFFSETS.get(
			sel_species[i], CreatureRenderer.DEFAULT_Y_OFFSET
		)
		var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
		var dist_sq := _point_to_ray_dist_sq(world_pos, ray_origin, ray_dir)
		if dist_sq < best_dist_sq:
			best_dist_sq = dist_sq
			best_id = sel_ids[i]

	if best_id != "":
		_deselect_structure_only()
		_deselect_pile_only()
		var result := SelectionUtils.apply_click_modifier(
			_selected_creature_ids, best_id, shift, alt
		)
		if result["changed"]:
			_selected_creature_ids = result["ids"]
			if _selected_creature_ids.is_empty():
				creature_deselected.emit()
			else:
				creatures_selected.emit(_selected_creature_ids)
		get_viewport().set_input_as_handled()
		return

	# No creature hit — select the structure if the raycast found one
	# (whether it was a roof hit or any other structure voxel).
	if hit_sid >= 0:
		_deselect_creature_only()
		_deselect_pile_only()
		_selected_structure_id = hit_sid
		structure_selected.emit(hit_sid)
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

	# Clicked on nothing — deselect whatever was active (unless Shift/Alt held).
	if not shift and not alt:
		if (
			not _selected_creature_ids.is_empty()
			or _selected_structure_id >= 0
			or _selected_pile_pos != Vector3i(-1, -1, -1)
		):
			deselect()


## Double-click group select: find the creature under the cursor, then select
## all on-screen player-civ creatures in the same military group. Civilians
## (military_group_id == -1) are treated as their own implicit group.
## Non-player creatures are not expanded — the click falls through to a
## normal single select.
func _try_double_click_select(mouse_pos: Vector2, shift: bool) -> void:
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)
	var viewport_rect := Rect2(Vector2.ZERO, get_viewport().get_visible_rect().size)

	# Roof shield: detect roof hits so creatures inside buildings are excluded.
	var struct_hit := _bridge.raycast_structure_detailed(ray_origin, ray_dir, _roofs_hidden)
	var hit_is_roof: bool = struct_hit.get("is_roof", false)
	var roof_y: int = struct_hit.get("roof_y", -1)

	# First pass: find the clicked creature AND collect all on-screen creatures.
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var best_id := ""
	var best_group_id: int = -1
	var best_is_player_civ: bool = false

	# Parallel arrays for all on-screen creatures.
	var screen_ids: Array = []
	var screen_group_ids: Array = []
	var screen_is_player_civ: Array = []

	var sel_data := _bridge.get_creature_selection_data(_render_tick)
	var sel_ids: PackedStringArray = sel_data.get("ids", PackedStringArray())
	var sel_species: PackedStringArray = sel_data.get("species", PackedStringArray())
	var sel_positions: PackedVector3Array = sel_data.get("positions", PackedVector3Array())
	var sel_civ_flags: PackedByteArray = sel_data.get("is_player_civ", PackedByteArray())
	var sel_group_ids: PackedInt32Array = sel_data.get("military_group_ids", PackedInt32Array())
	for i in sel_positions.size():
		var pos := sel_positions[i]
		var y_off: float = CreatureRenderer.SPECIES_Y_OFFSETS.get(
			sel_species[i], CreatureRenderer.DEFAULT_Y_OFFSET
		)
		var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)

		# Roof shield: skip creatures inside the building (below roof).
		if GeometryUtils.is_shielded_by_roof(int(pos.y), hit_is_roof, roof_y):
			continue

		# Check if this creature is the one being clicked.
		var dist_sq := _point_to_ray_dist_sq(world_pos, ray_origin, ray_dir)
		if dist_sq < best_dist_sq:
			best_dist_sq = dist_sq
			best_id = sel_ids[i]
			best_group_id = sel_group_ids[i]
			best_is_player_civ = sel_civ_flags[i] == 1

		# Collect on-screen creatures for the group filter pass.
		if not _camera.is_position_behind(world_pos):
			var screen_pos := _camera.unproject_position(world_pos)
			if viewport_rect.has_point(screen_pos):
				screen_ids.append(sel_ids[i])
				screen_group_ids.append(sel_group_ids[i])
				screen_is_player_civ.append(sel_civ_flags[i] == 1)

	# No creature under cursor — fall through (deselect handled by caller).
	if best_id == "":
		if not shift:
			deselect()
		return

	# Non-player creature — just do a normal single select.
	if not best_is_player_civ:
		_deselect_structure_only()
		_deselect_pile_only()
		if shift:
			if _selected_creature_ids.find(best_id) < 0:
				_selected_creature_ids.append(best_id)
		else:
			_selected_creature_ids = [best_id]
		creatures_selected.emit(_selected_creature_ids)
		return

	# Filter on-screen creatures to those matching the clicked creature's group.
	var new_ids: Array = []
	for i in screen_ids.size():
		if (
			GeometryUtils
			. matches_double_click_group(
				screen_group_ids[i],
				screen_is_player_civ[i],
				best_group_id,
				best_is_player_civ,
			)
		):
			new_ids.append(screen_ids[i])

	if new_ids.is_empty():
		return

	_deselect_structure_only()
	_deselect_pile_only()

	if shift:
		for cid in new_ids:
			if _selected_creature_ids.find(cid) < 0:
				_selected_creature_ids.append(cid)
	else:
		_selected_creature_ids = new_ids

	creatures_selected.emit(_selected_creature_ids)


## Complete a box-select drag. Projects all creature world positions to screen
## space and selects those inside the rectangle. Prefers player-civ creatures:
## if the box contains any, only those are selected (RTS convention).
func _finish_box_select(end_pos: Vector2, shift: bool, alt: bool) -> void:
	var rect := _make_screen_rect(_drag_start, end_pos)
	var player_ids: Array = []
	var all_ids: Array = []

	var sel_data := _bridge.get_creature_selection_data(_render_tick)
	var sel_ids: PackedStringArray = sel_data.get("ids", PackedStringArray())
	var sel_species: PackedStringArray = sel_data.get("species", PackedStringArray())
	var sel_positions: PackedVector3Array = sel_data.get("positions", PackedVector3Array())
	var sel_civ_flags: PackedByteArray = sel_data.get("is_player_civ", PackedByteArray())
	for i in sel_positions.size():
		var pos := sel_positions[i]
		var y_off: float = CreatureRenderer.SPECIES_Y_OFFSETS.get(
			sel_species[i], CreatureRenderer.DEFAULT_Y_OFFSET
		)
		var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
		if not _camera.is_position_behind(world_pos):
			var screen_pos := _camera.unproject_position(world_pos)
			if rect.has_point(screen_pos):
				all_ids.append(sel_ids[i])
				if sel_civ_flags[i] == 1:
					player_ids.append(sel_ids[i])

	# Prefer player-civ creatures; fall back to all if none are player-owned.
	var new_ids: Array = player_ids if not player_ids.is_empty() else all_ids

	if new_ids.is_empty():
		if not shift and not alt:
			deselect()
		return

	_deselect_structure_only()
	_deselect_pile_only()

	var result := SelectionUtils.apply_box_modifier(_selected_creature_ids, new_ids, shift, alt)
	_selected_creature_ids = result["ids"]
	if _selected_creature_ids.is_empty():
		creature_deselected.emit()
	else:
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
## Delegates to GeometryUtils (geometry_utils.gd) — the single source of truth.
func _point_to_ray_dist_sq(point: Vector3, ray_origin: Vector3, ray_dir: Vector3) -> float:
	return GeometryUtils.point_to_ray_dist_sq(point, ray_origin, ray_dir)


## Right-click command: if a creature is selected, right-clicking on the world
## issues a context-sensitive command (attack hostile, move to ground). Uses
## UUID-based creature IDs from the stable selection system. When multiple
## creatures are selected, commands are issued to all of them.
func _try_right_click_command(mouse_pos: Vector2, queue: bool) -> void:
	# Only works when creatures are selected.
	if _selected_creature_ids.is_empty():
		return

	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	# Check if we clicked on a creature (potential attack target).
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var target_id := ""
	var target_pos := Vector3.ZERO

	var sel_data := _bridge.get_creature_selection_data(_render_tick)
	var sel_ids: PackedStringArray = sel_data.get("ids", PackedStringArray())
	var sel_species: PackedStringArray = sel_data.get("species", PackedStringArray())
	var sel_positions: PackedVector3Array = sel_data.get("positions", PackedVector3Array())
	for i in sel_positions.size():
		var pos := sel_positions[i]
		var y_off: float = CreatureRenderer.SPECIES_Y_OFFSETS.get(
			sel_species[i], CreatureRenderer.DEFAULT_Y_OFFSET
		)
		var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
		var dist_sq := _point_to_ray_dist_sq(world_pos, ray_origin, ray_dir)
		if dist_sq < best_dist_sq:
			best_dist_sq = dist_sq
			target_id = sel_ids[i]
			target_pos = pos

	# If we clicked on a creature, issue commands to each selected creature.
	if target_id != "":
		var move_ids: Array = []
		for attacker_uuid in _selected_creature_ids:
			if attacker_uuid == target_id:
				continue
			if _bridge.is_hostile_by_id(attacker_uuid, target_id):
				_bridge.attack_creature(attacker_uuid, target_id, queue)
			else:
				# Friendly creature — collect for group move.
				move_ids.append(attacker_uuid)
		if not move_ids.is_empty():
			_bridge.group_directed_goto(
				move_ids, int(target_pos.x), int(target_pos.y), int(target_pos.z), queue
			)
		get_viewport().set_input_as_handled()
		return

	# No creature clicked — snap to nearest nav node and issue directed goto.
	var result: Dictionary = _bridge.snap_placement_to_ray(ray_origin, ray_dir, false, false)
	if result.get("hit", false):
		var nav_pos: Vector3 = result["position"]
		_bridge.group_directed_goto(
			_selected_creature_ids, int(nav_pos.x), int(nav_pos.y), int(nav_pos.z), queue
		)
		get_viewport().set_input_as_handled()


## Execute attack-move: dispatch a GroupAttackMove command for all selected
## creatures to the clicked location (ground or creature position).
func _execute_attack_move(mouse_pos: Vector2, queue: bool = false) -> void:
	if _selected_creature_ids.is_empty():
		return

	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	# Check if we clicked on a creature — use their position as the destination.
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var target_pos := Vector3.ZERO
	var found_creature := false

	var sel_data := _bridge.get_creature_selection_data(_render_tick)
	var sel_species: PackedStringArray = sel_data.get("species", PackedStringArray())
	var sel_positions: PackedVector3Array = sel_data.get("positions", PackedVector3Array())
	for i in sel_positions.size():
		var pos := sel_positions[i]
		var y_off: float = CreatureRenderer.SPECIES_Y_OFFSETS.get(
			sel_species[i], CreatureRenderer.DEFAULT_Y_OFFSET
		)
		var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
		var dist_sq := _point_to_ray_dist_sq(world_pos, ray_origin, ray_dir)
		if dist_sq < best_dist_sq:
			best_dist_sq = dist_sq
			target_pos = pos
			found_creature = true

	if found_creature:
		_bridge.group_attack_move(
			_selected_creature_ids, int(target_pos.x), int(target_pos.y), int(target_pos.z), queue
		)
		return

	# No creature clicked — snap to nearest nav node.
	var result: Dictionary = _bridge.snap_placement_to_ray(ray_origin, ray_dir, false, false)
	if result.get("hit", false):
		var nav_pos: Vector3 = result["position"]
		_bridge.group_attack_move(
			_selected_creature_ids, int(nav_pos.x), int(nav_pos.y), int(nav_pos.z), queue
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
## Delegates to GeometryUtils (geometry_utils.gd) — the single source of truth.
func _make_screen_rect(a: Vector2, b: Vector2) -> Rect2:
	return GeometryUtils.make_screen_rect(a, b)


# ---------------------------------------------------------------------------
# Selection groups (F-selection-groups)
# ---------------------------------------------------------------------------


## Map a keycode to group number 1–9, or 0 if not a number key.
func _keycode_to_group_number(keycode: int) -> int:
	if keycode >= KEY_1 and keycode <= KEY_9:
		return keycode - KEY_0
	return 0


## Save the current selection as group N (Ctrl+N).
func _save_selection_group(group_num: int) -> void:
	var group := {
		"creature_ids": _selected_creature_ids.duplicate(),
		"structure_ids": [] as Array,
	}
	if _selected_structure_id >= 0:
		group["structure_ids"] = [_selected_structure_id]
	_selection_groups[group_num] = group
	# Persist to sim for save/load.
	if _bridge:
		_bridge.set_selection_group(group_num, group["creature_ids"], group["structure_ids"])


## Add the current selection to group N (Shift+N).
func _add_to_selection_group(group_num: int) -> void:
	if not _selection_groups.has(group_num):
		_save_selection_group(group_num)
		return
	var group: Dictionary = _selection_groups[group_num]
	var cids: Array = group["creature_ids"]
	for cid in _selected_creature_ids:
		if cids.find(cid) < 0:
			cids.append(cid)
	var sids: Array = group["structure_ids"]
	if _selected_structure_id >= 0 and sids.find(_selected_structure_id) < 0:
		sids.append(_selected_structure_id)
	# Persist to sim.
	if _bridge:
		_bridge.set_selection_group(group_num, cids, sids)


## Recall group N (plain number key). Double-tap centers camera.
func _recall_selection_group(group_num: int) -> void:
	var now := Time.get_ticks_msec() / 1000.0
	var is_double_tap := (
		_last_group_key == group_num and (now - _last_group_key_time) < DOUBLE_TAP_THRESHOLD
	)
	_last_group_key = group_num
	_last_group_key_time = now

	if not _selection_groups.has(group_num):
		return
	var group: Dictionary = _selection_groups[group_num]
	var cids: Array = group["creature_ids"]
	var sids: Array = group["structure_ids"]

	# Apply the selection.
	_deselect_creature_only()
	_deselect_structure_only()
	_deselect_pile_only()
	_attack_move_mode = false

	if not cids.is_empty():
		_selected_creature_ids = cids.duplicate()
		creatures_selected.emit(_selected_creature_ids)
	elif not sids.is_empty():
		_selected_structure_id = sids[0]
		structure_selected.emit(_selected_structure_id)

	# Double-tap: center camera on group centroid.
	if is_double_tap:
		var centroid := _compute_group_centroid(cids, sids)
		if centroid != Vector3.ZERO:
			group_center_requested.emit(centroid)


## Compute the centroid of a selection group's members for camera centering.
func _compute_group_centroid(creature_ids: Array, structure_ids: Array) -> Vector3:
	var sum := Vector3.ZERO
	var count := 0

	for cid in creature_ids:
		var info := _bridge.get_creature_info_by_id(cid, _render_tick)
		if info.is_empty():
			continue
		sum += Vector3(info.get("x", 0.0), info.get("y", 0.0), info.get("z", 0.0))
		count += 1

	for sid in structure_ids:
		var sinfo := _bridge.get_structure_info(sid)
		if sinfo.is_empty():
			continue
		# Compute center from anchor + dimensions.
		var ax: float = sinfo.get("anchor_x", 0)
		var ay: float = sinfo.get("anchor_y", 0)
		var az: float = sinfo.get("anchor_z", 0)
		var w: float = sinfo.get("width", 1)
		var d: float = sinfo.get("depth", 1)
		var h: float = sinfo.get("height", 1)
		sum += Vector3(ax + w / 2.0, ay + h / 2.0, az + d / 2.0)
		count += 1

	if count == 0:
		return Vector3.ZERO
	return sum / count


## Hydrate local selection groups from the sim after loading a save.
## Called by main.gd after a successful load_game_json().
func hydrate_selection_groups() -> void:
	_selection_groups.clear()
	if not _bridge:
		return
	var groups := _bridge.get_all_selection_groups()
	for entry in groups:
		var num: int = entry.get("group_number", 0)
		_selection_groups[num] = {
			"creature_ids": entry.get("creature_ids", []),
			"structure_ids": entry.get("structure_ids", []),
		}
