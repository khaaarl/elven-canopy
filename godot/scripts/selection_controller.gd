## Handles click-to-select for all creature species.
##
## On left-click (when not in placement mode), casts a ray from the camera
## through the mouse position and finds the closest creature sprite using
## perpendicular distance. Uses the interpolated render_tick positions (via
## set_render_tick(), called by main.gd each frame) so click targets match
## the smooth visual positions.
##
## Selection state: tracks species name and index (matching the position array
## order from SimBridge). When a creature is selected, emits creature_selected;
## when deselected, emits creature_deselected.
##
## Uses a data-driven SPECIES_Y_OFFSETS dict so adding new species doesn't
## require code changes here — just add the entry.
##
## See also: creature_info_panel.gd for the UI panel, orbital_camera.gd for
## follow mode, placement_controller.gd for the ray-snap algorithm origin,
## elf_renderer.gd / capybara_renderer.gd / creature_renderer.gd for sprite
## position offsets.

extends Node3D

signal creature_selected(species: String, index: int)
signal creature_deselected

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
	"Monkey": 0.44,
	"Squirrel": 0.28,
}

var _bridge: SimBridge
var _camera: Camera3D
var _placement_controller: Node3D
var _render_tick: float = 0.0

var _selected_species: String = ""
var _selected_index: int = -1


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


func deselect() -> void:
	_selected_species = ""
	_selected_index = -1
	creature_deselected.emit()


func _unhandled_input(event: InputEvent) -> void:
	# Don't process selection clicks during placement mode.
	if _placement_controller and _placement_controller.is_placing():
		return

	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.pressed and mb.button_index == MOUSE_BUTTON_LEFT:
			_try_select_creature(mb.position)

	if event is InputEventKey:
		var key := event as InputEventKey
		if key.pressed and key.keycode == KEY_ESCAPE and _selected_index >= 0:
			deselect()
			get_viewport().set_input_as_handled()


func _try_select_creature(mouse_pos: Vector2) -> void:
	var ray_origin := _camera.project_ray_origin(mouse_pos)
	var ray_dir := _camera.project_ray_normal(mouse_pos)

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
		_selected_species = best_species
		_selected_index = best_index
		creature_selected.emit(best_species, best_index)
		get_viewport().set_input_as_handled()
	else:
		if _selected_index >= 0:
			deselect()


## Perpendicular distance squared from a point to an infinite ray.
## Clamps t >= 0 so points behind the camera are handled correctly.
func _point_to_ray_dist_sq(point: Vector3, ray_origin: Vector3, ray_dir: Vector3) -> float:
	var to_point := point - ray_origin
	var t := maxf(0.0, to_point.dot(ray_dir))
	var closest := ray_origin + ray_dir * t
	return (point - closest).length_squared()
