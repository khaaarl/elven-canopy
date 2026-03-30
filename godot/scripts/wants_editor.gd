## Reusable wants list editor (item kind + material filter + quantity).
##
## Displays a scrollable list of wants with per-row quantity controls
## (-/+ buttons, editable quantity field, remove button) and a two-step
## add-item picker (item kind → material filter). Used by both building
## logistics (structure_info_panel.gd) and military equipment
## (military_panel.gd).
##
## When `enforce_unique_equip_slots` is true (military usage), the editor
## rejects adding a wearable item if another item already occupies the same
## equip slot, showing a temporary error label.
##
## The item/material pickers are placed inline by default. Set
## `picker_container` to a VBoxContainer to host them externally (e.g. in a
## middle-column panel). The external container must already be in the tree.
##
## Emits `wants_changed(wants_json: String)` whenever the list changes.
## The parent is responsible for routing this signal to the bridge.
##
## See also: structure_info_panel.gd (logistics usage),
## military_panel.gd (equipment usage), sim_bridge.rs (bridge API).

extends VBoxContainer

signal wants_changed(wants_json: String)
## Emitted when the picker panel should be shown (true) or hidden (false).
## Only relevant when `picker_container` is set.
signal picker_visibility_changed(is_visible: bool)

## Default quantity added when the user picks a new item kind. Callers can
## override this (e.g. 10 for logistics, 1 for equipment).
var default_add_quantity: int = 10

## When true, reject adding a wearable item if another item already occupies
## the same equip slot. Used by military equipment (not logistics).
var enforce_unique_equip_slots: bool = false

## Optional external container for the item/material pickers. When set, the
## pickers are added there instead of inline. The parent panel is responsible
## for creating and positioning this container (e.g. in a middle column).
var picker_container: VBoxContainer = null

var _item_kinds: Array = []
var _material_options: Dictionary = {}
## Maps item kind name → equip slot name (e.g. "Helmet" → "Head"). Built
## from picker data; empty for non-wearable items.
var _equip_slot_map: Dictionary = {}

var _wants_vbox: VBoxContainer
var _add_button: Button
var _item_picker: VBoxContainer
var _material_picker: VBoxContainer
var _error_label: Label
var _pending_item_kind: String = ""
var _pickers_added: bool = false
## Guard against spurious focus_exited signals during update_wants() rebuild.
var _rebuilding: bool = false
## Last wants data passed to update_wants(). Skip rebuild when unchanged —
## newly-created buttons have no valid layout rect until the next frame, so
## rebuilding every frame means clicks always fall through.
var _last_wants: Array = []


func _ready() -> void:
	add_theme_constant_override("separation", 4)

	# Wants list inside a scroll container.
	var wants_scroll := ScrollContainer.new()
	wants_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	add_child(wants_scroll)

	_wants_vbox = VBoxContainer.new()
	_wants_vbox.add_theme_constant_override("separation", 2)
	_wants_vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	wants_scroll.add_child(_wants_vbox)

	# Error label (hidden by default).
	_error_label = Label.new()
	_error_label.add_theme_color_override("font_color", Color(1.0, 0.3, 0.3))
	_error_label.visible = false
	_error_label.autowrap_mode = TextServer.AUTOWRAP_WORD
	add_child(_error_label)

	# Add item button.
	_add_button = Button.new()
	_add_button.text = "Add Item..."
	_add_button.pressed.connect(_on_add_pressed)
	add_child(_add_button)

	# Pickers (hidden by default, added inline). If picker_container is set,
	# they are reparented on first use via _ensure_pickers().
	_item_picker = VBoxContainer.new()
	_item_picker.visible = false
	add_child(_item_picker)

	_material_picker = VBoxContainer.new()
	_material_picker.visible = false
	add_child(_material_picker)


## If picker_container is set and pickers haven't been reparented yet,
## move them from inline to the external container.
func _ensure_pickers() -> void:
	if _pickers_added or not picker_container:
		return
	_pickers_added = true
	remove_child(_item_picker)
	remove_child(_material_picker)
	picker_container.add_child(_item_picker)
	picker_container.add_child(_material_picker)


## Set the available item kinds and material options (from bridge).
func set_picker_data(item_kinds: Array, material_options: Dictionary) -> void:
	_item_kinds = item_kinds
	_material_options = material_options
	_equip_slot_map.clear()
	for entry in item_kinds:
		var kind: String = entry.get("kind", "")
		var slot: String = entry.get("equip_slot", "")
		if not slot.is_empty():
			_equip_slot_map[kind] = slot


