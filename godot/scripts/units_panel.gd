## Full-screen units panel listing all creatures with sprites, names, and activity.
##
## Displays a scrollable roster in three major groups based on each creature's
## diplomatic relation to the player: Your Civilization (friendly), Neutral,
## and Hostile. Within each group, creatures are grouped by species sorted by
## display_order (from SpeciesData config), then by name/index within a species.
## Groups and species sections appear dynamically — empty ones are hidden.
##
## Clicking a creature row emits creature_clicked so the selection controller
## can select that creature and open the creature info panel on top of this one.
##
## Data flow: main.gd calls update_creatures(data) each frame while the panel
## is visible, passing the result of bridge.get_all_creatures_summary(). Each
## entry includes a "player_relation" field ("friendly"/"hostile"/"neutral").
## The panel uses a reconciliation pattern — it maintains a dictionary mapping
## creature_id (UUID string) keys to row nodes, creating/updating/removing
## rows as creatures appear and disappear.
##
## Sprites are fetched from the central CreatureSprites cache, which is
## populated by creature_renderer.gd and falls back to on-demand generation.
##
## Signals:
## - creature_clicked(creature_id) — select and show info for a creature
## - panel_closed — emitted when the panel is hidden (ESC or close button)
##
## ESC handling: when visible, consumes ESC in _unhandled_input and closes.
## This sits in the ESC precedence chain between structure_list_panel and
## selection_controller (see main.gd docstring).
##
## See also: main.gd (creates and wires this panel), sim_bridge.rs for
## get_all_creatures_summary(), action_toolbar.gd for the "Units [U]" button,
## task_panel.gd / structure_list_panel.gd for similar full-screen overlay
## patterns, elven_canopy_sprites (Rust crate) for creature sprite generation.

extends ColorRect

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

## The three relation groups in display order.
const RELATION_GROUPS: Array = ["friendly", "neutral", "hostile"]
const RELATION_TITLES: Dictionary = {
	"friendly": "Your Civilization",
	"neutral": "Neutral",
	"hostile": "Hostile",
}
const RELATION_COLORS: Dictionary = {
	"friendly": Color(0.65, 0.85, 0.65),
	"neutral": Color(0.85, 0.80, 0.65),
	"hostile": Color(0.95, 0.50, 0.45),
}

## Maps creature_id (UUID string) -> HBoxContainer row node.
var _creature_rows: Dictionary = {}
## Reference to SimBridge for on-demand sprite generation via CreatureSprites.
var _bridge: SimBridge
## Species display info: species_name -> { plural_name, display_order }.
var _species_info: Dictionary = {}
## The scrollable content container.
var _content_vbox: VBoxContainer
## Empty-state label.
var _empty_label: Label

## Per-relation-group containers: relation -> VBoxContainer.
var _group_containers: Dictionary = {}
## Per-relation-group headers: relation -> Label.
var _group_headers: Dictionary = {}
## Per-species section containers: "relation:species" -> VBoxContainer.
var _species_sections: Dictionary = {}
## Per-species section headers: "relation:species" -> Label.
var _species_headers: Dictionary = {}


func _ready() -> void:
	# Full-screen semi-transparent overlay.
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	color = Color(0.10, 0.12, 0.08, 0.90)

	var margin := MarginContainer.new()
	margin.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	margin.add_theme_constant_override("margin_left", 60)
	margin.add_theme_constant_override("margin_right", 60)
	margin.add_theme_constant_override("margin_top", 40)
	margin.add_theme_constant_override("margin_bottom", 40)
	add_child(margin)

	var outer_vbox := VBoxContainer.new()
	outer_vbox.add_theme_constant_override("separation", 12)
	margin.add_child(outer_vbox)

	# Header row.
	var header_hbox := HBoxContainer.new()
	outer_vbox.add_child(header_hbox)

	var title := Label.new()
	title.text = "Units"
	title.add_theme_font_size_override("font_size", 28)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header_hbox.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.custom_minimum_size = Vector2(40, 40)
	close_btn.pressed.connect(hide_panel)
	header_hbox.add_child(close_btn)

	var sep := HSeparator.new()
	outer_vbox.add_child(sep)

	# Empty-state label (shown when no creatures exist).
	_empty_label = Label.new()
	_empty_label.text = "No creatures."
	_empty_label.add_theme_font_size_override("font_size", 18)
	_empty_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	outer_vbox.add_child(_empty_label)

	# Scrollable creature list.
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	outer_vbox.add_child(scroll)

	_content_vbox = VBoxContainer.new()
	_content_vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_content_vbox.add_theme_constant_override("separation", 4)
	scroll.add_child(_content_vbox)

	# Create the three relation group headers and containers.
	for relation in RELATION_GROUPS:
		var group_header := Label.new()
		group_header.text = RELATION_TITLES[relation]
		group_header.add_theme_font_size_override("font_size", 24)
		group_header.add_theme_color_override(
			"font_color", RELATION_COLORS.get(relation, Color.WHITE)
		)
		_content_vbox.add_child(group_header)
		_group_headers[relation] = group_header

		var group_container := VBoxContainer.new()
		group_container.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		group_container.add_theme_constant_override("separation", 2)
		_content_vbox.add_child(group_container)
		_group_containers[relation] = group_container

	# Start hidden.
	visible = false


