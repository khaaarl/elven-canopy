## Handles click-to-select for creatures and structures.
##
## On left-click (when not in placement mode), casts a ray from the camera
## through the mouse position and finds the closest creature sprite using
## perpendicular distance. If no creature is within snap threshold, falls
## back to a voxel raycast via bridge.raycast_structure() to check for
## structure hits. Uses the interpolated render_tick positions (via
## set_render_tick(), called by main.gd each frame) so click targets match
## the smooth visual positions.
##
## Selection state: tracks either a creature (species name + index) or a
## structure (structure_id). Selecting a creature deselects any structure
## and vice versa. ESC deselects whichever is active.
##
## Uses a data-driven SPECIES_Y_OFFSETS dict so adding new species doesn't
## require code changes here — just add the entry.
##
## See also: creature_info_panel.gd for the creature UI panel,
## structure_info_panel.gd for the structure UI panel, orbital_camera.gd
## for follow mode, placement_controller.gd for the ray-snap algorithm
## origin, elf_renderer.gd / capybara_renderer.gd / creature_renderer.gd
## for sprite position offsets.

extends Node3D

signal creature_selected(species: String, index: int)
signal creature_deselected
signal structure_selected(structure_id: int)
signal structure_deselected

## Maximum perpendicular distance (world units) from the mouse ray to a
## creature sprite center for it to count as a click hit. Tighter than
## placement_controller's 5.0 since sprites are small.
const SNAP_THRESHOLD := 1.5

## Y offsets per species — must match the renderers.
const SPECIES_Y_OFFSETS = {
	"Elf": 0.48,
	"Capybara": 0.32,
	"Boar": 0.38,
	"Deer": 0.46,
	"Elephant": 0.8,
	"Monkey": 0.44,
	"Squirrel": 0.28,
}

var _bridge: SimBridge
var _camera: Camera3D
var _placement_controller: Node3D
var _render_tick: float = 0.0

var _selected_species: String = ""
var _selected_index: int = -1
var _selected_structure_id: int = -1


func setup(bridge: SimBridge, camera: Camera3D) -> void:
	_bridge = bridge
	_camera = camera


## Set the fractional render tick for smooth movement interpolation.
## Called by main.gd each frame after stepping the sim.
func set_render_tick(tick: float) -> void:
	_render_tick = tick


func set_placement_controller(controller: Node3D) -> void:
	_placement_controller = controller


func get_selected_species() -> String:
	return _selected_species


func get_selected_index() -> int:
	return _selected_index


func get_selected_structure_id() -> int:
	return _selected_structure_id


## Programmatically select a creature by species and index, as if the player
## clicked on it. Used by the task panel to trigger the full selection flow.
func select_creature(species: String, index: int) -> void:
	_deselect_structure_only()
	_selected_species = species
	_selected_index = index
	creature_selected.emit(species, index)


## Programmatically select a structure by ID. Used by the structure list
## panel's zoom button to open the info panel alongside camera movement.
func select_structure(id: int) -> void:
	_deselect_creature_only()
	_selected_structure_id = id
	structure_selected.emit(id)


func deselect() -> void:
	if _selected_index >= 0:
		_selected_species = ""
		_selected_index = -1
		creature_deselected.emit()
	if _selected_structure_id >= 0:
		_selected_structure_id = -1
		structure_deselected.emit()


func _unhandled_input(event: InputEvent) -> void:
	# Don't process selection clicks during placement mode.
	if _placement_controller and _placement_controller.is_placing():
		return

	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.pressed and mb.button_index == MOUSE_BUTTON_LEFT:
			_try_select(mb.position)

	if event is InputEventKey:
		var key := event as InputEventKey
		if (
			key.pressed
			and key.keycode == KEY_ESCAPE
			and (_selected_index >= 0 or _selected_structure_id >= 0)
		):
			deselect()
			get_viewport().set_input_as_handled()


func _try_select(mouse_pos: Vector2) -> void:
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

	# First, try to select a creature (closest sprite within snap threshold).
	var best_dist_sq := SNAP_THRESHOLD * SNAP_THRESHOLD
	var best_species := ""
	var best_index := -1

	for species_name in SPECIES_Y_OFFSETS:
		var positions := _bridge.get_creature_positions(species_name, _render_tick)
		var y_off: float = SPECIES_Y_OFFSETS[species_name]
		for i in positions.size():
			var pos := positions[i]
			var world_pos := Vector3(pos.x + 0.5, pos.y + y_off, pos.z + 0.5)
			var dist_sq := _point_to_ray_dist_sq(world_pos, ray_origin, ray_dir)
			if dist_sq < best_dist_sq:
				best_dist_sq = dist_sq
				best_species = species_name
				best_index = i

	if best_index >= 0:
		_deselect_structure_only()
		_selected_species = best_species
		_selected_index = best_index
		creature_selected.emit(best_species, best_index)
		get_viewport().set_input_as_handled()
		return

	# No creature hit — try structure raycast.
	var sid := _bridge.raycast_structure(ray_origin, ray_dir)
	if sid >= 0:
		_deselect_creature_only()
		_selected_structure_id = sid
		structure_selected.emit(sid)
		get_viewport().set_input_as_handled()
		return

	# Clicked on nothing — deselect whatever was active.
	if _selected_index >= 0 or _selected_structure_id >= 0:
		deselect()


## Clear creature selection without touching structure state. Emits
## creature_deselected so main.gd can hide the creature info panel.
func _deselect_creature_only() -> void:
	if _selected_index >= 0:
		_selected_species = ""
		_selected_index = -1
		creature_deselected.emit()


## Clear structure selection without touching creature state. Emits
## structure_deselected so main.gd can hide the structure info panel.
func _deselect_structure_only() -> void:
	if _selected_structure_id >= 0:
		_selected_structure_id = -1
		structure_deselected.emit()


## Perpendicular distance squared from a point to an infinite ray.
## Clamps t >= 0 so points behind the camera are handled correctly.
func _point_to_ray_dist_sq(point: Vector3, ray_origin: Vector3, ray_dir: Vector3) -> float:
	var to_point := point - ray_origin
	var t := maxf(0.0, to_point.dot(ray_dir))
	var closest := ray_origin + ray_dir * t
	return (point - closest).length_squared()