## Update the displayed wants list from an array of want dictionaries.
## Each dict: { kind: String, material_filter: String, target_quantity: int, label: String }
func update_wants(wants: Array) -> void:
	if wants == _last_wants:
		return
	_last_wants = wants.duplicate(true)
	_rebuilding = true
	for child in _wants_vbox.get_children():
		child.queue_free()
	for want in wants:
		var display_label: String = want.get("label", want.get("kind", "?"))
		var qty: int = want.get("target_quantity", 0)
		var kind: String = want.get("kind", "?")
		var mat_filter: String = want.get("material_filter", '"Any"')
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 2)
		row.set_meta("kind", kind)
		row.set_meta("material_filter", mat_filter)
		row.set_meta("quantity", qty)

		var label := Label.new()
		label.text = display_label
		label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		row.add_child(label)

		var minus_btn := Button.new()
		minus_btn.text = "-"
		minus_btn.custom_minimum_size.x = 28
		minus_btn.pressed.connect(_on_quantity_changed.bind(kind, mat_filter, -1))
		row.add_child(minus_btn)

		var qty_edit := LineEdit.new()
		qty_edit.text = str(qty)
		qty_edit.custom_minimum_size.x = 40
		qty_edit.alignment = HORIZONTAL_ALIGNMENT_CENTER
		qty_edit.text_submitted.connect(_on_quantity_submitted.bind(kind, mat_filter, qty_edit))
		qty_edit.focus_exited.connect(_on_quantity_focus_exited.bind(kind, mat_filter, qty_edit))
		row.add_child(qty_edit)

		var plus_btn := Button.new()
		plus_btn.text = "+"
		plus_btn.custom_minimum_size.x = 28
		plus_btn.pressed.connect(_on_quantity_changed.bind(kind, mat_filter, 1))
		row.add_child(plus_btn)

		var remove_btn := Button.new()
		remove_btn.text = "X"
		remove_btn.custom_minimum_size.x = 28
		remove_btn.pressed.connect(_on_want_removed.bind(kind, mat_filter))
		row.add_child(remove_btn)

		_wants_vbox.add_child(row)
	_rebuilding = false


## Hide the pickers and notify the parent to close the picker panel.
func hide_pickers() -> void:
	_item_picker.visible = false
	_material_picker.visible = false
	if picker_container:
		picker_visibility_changed.emit(false)


# --- Add item flow ---


func _on_add_pressed() -> void:
	_ensure_pickers()
	_material_picker.visible = false
	_error_label.visible = false
	var was_visible := _item_picker.visible
	_item_picker.visible = not was_visible
	if not was_visible:
		_populate_item_kind_picker()
	if picker_container:
		picker_visibility_changed.emit(not was_visible)


func _populate_item_kind_picker() -> void:
	for child in _item_picker.get_children():
		child.queue_free()
	for entry in _item_kinds:
		var kind: String = entry.get("kind", "")
		var label: String = entry.get("label", kind)
		var btn := Button.new()
		btn.text = label
		btn.pressed.connect(_on_item_kind_picked.bind(kind))
		_item_picker.add_child(btn)


func _on_item_kind_picked(kind: String) -> void:
	_item_picker.visible = false
	_pending_item_kind = kind

	var options: Array = _material_options.get(kind, [])
	if options.size() <= 1:
		_on_material_picked('"Any"')
		return

	_populate_material_picker(options)
	_material_picker.visible = true


func _populate_material_picker(options: Array) -> void:
	for child in _material_picker.get_children():
		child.queue_free()
	for entry in options:
		var filter_str: String = entry.get("filter", '"Any"')
		var label: String = entry.get("label", "?")
		var btn := Button.new()
		btn.text = label
		btn.pressed.connect(_on_material_picked.bind(filter_str))
		_material_picker.add_child(btn)


func _on_material_picked(filter_str: String) -> void:
	_material_picker.visible = false
	if picker_container:
		picker_visibility_changed.emit(false)
	_emit_wants_with_added(_pending_item_kind, filter_str, default_add_quantity)


func _on_want_removed(kind: String, filter_str: String) -> void:
	_error_label.visible = false
	_emit_wants_without(kind, filter_str)


func _on_quantity_changed(kind: String, filter_str: String, delta: int) -> void:
	_emit_wants_with_quantity(kind, filter_str, delta)


func _on_quantity_submitted(
	_text: String, kind: String, filter_str: String, edit: LineEdit
) -> void:
	var new_qty := maxi(int(edit.text), 1)
	edit.text = str(new_qty)
	edit.release_focus()
	_emit_wants_with_set_quantity(kind, filter_str, new_qty)


