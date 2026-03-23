## Zoomable top-down (XZ) minimap overlay.
##
## Renders a cached terrain heightmap, creature position dots, selected-unit
## highlights, and the camera frustum outline in a small square panel anchored
## to the bottom-right corner of the screen. Size is ~15% of viewport height.
##
## Zoom: discrete steps controlled by mouse wheel (when cursor is over the
## minimap) or +/- icon buttons. Follow mode: toggleable — follow camera
## focal point or center on main tree.
##
## Terrain texture is regenerated only when voxels change (tree growth,
## construction). Creature dots are redrawn every frame (cheap — just points
## on a small canvas). Z-levels above camera focal height are rendered at
## reduced opacity (ghostly).
##
## Architecture: the drawing logic is kept generic (map-rendering code) to
## enable future reuse for a full-screen map, side-view panel, etc.
##
## See also: main.gd for wiring and per-frame updates, orbital_camera.gd for
## camera position and focus, sim_bridge.rs for get_terrain_heightmap() and
## creature position queries.

extends PanelContainer

## Signal emitted when the user clicks the minimap to jump the camera.
signal camera_jump_requested(world_pos: Vector3)

## Fraction of viewport height used for the minimap side length.
const SIZE_FRACTION := 0.15
## Minimum minimap size in pixels (so it doesn't get unusably small).
const MIN_SIZE_PX := 120.0
## Maximum minimap size in pixels.
const MAX_SIZE_PX := 400.0

## Discrete zoom levels. Each value is the number of world units visible
## across the minimap's width/height. Lower = more zoomed in.
const ZOOM_LEVELS: Array[float] = [32.0, 64.0, 128.0, 256.0, 512.0]
## Default zoom index (show 64 world units across the minimap).
const DEFAULT_ZOOM_INDEX := 1

## Colors for terrain height shading.
const COLOR_GROUND := Color(0.22, 0.35, 0.15)
const COLOR_TREE_LOW := Color(0.35, 0.25, 0.15)
const COLOR_TREE_HIGH := Color(0.55, 0.45, 0.25)
## Color for empty columns (no solid voxel).
const COLOR_EMPTY := Color(0.08, 0.08, 0.08, 0.6)
## Camera frustum outline color.
const COLOR_FRUSTUM := Color(1.0, 1.0, 1.0, 0.7)
## Creature dot colors by faction — matches selection_highlight.gd ring colors.
const COLOR_PLAYER := Color(0.3, 0.5, 1.0)
const COLOR_NEUTRAL := Color(1.0, 0.85, 0.2)
const COLOR_HOSTILE := Color(1.0, 0.2, 0.2)
## Selected creature highlight — brighter white so it stands out over any faction.
const COLOR_SELECTED := Color(1.0, 1.0, 1.0)

## Throttle terrain regeneration to avoid per-frame heightmap queries.
const TERRAIN_UPDATE_INTERVAL := 1.0

## Button size for minimap toolbar icons.
const BTN_SIZE := 16

var _bridge: SimBridge
var _camera_pivot: Node3D
var _selector: Node3D

## World dimensions from the sim.
var _world_size := Vector3i.ZERO

## Current zoom index into ZOOM_LEVELS.
var _zoom_index: int = DEFAULT_ZOOM_INDEX

## Follow mode: true = follow camera, false = center on main tree.
var _follow_camera: bool = true

## The draw area control (child of this PanelContainer).
var _draw_area: Control

## Cached terrain heightmap (flat array, size_x * size_z, row-major X-fastest).
var _heightmap := PackedByteArray()
## Cached terrain image for drawing.
var _terrain_image: Image
var _terrain_texture: ImageTexture
## Timer for terrain update throttling.
var _terrain_timer: float = 0.0

## Buttons.
var _zoom_in_btn: Button
var _zoom_out_btn: Button
var _follow_btn: Button

## Whether the mouse is currently over the minimap (for scroll capture).
var _mouse_inside: bool = false
## Whether a drag is in progress (for panning).
var _dragging: bool = false

## Render tick from main.gd for creature position interpolation.
var _render_tick: float = 0.0

## Currently selected creature IDs (from selection controller).
var _selected_ids: Array = []


