## Hover tooltip for world objects (creatures, structures, ground piles, fruit).
##
## Each frame, casts a ray from the camera through the current mouse position
## and checks for the closest hoverable object using the same hit-detection
## pattern as selection_controller.gd. When a hit is found, displays a small
## tooltip label near the mouse cursor showing contextual info:
##
## - Creatures: "Elf: Vaelindra — Eating" or "Capybara — Idle"
## - Structures: "Kitchen: Hearthglow" or "Platform #42"
## - Ground piles: "Apple x3, Wood x2" (up to 3 stacks, then "and N more...")
## - Fruit: "Thúni Réva (red berry)" or "Fruit" if no species tracked
##
## The tooltip is suppressed during placement mode, construction mode, and
## when UI overlays are open (pause menu, task panel, etc.). Tooltip text is
## regenerated every frame so live changes (e.g., activity transitions) are
## reflected immediately.
##
## See also: selection_controller.gd for the click-to-select equivalent,
## main.gd for wiring and render_tick distribution.

extends Node

## Maximum perpendicular distance (world units) from the mouse ray to an
## object center for it to count as a hover hit. Same as selection_controller.
const SNAP_THRESHOLD := 1.5
const SNAP_THRESHOLD_SQ := SNAP_THRESHOLD * SNAP_THRESHOLD

## Y offsets per species — must match the renderers and selection_controller.
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

## Human-readable activity labels for task kinds.
const ACTIVITY_NAMES = {
	"GoTo": "Walking",
	"Build": "Building",
	"EatBread": "Eating",
	"EatFruit": "Eating",
	"Sleep": "Sleeping",
	"Furnish": "Furnishing",
	"Haul": "Hauling",
	"Cook": "Cooking",
	"Harvest": "Harvesting",
	"AcquireItem": "Fetching",
	"Moping": "Moping",
	"Craft": "Crafting",
	"AttackMove": "Attack Moving",
	"Attack": "Attacking",
}

var _bridge: SimBridge
var _camera: Camera3D
var _placement_controller: Node3D
var _construction_controller: Node
var _render_tick: float = 0.0

## When true, tooltip is hidden (e.g., overlay panels are open).
var _suppressed: bool = false

## The tooltip label, positioned near the mouse on a CanvasLayer.
var _tooltip_panel: PanelContainer
var _tooltip_label: Label

## Last known mouse position (viewport coordinates).
var _mouse_pos: Vector2 = Vector2.ZERO


func setup(bridge: SimBridge, camera: Camera3D, canvas_layer: CanvasLayer) -> void:
	_bridge = bridge
	_camera = camera
	_build_tooltip_ui(canvas_layer)


func set_render_tick(tick: float) -> void:
	_render_tick = tick


func set_placement_controller(controller: Node3D) -> void:
	_placement_controller = controller


func set_construction_controller(controller: Node) -> void:
	_construction_controller = controller


## Suppress or unsuppress the tooltip (e.g., when overlay panels are open).
func set_suppressed(suppressed: bool) -> void:
	_suppressed = suppressed
	if suppressed:
		_hide_tooltip()


func _build_tooltip_ui(canvas_layer: CanvasLayer) -> void:
	_tooltip_panel = PanelContainer.new()
	_tooltip_panel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_tooltip_panel.visible = false

	# Style: semi-transparent dark background.
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.1, 0.1, 0.1, 0.85)
	style.set_corner_radius_all(4)
	style.set_content_margin_all(6)
	_tooltip_panel.add_theme_stylebox_override("panel", style)

	_tooltip_label = Label.new()
	_tooltip_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_tooltip_panel.add_child(_tooltip_label)

	canvas_layer.add_child(_tooltip_panel)


func _process(_delta: float) -> void:
	if not _bridge or not _camera:
		return

	# Suppress during placement/construction modes or when overlays are open.
	if _suppressed:
		return
	if _placement_controller and _placement_controller.is_placing():
		_hide_tooltip()
		return
	if _construction_controller and _construction_controller.is_placing():
		_hide_tooltip()
		return

	_mouse_pos = _tooltip_panel.get_viewport().get_mouse_position()
	var target := _find_hover_target()

	if target.is_empty():
		_hide_tooltip()
		return

	_update_tooltip_text(target)
	_tooltip_panel.visible = true
	_position_tooltip()


func _hide_tooltip() -> void:
	_tooltip_panel.visible = false