func _on_quantity_focus_exited(kind: String, filter_str: String, edit: LineEdit) -> void:
	if _rebuilding:
		return
	var new_qty := maxi(int(edit.text), 1)
	edit.text = str(new_qty)
	_emit_wants_with_set_quantity(kind, filter_str, new_qty)


# --- Slot conflict detection ---


## Returns the equip slot name occupied by the given item kind, or "" if
## the item is not wearable.
func _get_equip_slot(kind: String) -> String:
	return _equip_slot_map.get(kind, "")


## Check if adding `kind` would conflict with an existing want that
## occupies the same equip slot. Returns the conflicting kind name, or ""
## if no conflict.
func _find_slot_conflict(kind: String) -> String:
	var new_slot: String = _get_equip_slot(kind)
	if new_slot.is_empty():
		return ""
	for child in _wants_vbox.get_children():
		if child is HBoxContainer and child.has_meta("kind"):
			var existing_kind: String = child.get_meta("kind")
			if existing_kind == kind:
				continue
			var existing_slot: String = _get_equip_slot(existing_kind)
			if existing_slot == new_slot:
				return existing_kind
	return ""


# --- JSON emission ---


func _emit_wants_with_added(kind: String, filter_str: String, qty: int) -> void:
	# Check for equip slot conflicts (military mode only).
	if enforce_unique_equip_slots:
		var conflict: String = _find_slot_conflict(kind)
		if not conflict.is_empty():
			var slot_name: String = _get_equip_slot(kind)
			_error_label.text = (
				"Cannot add %s — %s slot already assigned to %s" % [kind, slot_name, conflict]
			)
			_error_label.visible = true
			return

	_error_label.visible = false
	var wants: Array = []
	var found := false
	var filter_val: Variant = JSON.parse_string(filter_str)
	for child in _wants_vbox.get_children():
		if child is HBoxContainer and child.has_meta("kind"):
			var k: String = child.get_meta("kind")
			var mf: String = child.get_meta("material_filter")
			var q: int = child.get_meta("quantity")
			var mf_val: Variant = JSON.parse_string(mf)
			if k == kind and _variant_eq(mf_val, filter_val):
				q += qty
				found = true
			wants.append(_make_want_entry(k, mf_val, q))
	if not found:
		wants.append(_make_want_entry(kind, filter_val, qty))
	wants_changed.emit(JSON.stringify(wants))


func _emit_wants_without(kind: String, filter_str: String) -> void:
	var wants: Array = []
	var filter_val: Variant = JSON.parse_string(filter_str)
	for child in _wants_vbox.get_children():
		if child is HBoxContainer and child.has_meta("kind"):
			var k: String = child.get_meta("kind")
			var mf: String = child.get_meta("material_filter")
			var q: int = child.get_meta("quantity")
			var mf_val: Variant = JSON.parse_string(mf)
			if k != kind or not _variant_eq(mf_val, filter_val):
				wants.append(_make_want_entry(k, mf_val, q))
	wants_changed.emit(JSON.stringify(wants))


func _emit_wants_with_quantity(kind: String, filter_str: String, delta: int) -> void:
	var wants: Array = []
	var filter_val: Variant = JSON.parse_string(filter_str)
	for child in _wants_vbox.get_children():
		if child is HBoxContainer and child.has_meta("kind"):
			var k: String = child.get_meta("kind")
			var mf: String = child.get_meta("material_filter")
			var q: int = child.get_meta("quantity")
			var mf_val: Variant = JSON.parse_string(mf)
			if k == kind and _variant_eq(mf_val, filter_val):
				q = maxi(q + delta, 1)
			wants.append(_make_want_entry(k, mf_val, q))
	wants_changed.emit(JSON.stringify(wants))


func _emit_wants_with_set_quantity(kind: String, filter_str: String, new_qty: int) -> void:
	var wants: Array = []
	var filter_val: Variant = JSON.parse_string(filter_str)
	for child in _wants_vbox.get_children():
		if child is HBoxContainer and child.has_meta("kind"):
			var k: String = child.get_meta("kind")
			var mf: String = child.get_meta("material_filter")
			var q: int = child.get_meta("quantity")
			var mf_val: Variant = JSON.parse_string(mf)
			if k == kind and _variant_eq(mf_val, filter_val):
				q = new_qty
			wants.append(_make_want_entry(k, mf_val, q))
	wants_changed.emit(JSON.stringify(wants))


static func _make_want_entry(k: String, mf: Variant, q: int) -> Dictionary:
	return {"kind": k, "material_filter": mf, "quantity": q}


static func _variant_eq(a: Variant, b: Variant) -> bool:
	return JSON.stringify(a) == JSON.stringify(b)