func setup(bridge: SimBridge, camera_pivot: Node3D, selector: Node3D) -> void:
	_bridge = bridge
	_camera_pivot = camera_pivot
	_selector = selector
	_world_size = bridge.get_world_size()
	_rebuild_terrain()


func set_render_tick(tick: float) -> void:
	_render_tick = tick


func set_selected_ids(ids: Array) -> void:
	_selected_ids = ids


func _ready() -> void:
	# Style: thin border, dark semi-transparent background.
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.06, 0.06, 0.06, 0.85)
	style.border_color = Color(0.4, 0.4, 0.4, 0.8)
	style.set_border_width_all(1)
	style.set_corner_radius_all(2)
	style.set_content_margin_all(0)
	add_theme_stylebox_override("panel", style)

	# Anchor to bottom-right corner.
	set_anchors_preset(Control.PRESET_BOTTOM_RIGHT)
	anchor_left = 1.0
	anchor_top = 1.0
	anchor_right = 1.0
	anchor_bottom = 1.0
	grow_horizontal = Control.GROW_DIRECTION_BEGIN
	grow_vertical = Control.GROW_DIRECTION_BEGIN

	# The draw area handles all custom rendering.
	_draw_area = Control.new()
	_draw_area.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(_draw_area)
	_draw_area.draw.connect(_on_draw)

	# Overlay container for buttons (anchored top-right inside the minimap).
	# Button container: anchored top-right, shrinks vertically so the
	# PanelContainer doesn't stretch it to fill the full minimap height.
	var btn_container := HBoxContainer.new()
	btn_container.set_anchors_preset(Control.PRESET_TOP_RIGHT)
	btn_container.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	btn_container.grow_vertical = Control.GROW_DIRECTION_END
	btn_container.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	btn_container.offset_top = 2
	btn_container.offset_right = -2
	btn_container.mouse_filter = Control.MOUSE_FILTER_IGNORE
	btn_container.add_theme_constant_override("separation", 2)
	add_child(btn_container)

	# Initial icon is "eye" because _follow_camera starts true.
	_follow_btn = _make_icon_button("eye", "Following camera (click to center on tree)")
	_follow_btn.pressed.connect(_toggle_follow_mode)
	btn_container.add_child(_follow_btn)

	_zoom_in_btn = _make_icon_button("+", "Zoom in")
	_zoom_in_btn.pressed.connect(_zoom_in)
	btn_container.add_child(_zoom_in_btn)

	_zoom_out_btn = _make_icon_button("-", "Zoom out")
	_zoom_out_btn.pressed.connect(_zoom_out)
	btn_container.add_child(_zoom_out_btn)

	# Track mouse enter/exit for scroll wheel capture.
	mouse_entered.connect(func(): _mouse_inside = true)
	mouse_exited.connect(func(): _mouse_inside = false)

	_update_size()


func _process(delta: float) -> void:
	_update_size()

	# Periodically refresh the terrain texture.
	if _bridge:
		_terrain_timer += delta
		if _terrain_timer >= TERRAIN_UPDATE_INTERVAL:
			_terrain_timer = 0.0
			_rebuild_terrain()

	# Request redraw every frame for creature dots and camera frustum.
	if _draw_area:
		_draw_area.queue_redraw()


func _gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_WHEEL_UP and mb.pressed:
			_zoom_in()
			accept_event()
		elif mb.button_index == MOUSE_BUTTON_WHEEL_DOWN and mb.pressed:
			_zoom_out()
			accept_event()
		elif mb.button_index == MOUSE_BUTTON_LEFT:
			if mb.pressed:
				_dragging = true
				_handle_click(mb.position)
			else:
				_dragging = false
			accept_event()
	elif event is InputEventMouseMotion and _dragging:
		_handle_click((event as InputEventMouseMotion).position)
		accept_event()


func _zoom_in() -> void:
	if _zoom_index > 0:
		_zoom_index -= 1
		_update_zoom_buttons()


func _zoom_out() -> void:
	if _zoom_index < ZOOM_LEVELS.size() - 1:
		_zoom_index += 1
		_update_zoom_buttons()


func _toggle_follow_mode() -> void:
	_follow_camera = not _follow_camera
	_update_follow_button()