## Initialize species display info from bridge data.
func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	for entry in bridge.get_species_display_info():
		var sp_name: String = entry.get("name", "")
		if sp_name != "":
			_species_info[sp_name] = {
				"plural_name": entry.get("plural_name", sp_name),
				"display_order": entry.get("display_order", 0),
			}


func show_panel() -> void:
	visible = true


func hide_panel() -> void:
	visible = false
	panel_closed.emit()


func toggle() -> void:
	if visible:
		hide_panel()
	else:
		show_panel()


func _unhandled_input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey:
		var key := event as InputEventKey
		if key.pressed and key.keycode == KEY_ESCAPE:
			hide_panel()
			get_viewport().set_input_as_handled()


## Return the display_order for a species, defaulting to a high value.
func _species_display_order(species: String) -> int:
	var info: Dictionary = _species_info.get(species, {})
	return info.get("display_order", 9999)


## Return the plural display name for a species.
func _species_plural(species: String) -> String:
	var info: Dictionary = _species_info.get(species, {})
	return info.get("plural_name", species)


## Get or create a species section (header + container) within a relation group.
func _ensure_species_section(relation: String, species: String) -> VBoxContainer:
	var key := relation + ":" + species
	if _species_sections.has(key):
		return _species_sections[key]

	# Create new species section.
	var group_container: VBoxContainer = _group_containers[relation]

	var header := Label.new()
	header.text = _species_plural(species)
	header.add_theme_font_size_override("font_size", 20)
	header.add_theme_color_override("font_color", Color(0.85, 0.80, 0.65))
	header.set_meta("display_order", _species_display_order(species))

	var section := VBoxContainer.new()
	section.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	section.add_theme_constant_override("separation", 2)

	# Insert in display_order position among existing sections.
	var new_order := _species_display_order(species)
	var insert_idx := 0
	var child_count := group_container.get_child_count()
	# Children alternate: header, section, header, section...
	var i := 0
	while i < child_count:
		var existing_header: Control = group_container.get_child(i)
		var existing_order: int = existing_header.get_meta("display_order", 9999)
		if existing_order > new_order:
			insert_idx = i
			break
		i += 2
		insert_idx = i

	group_container.add_child(header)
	group_container.move_child(header, insert_idx)
	group_container.add_child(section)
	group_container.move_child(section, insert_idx + 1)

	_species_headers[key] = header
	_species_sections[key] = section
	return section


