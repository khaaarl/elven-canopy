## Reusable wants list editor (item kind + material filter + quantity).
##
## Displays a scrollable list of wants with add/remove controls and a
## two-step picker (item kind → material filter). Used by both building
## logistics (structure_info_panel.gd) and military equipment
## (military_panel.gd).
##
## Emits `wants_changed(wants_json: String)` whenever the list changes.
## The parent is responsible for routing this signal to the bridge.
##
## See also: structure_info_panel.gd (logistics usage),
## military_panel.gd (equipment usage), sim_bridge.rs (bridge API).

extends VBoxContainer

signal wants_changed(wants_json: String)

## Default quantity added when the user picks a new item kind. Callers can
## override this (e.g. 10 for logistics, 1 for equipment).
var default_add_quantity: int = 10

var _item_kinds: Array = []
var _material_options: Dictionary = {}

var _wants_vbox: VBoxContainer
var _add_button: Button
var _item_picker: VBoxContainer
var _material_picker: VBoxContainer
var _pending_item_kind: String = ""


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

	# Add item button.
	_add_button = Button.new()
	_add_button.text = "Add Item..."
	_add_button.pressed.connect(_on_add_pressed)
	add_child(_add_button)

	# Pickers (hidden by default).
	_item_picker = VBoxContainer.new()
	_item_picker.visible = false
	add_child(_item_picker)

	_material_picker = VBoxContainer.new()
	_material_picker.visible = false
	add_child(_material_picker)


## Set the available item kinds and material options (from bridge).
func set_picker_data(item_kinds: Array, material_options: Dictionary) -> void:
	_item_kinds = item_kinds
	_material_options = material_options


## Update the displayed wants list from an array of want dictionaries.
## Each dict: { kind: String, material_filter: String, target_quantity: int, label: String }
func update_wants(wants: Array) -> void:
	for child in _wants_vbox.get_children():
		child.queue_free()
	for want in wants:
		var display_label: String = want.get("label", want.get("kind", "?"))
		var qty: int = want.get("target_quantity", 0)
		var kind: String = want.get("kind", "?")
		var mat_filter: String = want.get("material_filter", '"Any"')
		var row := HBoxContainer.new()
		var label := Label.new()
		label.text = "%s: %d" % [display_label, qty]
		label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		row.set_meta("kind", kind)
		row.set_meta("material_filter", mat_filter)
		row.set_meta("quantity", qty)
		row.add_child(label)
		var remove_btn := Button.new()
		remove_btn.text = "X"
		remove_btn.pressed.connect(_on_want_removed.bind(kind, mat_filter))
		row.add_child(remove_btn)
		_wants_vbox.add_child(row)


# --- Add item flow ---


func _on_add_pressed() -> void:
	_material_picker.visible = false
	var was_visible := _item_picker.visible
	_item_picker.visible = not was_visible
	if not was_visible:
		_populate_item_kind_picker()


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
	_emit_wants_with_added(_pending_item_kind, filter_str, default_add_quantity)


func _on_want_removed(kind: String, filter_str: String) -> void:
	_emit_wants_without(kind, filter_str)


# --- JSON emission ---


func _emit_wants_with_added(kind: String, filter_str: String, qty: int) -> void:
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


static func _make_want_entry(k: String, mf: Variant, q: int) -> Dictionary:
	return {"kind": k, "material_filter": mf, "quantity": q}


static func _variant_eq(a: Variant, b: Variant) -> bool:
	return JSON.stringify(a) == JSON.stringify(b)