func _handle_click(local_pos: Vector2) -> void:
	var world_pos := _screen_to_world(local_pos)
	if (
		world_pos.x >= 0
		and world_pos.x < _world_size.x
		and world_pos.z >= 0
		and world_pos.z < _world_size.z
	):
		camera_jump_requested.emit(world_pos)


## Convert a local minimap position to a world XZ position (Y = camera height).
func _screen_to_world(local_pos: Vector2) -> Vector3:
	var map_size := _get_map_size()
	var center := _get_center()
	var span: float = ZOOM_LEVELS[_zoom_index]

	# local_pos is relative to the PanelContainer. Account for the 1px border.
	var draw_pos := local_pos - Vector2(1, 1)
	var draw_size := Vector2(map_size, map_size)

	# Normalized position within the draw area (0..1).
	var norm_x: float = draw_pos.x / draw_size.x
	var norm_z: float = draw_pos.y / draw_size.y

	var world_x: float = center.x + (norm_x - 0.5) * span
	var world_z: float = center.y + (norm_z - 0.5) * span
	var world_y: float = _camera_pivot.position.y if _camera_pivot else 0.0

	return Vector3(world_x, world_y, world_z)


## Get the XZ center of the minimap view in world coordinates.
func _get_center() -> Vector2:
	if _follow_camera and _camera_pivot:
		return Vector2(_camera_pivot.position.x, _camera_pivot.position.z)
	# Center on the middle of the world (where the main tree is).
	return Vector2(_world_size.x * 0.5, _world_size.z * 0.5)


## Get the pixel size of the minimap drawing area (square).
func _get_map_size() -> float:
	return size.x - 2.0  # Account for 1px border on each side.


func _update_size() -> void:
	var viewport_h: float = get_viewport_rect().size.y
	var map_px := clampf(viewport_h * SIZE_FRACTION, MIN_SIZE_PX, MAX_SIZE_PX)
	# Add 2 for the border.
	var total := map_px + 2.0
	custom_minimum_size = Vector2(total, total)
	size = Vector2(total, total)
	offset_left = -total - 10
	offset_top = -total - 10
	offset_right = -10
	offset_bottom = -10


func _update_zoom_buttons() -> void:
	if _zoom_in_btn:
		_zoom_in_btn.disabled = (_zoom_index <= 0)
	if _zoom_out_btn:
		_zoom_out_btn.disabled = (_zoom_index >= ZOOM_LEVELS.size() - 1)


func _update_follow_button() -> void:
	if not _follow_btn:
		return
	if _follow_camera:
		_follow_btn.tooltip_text = "Following camera (click to center on tree)"
	else:
		_follow_btn.tooltip_text = "Centered on tree (click to follow camera)"
	# Trigger icon redraw.
	var icon_node: Control = _follow_btn.get_child(0) if _follow_btn.get_child_count() > 0 else null
	if icon_node:
		icon_node.set_meta("icon_type", "eye" if _follow_camera else "tree")
		icon_node.queue_redraw()


## Rebuild the cached terrain texture from the heightmap.
func _rebuild_terrain() -> void:
	if not _bridge or _world_size.x <= 0 or _world_size.z <= 0:
		return
	_heightmap = _bridge.get_terrain_heightmap()
	var w: int = _world_size.x
	var h: int = _world_size.z
	_terrain_image = Image.create(w, h, false, Image.FORMAT_RGBA8)
	var max_y: float = _world_size.y
	for z in range(h):
		for x in range(w):
			var height: int = _heightmap[x + z * w]
			if height == 0:
				_terrain_image.set_pixel(x, z, COLOR_EMPTY)
			else:
				var t: float = float(height) / max_y
				var color: Color
				if height <= 1:
					# Ground level.
					color = COLOR_GROUND
				else:
					# Tree / construction — lerp from low brown to high tan.
					color = COLOR_TREE_LOW.lerp(COLOR_TREE_HIGH, t)
				_terrain_image.set_pixel(x, z, color)
	_terrain_texture = ImageTexture.create_from_image(_terrain_image)


