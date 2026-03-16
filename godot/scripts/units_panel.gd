## Full-screen units panel listing all creatures with sprites, names, and activity.
##
## Displays a scrollable roster grouped by species: elves first (alphabetically
## by Vaelith name), then other species grouped alphabetically. Each row shows
## the creature's sprite (32x32), name (or "Species #N" for unnamed creatures),
## and current activity (Idle, Building, Eating, Sleeping, Walking, Furnishing).
##
## Clicking a creature row emits creature_clicked so the selection controller
## can select that creature and open the creature info panel on top of this one.
##
## Data flow: main.gd calls update_creatures(data) each frame while the panel
## is visible, passing the result of bridge.get_all_creatures_summary(). The
## panel uses a reconciliation pattern — it maintains a dictionary mapping
## creature_id (UUID string) keys to row nodes, creating/updating/removing
## rows as creatures appear and disappear.
##
## Sprites are cached in a dictionary keyed by creature_id and generated
## via SpriteGenerator on first encounter.
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

## Map species names to plural section titles.
const SECTION_TITLES = {
	"Elf": "Elves",
	"Boar": "Boars",
	"Capybara": "Capybaras",
	"Deer": "Deer",
	"Elephant": "Elephants",
	"Monkey": "Monkeys",
	"Squirrel": "Squirrels",
}

## Maps creature_id (UUID string) -> HBoxContainer row node.
var _creature_rows: Dictionary = {}
## Cached sprite textures keyed by creature_id.
var _sprite_cache: Dictionary = {}
## Section containers keyed by species name.
var _sections: Dictionary = {}
## Section headers keyed by species name, for visibility toggling.
var _section_headers: Dictionary = {}
## The scrollable content container.
var _content_vbox: VBoxContainer
## Empty-state label.
var _empty_label: Label
## The species ordering for sections (elves first, then alphabetical).
var _species_order: Array = ["Elf", "Boar", "Capybara", "Deer", "Elephant", "Monkey", "Squirrel"]


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

	# Pre-create section headers and containers for each species.
	for sp in _species_order:
		var header := Label.new()
		header.text = _section_title(sp)
		header.add_theme_font_size_override("font_size", 20)
		header.add_theme_color_override("font_color", Color(0.85, 0.80, 0.65))
		_content_vbox.add_child(header)
		_section_headers[sp] = header

		var container := VBoxContainer.new()
		container.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		container.add_theme_constant_override("separation", 2)
		_content_vbox.add_child(container)
		_sections[sp] = container

	# Start hidden.
	visible = false


func _section_title(species: String) -> String:
	return SECTION_TITLES.get(species, species)


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


## Called each frame by main.gd with the result of bridge.get_all_creatures_summary().
## Uses reconciliation: creates new rows, updates existing ones, removes stale.
func update_creatures(data: Array) -> void:
	var seen_keys: Dictionary = {}

	for i in data.size():
		var entry: Dictionary = data[i]
		var sp: String = entry.get("species", "")
		var creature_key: String = entry.get("creature_id", "")
		seen_keys[creature_key] = true

		if _creature_rows.has(creature_key):
			_update_row(_creature_rows[creature_key], entry)
		else:
			var row := _create_row(entry)
			var section: VBoxContainer = _sections.get(sp)
			if section:
				section.add_child(row)
			_creature_rows[creature_key] = row

	# Remove rows for creatures no longer present.
	var to_remove: Array = []
	for key in _creature_rows:
		if not seen_keys.has(key):
			to_remove.append(key)
	for key in to_remove:
		var row: HBoxContainer = _creature_rows[key]
		row.get_parent().remove_child(row)
		row.queue_free()
		_creature_rows.erase(key)
		_sprite_cache.erase(key)

	# Show/hide per-species section headers based on whether they have rows.
	for sp in _species_order:
		var section: VBoxContainer = _sections[sp]
		var has_creatures: bool = section.get_child_count() > 0
		section.visible = has_creatures
		var header: Label = _section_headers[sp]
		header.visible = has_creatures

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
	tex_rect.texture = _get_sprite(cid, sp, idx)
	row.add_child(tex_rect)

	# Name label.
	var name_label := Label.new()
	name_label.name = "NameLabel"
	name_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	name_label.custom_minimum_size.x = 200
	row.add_child(name_label)

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
	var creature_name: String = entry.get("name", "")
	var name_meaning: String = entry.get("name_meaning", "")
	var task_kind: String = entry.get("task_kind", "")

	var name_label: Label = row.find_child("NameLabel", false, false)
	if name_label:
		if creature_name != "":
			if name_meaning != "":
				name_label.text = "%s (%s)" % [creature_name, name_meaning]
			else:
				name_label.text = creature_name
		else:
			name_label.text = "%s #%d" % [sp, idx + 1]

	var activity_label: Label = row.find_child("ActivityLabel", false, false)
	if activity_label:
		activity_label.text = ACTIVITY_LABELS.get(task_kind, "Idle")


func _get_sprite(creature_id: String, species: String, index: int) -> ImageTexture:
	if _sprite_cache.has(creature_id):
		return _sprite_cache[creature_id]
	var tex := SpriteGenerator.species_sprite(species, index)
	_sprite_cache[creature_id] = tex
	return tex