func _find_hover_target() -> Dictionary:
	var ray_origin := _camera.project_ray_origin(_mouse_pos)
	var ray_dir := _camera.project_ray_normal(_mouse_pos)

	# 1. Creatures (closest sprite within snap threshold).
	var best_dist_sq := SNAP_THRESHOLD_SQ
	var best_creature_id := ""

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
				best_creature_id = ids[i]

	if best_creature_id != "":
		return {"type": "creature", "creature_id": best_creature_id}

	# 2. Structures (voxel raycast via bridge).
	var sid := _bridge.raycast_structure(ray_origin, ray_dir)
	if sid >= 0:
		return {"type": "structure", "id": sid}

	# 3. Ground piles (point-to-ray distance check).
	var piles := _bridge.get_ground_piles()
	var pile_best_dist_sq := SNAP_THRESHOLD_SQ
	var pile_best_pos := Vector3i(-1, -1, -1)
	for pile_entry in piles:
		var px: int = pile_entry.get("x", 0)
		var py: int = pile_entry.get("y", 0)
		var pz: int = pile_entry.get("z", 0)
		var pile_world := Vector3(px + 0.5, py + 0.1, pz + 0.5)
		var pdist_sq := _point_to_ray_dist_sq(pile_world, ray_origin, ray_dir)
		if pdist_sq < pile_best_dist_sq:
			pile_best_dist_sq = pdist_sq
			pile_best_pos = Vector3i(px, py, pz)

	if pile_best_pos != Vector3i(-1, -1, -1):
		return {"type": "pile", "x": pile_best_pos.x, "y": pile_best_pos.y, "z": pile_best_pos.z}

	# 4. Fruit voxels (point-to-ray distance check).
	var fruit_data := _bridge.get_fruit_voxels()
	var fruit_best_dist_sq := SNAP_THRESHOLD_SQ
	var fruit_hit := false
	var fruit_x := 0
	var fruit_y := 0
	var fruit_z := 0
	var i := 0
	while i + 3 < fruit_data.size():
		var fx: int = fruit_data[i]
		var fy: int = fruit_data[i + 1]
		var fz: int = fruit_data[i + 2]
		# fruit_data[i + 3] is the species_id — not needed for tooltip hit testing.
		var fruit_world := Vector3(fx + 0.5, fy + 0.5, fz + 0.5)
		var fdist_sq := _point_to_ray_dist_sq(fruit_world, ray_origin, ray_dir)
		if fdist_sq < fruit_best_dist_sq:
			fruit_best_dist_sq = fdist_sq
			fruit_hit = true
			fruit_x = fx
			fruit_y = fy
			fruit_z = fz
		i += 4

	if fruit_hit:
		return {"type": "fruit", "x": fruit_x, "y": fruit_y, "z": fruit_z}

	return {}


func _update_tooltip_text(target: Dictionary) -> void:
	var text := ""
	match target.get("type", ""):
		"creature":
			text = _creature_tooltip(target["creature_id"])
		"structure":
			text = _structure_tooltip(target["id"])
		"pile":
			text = _pile_tooltip(target["x"], target["y"], target["z"])
		"fruit":
			text = _fruit_tooltip(target["x"], target["y"], target["z"])
	_tooltip_label.text = text


func _creature_tooltip(creature_id: String) -> String:
	var info := _bridge.get_creature_info_by_id(creature_id, _render_tick)
	if info.is_empty():
		return "Creature"

	var species: String = info.get("species", "Creature")
	var name_str: String = info.get("name", "")
	var has_task: bool = info.get("has_task", false)
	var task_kind: String = info.get("task_kind", "")

	var activity := "Idle"
	if has_task and not task_kind.is_empty():
		activity = ACTIVITY_NAMES.get(task_kind, task_kind)

	if not name_str.is_empty():
		return "%s: %s — %s" % [species, name_str, activity]
	return "%s — %s" % [species, activity]


func _structure_tooltip(structure_id: int) -> String:
	var info := _bridge.get_structure_info(structure_id)
	if info.is_empty():
		return "Structure"

	var name_str: String = info.get("name", "Structure")
	var furnishing: String = info.get("furnishing", "")
	var has_custom_name: bool = info.get("has_custom_name", false)

	if not furnishing.is_empty():
		return "%s: %s" % [furnishing, name_str]
	if has_custom_name:
		var build_type: String = info.get("build_type", "Structure")
		return "%s: %s" % [build_type, name_str]
	return name_str


func _fruit_tooltip(x: int, y: int, z: int) -> String:
	var name := _bridge.get_fruit_species_name(x, y, z)
	if name.is_empty():
		return "Fruit"
	return name


func _pile_tooltip(x: int, y: int, z: int) -> String:
	var info := _bridge.get_ground_pile_info(x, y, z)
	if info.is_empty():
		return "Ground Pile"

	var inventory: Array = info.get("inventory", [])
	if inventory.is_empty():
		return "Empty Pile"

	var max_shown := 3
	var shown_parts: PackedStringArray = []
	var extra_stacks := 0
	var extra_items := 0

	for i in inventory.size():
		var stack: Dictionary = inventory[i]
		var kind: String = stack.get("kind", "?")
		var qty: int = stack.get("quantity", 0)
		if i < max_shown:
			if qty > 1:
				shown_parts.append("%s x%d" % [kind, qty])
			else:
				shown_parts.append(kind)
		else:
			extra_stacks += 1
			extra_items += qty

	var text := ", ".join(shown_parts)
	if extra_stacks > 0:
		text += (
			"\nand %d more stack%s of %d item%s"
			% [
				extra_stacks,
				"s" if extra_stacks != 1 else "",
				extra_items,
				"s" if extra_items != 1 else "",
			]
		)
	return text


func _position_tooltip() -> void:
	# Position tooltip near mouse, offset to the right and down.
	# If it would go off-screen, flip to the other side.
	var offset := Vector2(16, 16)
	var viewport_size := _tooltip_panel.get_viewport_rect().size
	var panel_size := _tooltip_panel.size

	var pos := _mouse_pos + offset

	# Flip horizontally if tooltip would go off-screen.
	if pos.x + panel_size.x > viewport_size.x:
		pos.x = _mouse_pos.x - panel_size.x - 8

	# Flip vertically if tooltip would go off-screen.
	if pos.y + panel_size.y > viewport_size.y:
		pos.y = _mouse_pos.y - panel_size.y - 8

	_tooltip_panel.position = pos


## Perpendicular distance squared from a point to an infinite ray.
## Clamps t >= 0 so points behind the camera are handled correctly.
func _point_to_ray_dist_sq(point: Vector3, ray_origin: Vector3, ray_dir: Vector3) -> float:
	var to_point := point - ray_origin
	var t := maxf(0.0, to_point.dot(ray_dir))
	var closest := ray_origin + ray_dir * t
	return (point - closest).length_squared()