## Main draw callback — terrain, creatures, frustum.
func _on_draw() -> void:
	if not _bridge or not _draw_area:
		return

	var map_size := _get_map_size()
	var center := _get_center()
	var span: float = ZOOM_LEVELS[_zoom_index]
	var draw_rect := Rect2(Vector2(1, 1), Vector2(map_size, map_size))

	# --- Terrain ---
	if _terrain_texture:
		_draw_terrain(draw_rect, center, span)

	# --- Camera focal height (for ghostly z-level filtering) ---
	var cam_y: float = _camera_pivot.position.y if _camera_pivot else 0.0

	# --- Creatures ---
	_draw_creatures(draw_rect, center, span, cam_y)

	# --- Camera frustum outline ---
	if _camera_pivot:
		_draw_frustum(draw_rect, center, span)


## Draw the terrain texture, cropped and scaled to the current view.
func _draw_terrain(draw_rect: Rect2, center: Vector2, span: float) -> void:
	if not _terrain_texture:
		return
	var tex_w: float = _terrain_texture.get_width()
	var tex_h: float = _terrain_texture.get_height()

	# Source rect in texture space (world coords map 1:1 to pixels).
	var src_left: float = center.x - span * 0.5
	var src_top: float = center.y - span * 0.5
	var src_rect := Rect2(src_left, src_top, span, span)

	# Clamp source to texture bounds and adjust draw rect accordingly.
	var clamped_src := Rect2(
		maxf(src_rect.position.x, 0),
		maxf(src_rect.position.y, 0),
		0,
		0,
	)
	var src_right := minf(src_rect.position.x + src_rect.size.x, tex_w)
	var src_bottom := minf(src_rect.position.y + src_rect.size.y, tex_h)
	clamped_src.size.x = src_right - clamped_src.position.x
	clamped_src.size.y = src_bottom - clamped_src.position.y

	if clamped_src.size.x <= 0 or clamped_src.size.y <= 0:
		return

	# Map clamped source back to draw coordinates.
	var scale: float = draw_rect.size.x / span
	var draw_left: float = (
		draw_rect.position.x + (clamped_src.position.x - src_rect.position.x) * scale
	)
	var draw_top: float = (
		draw_rect.position.y + (clamped_src.position.y - src_rect.position.y) * scale
	)
	var draw_w: float = clamped_src.size.x * scale
	var draw_h: float = clamped_src.size.y * scale

	(
		_draw_area
		. draw_texture_rect_region(
			_terrain_texture,
			Rect2(draw_left, draw_top, draw_w, draw_h),
			clamped_src,
		)
	)


## Draw creature dots on the minimap.
##
## Uses get_all_creature_positions_with_relations() for a single query that
## returns all alive creatures with their diplomatic relation to the player.
## Draws in priority order: neutrals (bottom), friendly, hostile (top),
## selected creatures last (topmost).
func _draw_creatures(draw_rect: Rect2, center: Vector2, span: float, cam_y: float) -> void:
	var data: Dictionary = _bridge.get_all_creature_positions_with_relations(_render_tick)
	if data.is_empty():
		return

	var positions: PackedVector3Array = data.get("positions", PackedVector3Array())
	var ids: Array = data.get("ids", [])
	var relations: PackedByteArray = data.get("relations", PackedByteArray())

	var half_span: float = span * 0.5
	var scale: float = draw_rect.size.x / span

	# Determine dot size based on zoom level.
	var dot_radius: float
	if span <= 64.0:
		dot_radius = 3.0
	elif span <= 128.0:
		dot_radius = 2.0
	elif span <= 256.0:
		dot_radius = 1.5
	else:
		dot_radius = 1.0

	# Build a set of selected IDs for quick lookup.
	var selected_set := {}
	for sid in _selected_ids:
		selected_set[sid] = true

	# Sort into draw-priority buckets by relation. Selected creatures deferred.
	# relation bytes: 0=friendly, 1=hostile, 2=neutral
	var neutral_draws: Array = []
	var friendly_draws: Array = []
	var hostile_draws: Array = []
	var selected_draws: Array[Vector2] = []

	for i in range(positions.size()):
		var pos: Vector3 = positions[i]
		var rel_x: float = pos.x - center.x + half_span
		var rel_z: float = pos.z - center.y + half_span

		# Skip if outside the visible area.
		if rel_x < 0 or rel_x > span or rel_z < 0 or rel_z > span:
			continue

		var screen_x: float = draw_rect.position.x + rel_x * scale
		var screen_z: float = draw_rect.position.y + rel_z * scale
		var screen_pos := Vector2(screen_x, screen_z)

		var cid: String = ids[i] if i < ids.size() else ""
		if selected_set.has(cid):
			selected_draws.append(screen_pos)
			continue

		# Determine base color from relation.
		var relation: int = relations[i] if i < relations.size() else 2
		var color: Color
		if relation == 0:
			color = COLOR_PLAYER
		elif relation == 1:
			color = COLOR_HOSTILE
		else:
			color = COLOR_NEUTRAL

		# Ghostly if above camera height.
		if pos.y > cam_y + 2.0:
			color.a = 0.25

		# Bucket by priority for draw ordering.
		if relation == 2:
			neutral_draws.append([screen_pos, color])
		elif relation == 0:
			friendly_draws.append([screen_pos, color])
		else:
			hostile_draws.append([screen_pos, color])

	# Draw in priority order: neutrals -> friendly -> hostiles -> selected.
	for entry in neutral_draws:
		_draw_area.draw_circle(entry[0], dot_radius, entry[1])
	for entry in friendly_draws:
		_draw_area.draw_circle(entry[0], dot_radius, entry[1])
	for entry in hostile_draws:
		_draw_area.draw_circle(entry[0], dot_radius, entry[1])
	for screen_pos in selected_draws:
		_draw_area.draw_circle(screen_pos, dot_radius + 1.5, COLOR_SELECTED)


