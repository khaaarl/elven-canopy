## Panel displayed when multiple creatures are selected via box-select,
## Shift+click, or double-click group select.
##
## Shows a scrollable list of selected creatures, each with their sprite,
## name, species, and current activity. Clicking a row selects just that
## creature (switching to the single-creature info panel). The close button
## deselects all.
##
## Built programmatically as a PanelContainer anchored to the right edge,
## following the same pattern as creature_info_panel.gd. Updated every frame
## by main.gd while visible.
##
## See also: selection_controller.gd for multi-select state,
## creature_info_panel.gd for single-creature display,
## elven_canopy_sprites (Rust crate) for procedural sprite generation,
## main.gd for wiring and per-frame refresh.

extends PanelContainer

signal creature_clicked(creature_id: String)
signal panel_closed

## Map task_kind strings to human-readable activity labels.
const ACTIVITY_LABELS = {
	"": "Idle",
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
var _title_label: Label
var _rows_container: VBoxContainer
var _creature_ids: Array = []
var _sprite_cache: Dictionary = {}
var _render_tick: float = 0.0
## Maps creature_id -> HBoxContainer row node for reconciliation.
var _row_nodes: Dictionary = {}
## Maps row node instance_id -> creature_id for click handling.
var _row_cids: Dictionary = {}


func _ready() -> void:
	set_anchors_preset(PRESET_RIGHT_WIDE)
	custom_minimum_size.x = 320
	# Force full viewport height — PanelContainer shrinks to content minimum,
	# and ScrollContainer has zero minimum height, so without this the panel
	# would only be ~75px tall (just the header).
	_match_viewport_height()
	get_viewport().size_changed.connect(_match_viewport_height)

	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	margin.add_theme_constant_override("margin_right", 12)
	margin.add_theme_constant_override("margin_top", 12)
	margin.add_theme_constant_override("margin_bottom", 12)
	add_child(margin)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 8)
	margin.add_child(vbox)

	# Header with title and close button.
	var header := HBoxContainer.new()
	vbox.add_child(header)

	_title_label = Label.new()
	_title_label.text = "Selected"
	_title_label.add_theme_font_size_override("font_size", 20)
	_title_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(_title_label)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close_pressed)
	header.add_child(close_btn)

	var sep := HSeparator.new()
	vbox.add_child(sep)

	# Scrollable list of creature rows.
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	vbox.add_child(scroll)

	_rows_container = VBoxContainer.new()
	_rows_container.add_theme_constant_override("separation", 4)
	_rows_container.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.add_child(_rows_container)

	visible = false


func setup(bridge: SimBridge) -> void:
	_bridge = bridge


func set_render_tick(tick: float) -> void:
	_render_tick = tick


## Show the panel with the given set of creature IDs.
func show_group(ids: Array) -> void:
	_creature_ids = ids.duplicate()
	_title_label.text = "Selected (%d)" % ids.size()
	_rebuild_rows()
	visible = true


## Refresh all rows with current creature data. Called per-frame by main.gd.
func update_group(ids: Array) -> void:
	# If the selection changed, rebuild.
	if ids.size() != _creature_ids.size():
		_creature_ids = ids.duplicate()
		_title_label.text = "Selected (%d)" % ids.size()
		_rebuild_rows()
		return

	var changed := false
	for i in ids.size():
		if ids[i] != _creature_ids[i]:
			changed = true
			break
	if changed:
		_creature_ids = ids.duplicate()
		_title_label.text = "Selected (%d)" % ids.size()
		_rebuild_rows()
		return

	# Same IDs — just refresh data.
	for cid in _creature_ids:
		if _row_nodes.has(cid):
			var info: Dictionary = _bridge.get_creature_info_by_id(cid, _render_tick)
			if not info.is_empty():
				_update_row(_row_nodes[cid], info)


func hide_panel() -> void:
	visible = false


## Rebuild all rows from scratch.
func _rebuild_rows() -> void:
	# Clear existing rows.
	for child in _rows_container.get_children():
		_rows_container.remove_child(child)
		child.queue_free()
	_row_nodes.clear()
	_row_cids.clear()

	for cid in _creature_ids:
		var info: Dictionary = _bridge.get_creature_info_by_id(cid, _render_tick)
		if info.is_empty():
			continue
		var row := _create_row(cid, info)
		_rows_container.add_child(row)
		_row_nodes[cid] = row


func _create_row(cid: String, info: Dictionary) -> HBoxContainer:
	var species: String = info.get("species", "")
	var species_index: int = info.get("species_index", 0)

	# Row is an HBoxContainer; the whole row is clickable via gui_input.
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	row.custom_minimum_size.y = 40
	row.mouse_filter = Control.MOUSE_FILTER_STOP
	_row_cids[row.get_instance_id()] = cid
	row.gui_input.connect(_on_row_input.bind(row))

	# Sprite.
	var tex_rect := TextureRect.new()
	tex_rect.name = "Sprite"
	tex_rect.custom_minimum_size = Vector2(32, 32)
	tex_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	tex_rect.texture = _get_sprite(cid, species, species_index)
	tex_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	row.add_child(tex_rect)

	# Name + species label.
	var name_label := Label.new()
	name_label.name = "NameLabel"
	name_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	name_label.custom_minimum_size.x = 120
	name_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	row.add_child(name_label)

	# Activity label.
	var activity_label := Label.new()
	activity_label.name = "ActivityLabel"
	activity_label.custom_minimum_size.x = 80
	activity_label.add_theme_color_override("font_color", Color(0.7, 0.7, 0.6))
	activity_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	row.add_child(activity_label)

	_update_row(row, info)
	return row


func _on_row_input(event: InputEvent, row: HBoxContainer) -> void:
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_LEFT and mb.pressed:
			var cid: String = _row_cids.get(row.get_instance_id(), "")
			if cid != "":
				creature_clicked.emit(cid)


func _update_row(row: HBoxContainer, info: Dictionary) -> void:
	var species: String = info.get("species", "")
	var creature_name: String = info.get("name", "")
	var task_kind: String = info.get("task_kind", "")

	var name_label: Label = row.find_child("NameLabel", true, false)
	if name_label:
		if creature_name != "":
			name_label.text = "%s (%s)" % [creature_name, species]
		else:
			name_label.text = species

	var activity_label: Label = row.find_child("ActivityLabel", true, false)
	if activity_label:
		activity_label.text = ACTIVITY_LABELS.get(task_kind, "Idle")


func _get_sprite(creature_id: String, species: String, species_index: int) -> ImageTexture:
	if _sprite_cache.has(creature_id):
		return _sprite_cache[creature_id]
	# Use the per-species index as seed, matching renderers and units_panel.
	var tex := SpriteGenerator.species_sprite(species, species_index)
	_sprite_cache[creature_id] = tex
	return tex


func _match_viewport_height() -> void:
	custom_minimum_size.y = get_viewport().get_visible_rect().size.y


func _on_close_pressed() -> void:
	panel_closed.emit()