## Called each frame by main.gd with the result of bridge.get_all_creatures_summary().
## Uses reconciliation: creates new rows, updates existing ones, removes stale.
func update_creatures(data: Array, bridge: SimBridge = null) -> void:
	if bridge:
		_bridge = bridge
	var seen_keys: Dictionary = {}

	for i in data.size():
		var entry: Dictionary = data[i]
		var sp: String = entry.get("species", "")
		var relation: String = entry.get("player_relation", "neutral")
		var creature_key: String = entry.get("creature_id", "")
		seen_keys[creature_key] = true

		if _creature_rows.has(creature_key):
			var row_info: Dictionary = _creature_rows[creature_key]
			var row: HBoxContainer = row_info["row"]
			var old_relation: String = row_info["relation"]
			var old_species: String = row_info["species"]
			_update_row(row, entry)
			# If relation or species changed, move the row.
			if old_relation != relation or old_species != sp:
				row.get_parent().remove_child(row)
				var section := _ensure_species_section(relation, sp)
				section.add_child(row)
				row_info["relation"] = relation
				row_info["species"] = sp
		else:
			var row := _create_row(entry)
			var section := _ensure_species_section(relation, sp)
			section.add_child(row)
			_creature_rows[creature_key] = {
				"row": row,
				"relation": relation,
				"species": sp,
			}

	# Remove rows for creatures no longer present.
	var to_remove: Array = []
	for key in _creature_rows:
		if not seen_keys.has(key):
			to_remove.append(key)
	for key in to_remove:
		var row_info: Dictionary = _creature_rows[key]
		var row: HBoxContainer = row_info["row"]
		row.get_parent().remove_child(row)
		row.queue_free()
		_creature_rows.erase(key)

	# Show/hide species sections based on whether they have rows.
	for key in _species_sections:
		var section: VBoxContainer = _species_sections[key]
		var has_creatures: bool = section.get_child_count() > 0
		section.visible = has_creatures
		var header: Label = _species_headers[key]
		header.visible = has_creatures

	# Show/hide relation group headers based on whether any species in the group
	# has creatures.
	for relation in RELATION_GROUPS:
		var group_container: VBoxContainer = _group_containers[relation]
		var has_any := false
		for child_idx in group_container.get_child_count():
			if group_container.get_child(child_idx).visible:
				has_any = true
				break
		_group_headers[relation].visible = has_any
		group_container.visible = has_any

	# Top-level empty state.
	var all_empty := _creature_rows.is_empty()
	_empty_label.visible = all_empty
	_content_vbox.visible = not all_empty


func _create_row(entry: Dictionary) -> HBoxContainer:
	var sp: String = entry.get("species", "")
	var idx: int = entry.get("index", 0)
	var cid: String = entry.get("creature_id", "")

	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	row.custom_minimum_size.y = 36

	# Sprite texture.
	var tex_rect := TextureRect.new()
	tex_rect.name = "Sprite"
	tex_rect.custom_minimum_size = Vector2(32, 32)
	tex_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	if _bridge:
		tex_rect.texture = CreatureSprites.get_sprite(_bridge, cid)

	row.add_child(tex_rect)

	# Name label.
	var name_label := Label.new()
	name_label.name = "NameLabel"
	name_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	name_label.custom_minimum_size.x = 200
	row.add_child(name_label)

	# Path label (F-path-ui). Blank for non-elf creatures.
	var path_label := Label.new()
	path_label.name = "PathLabel"
	path_label.custom_minimum_size.x = 70
	path_label.add_theme_color_override("font_color", Color(0.6, 0.75, 0.9))
	row.add_child(path_label)

	# Activity label.
	var activity_label := Label.new()
	activity_label.name = "ActivityLabel"
	activity_label.custom_minimum_size.x = 100
	activity_label.add_theme_color_override("font_color", Color(0.7, 0.7, 0.6))
	row.add_child(activity_label)

	# Click handler — use a Button styled to be invisible covering the row.
	var click_btn := Button.new()
	click_btn.name = "ClickBtn"
	click_btn.text = ""
	click_btn.flat = true
	click_btn.size_flags_horizontal = Control.SIZE_SHRINK_END
	click_btn.custom_minimum_size = Vector2(40, 32)
	click_btn.text = ">"
	click_btn.pressed.connect(func(): creature_clicked.emit(cid))
	row.add_child(click_btn)

	_update_row(row, entry)
	return row


func _update_row(row: HBoxContainer, entry: Dictionary) -> void:
	var sp: String = entry.get("species", "")
	var idx: int = entry.get("index", 0)
	var cid: String = entry.get("creature_id", "")
	var creature_name: String = entry.get("name", "")
	var name_meaning: String = entry.get("name_meaning", "")
	var task_kind: String = entry.get("task_kind", "")

	# Refresh sprite texture (equipment changes, etc.).
	var tex_rect: TextureRect = row.find_child("Sprite", false, false)
	if tex_rect and _bridge and cid != "":
		tex_rect.texture = CreatureSprites.get_sprite(_bridge, cid)

	var name_label: Label = row.find_child("NameLabel", false, false)
	if name_label:
		if creature_name != "":
			if name_meaning != "":
				name_label.text = "%s (%s)" % [creature_name, name_meaning]
			else:
				name_label.text = creature_name
		else:
			name_label.text = "%s #%d" % [sp, idx + 1]

	var path_label: Label = row.find_child("PathLabel", false, false)
	if path_label:
		path_label.text = entry.get("path_short", "")

	var activity_label: Label = row.find_child("ActivityLabel", false, false)
	if activity_label:
		activity_label.text = ACTIVITY_LABELS.get(task_kind, "Idle")