## Draw the camera's viewport footprint on the minimap.
func _draw_frustum(draw_rect: Rect2, center: Vector2, span: float) -> void:
	if not _camera_pivot:
		return

	var camera: Camera3D = _camera_pivot.get_child(0) as Camera3D
	if not camera:
		return

	# Project the four corners of the near plane onto the ground (Y = camera focal Y).
	var viewport_size := get_viewport_rect().size
	var ground_y: float = _camera_pivot.position.y

	var half_span: float = span * 0.5
	var scale: float = draw_rect.size.x / span

	var corners_2d: PackedVector2Array = PackedVector2Array()
	var screen_corners := [
		Vector2(0, 0),
		Vector2(viewport_size.x, 0),
		Vector2(viewport_size.x, viewport_size.y),
		Vector2(0, viewport_size.y),
	]

	for sc in screen_corners:
		var origin := camera.project_ray_origin(sc)
		var direction := camera.project_ray_normal(sc)

		# Intersect with the horizontal plane at ground_y.
		if absf(direction.y) < 0.0001:
			continue
		var t: float = (ground_y - origin.y) / direction.y
		if t < 0:
			# Ray points away from ground — use a far clamp.
			t = 500.0
		var hit := origin + direction * minf(t, 500.0)

		# Convert to minimap screen space.
		var rel_x: float = hit.x - center.x + half_span
		var rel_z: float = hit.z - center.y + half_span
		var sx: float = draw_rect.position.x + rel_x * scale
		var sz: float = draw_rect.position.y + rel_z * scale

		# Clamp to draw area bounds.
		sx = clampf(sx, draw_rect.position.x, draw_rect.position.x + draw_rect.size.x)
		sz = clampf(sz, draw_rect.position.y, draw_rect.position.y + draw_rect.size.y)

		corners_2d.append(Vector2(sx, sz))

	if corners_2d.size() >= 3:
		# Draw outline by connecting consecutive corners.
		for i in range(corners_2d.size()):
			var next_i: int = (i + 1) % corners_2d.size()
			_draw_area.draw_line(corners_2d[i], corners_2d[next_i], COLOR_FRUSTUM, 1.0)

	# Also draw a small crosshair at the camera focal point.
	var focal_x: float = (
		draw_rect.position.x + (_camera_pivot.position.x - center.x + half_span) * scale
	)
	var focal_z: float = (
		draw_rect.position.y + (_camera_pivot.position.z - center.y + half_span) * scale
	)
	var ch_size := 3.0
	if (
		focal_x >= draw_rect.position.x
		and focal_x <= draw_rect.position.x + draw_rect.size.x
		and focal_z >= draw_rect.position.y
		and focal_z <= draw_rect.position.y + draw_rect.size.y
	):
		(
			_draw_area
			. draw_line(
				Vector2(focal_x - ch_size, focal_z),
				Vector2(focal_x + ch_size, focal_z),
				COLOR_FRUSTUM,
				1.0,
			)
		)
		(
			_draw_area
			. draw_line(
				Vector2(focal_x, focal_z - ch_size),
				Vector2(focal_x, focal_z + ch_size),
				COLOR_FRUSTUM,
				1.0,
			)
		)


func _make_icon_button(icon_type: String, tooltip: String) -> Button:
	var btn := Button.new()
	btn.text = ""
	btn.tooltip_text = tooltip
	btn.custom_minimum_size = Vector2(BTN_SIZE, BTN_SIZE)
	btn.size = Vector2(BTN_SIZE, BTN_SIZE)
	# Compact, semi-transparent style.
	for state_name in ["normal", "hover", "pressed", "disabled"]:
		var style := StyleBoxFlat.new()
		if state_name == "hover":
			style.bg_color = Color(0.25, 0.25, 0.25, 0.85)
		elif state_name == "pressed":
			style.bg_color = Color(0.1, 0.1, 0.1, 0.9)
		else:
			style.bg_color = Color(0.15, 0.15, 0.15, 0.7)
		style.set_corner_radius_all(2)
		style.set_content_margin_all(0)
		btn.add_theme_stylebox_override(state_name, style)

	# Custom draw child for procedural icon.
	var icon := Control.new()
	icon.custom_minimum_size = Vector2(BTN_SIZE, BTN_SIZE)
	icon.size = Vector2(BTN_SIZE, BTN_SIZE)
	icon.mouse_filter = Control.MOUSE_FILTER_IGNORE
	icon.set_meta("icon_type", icon_type)
	icon.draw.connect(_draw_icon.bind(icon))
	btn.add_child(icon)
	return btn


## Draw a procedural icon inside a button's icon Control.
func _draw_icon(icon: Control) -> void:
	var icon_type: String = icon.get_meta("icon_type", "")
	var s: float = icon.size.x
	var color := Color(0.8, 0.8, 0.8)

	if icon_type == "tree":
		# Simple tree: triangle canopy + rectangle trunk.
		var cx: float = s * 0.5
		# Canopy triangle.
		var canopy := PackedVector2Array(
			[
				Vector2(cx, s * 0.15),
				Vector2(s * 0.2, s * 0.65),
				Vector2(s * 0.8, s * 0.65),
			]
		)
		icon.draw_colored_polygon(canopy, Color(0.3, 0.6, 0.25))
		# Trunk rectangle.
		(
			icon
			. draw_rect(
				Rect2(cx - s * 0.08, s * 0.65, s * 0.16, s * 0.22),
				Color(0.45, 0.3, 0.15),
			)
		)
	elif icon_type == "eye":
		# Simple eye: almond shape + pupil circle.
		var cx: float = s * 0.5
		var cy: float = s * 0.5
		var rx: float = s * 0.35
		var ry: float = s * 0.18
		# Draw almond shape with arcs (approximate with polygon).
		var points := PackedVector2Array()
		for i in range(13):
			var t: float = float(i) / 12.0
			var angle: float = -PI + t * PI
			points.append(Vector2(cx + cos(angle) * rx, cy + sin(angle) * ry))
		for i in range(13):
			var t: float = float(i) / 12.0
			var angle: float = t * PI
			points.append(Vector2(cx + cos(angle) * rx, cy + sin(angle) * ry))
		icon.draw_colored_polygon(points, Color(0.7, 0.7, 0.8))
		# Pupil.
		icon.draw_circle(Vector2(cx, cy), s * 0.1, Color(0.1, 0.1, 0.2))
	elif icon_type == "+":
		# Plus sign.
		var cx: float = s * 0.5
		var cy: float = s * 0.5
		var arm: float = s * 0.28
		var thick: float = s * 0.12
		icon.draw_rect(Rect2(cx - arm, cy - thick, arm * 2, thick * 2), color)
		icon.draw_rect(Rect2(cx - thick, cy - arm, thick * 2, arm * 2), color)
	elif icon_type == "-":
		# Minus sign.
		var cx: float = s * 0.5
		var cy: float = s * 0.5
		var arm: float = s * 0.28
		var thick: float = s * 0.12
		icon.draw_rect(Rect2(cx - arm, cy - thick, arm * 2, thick * 2), color)
