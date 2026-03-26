## Item detail panel displayed to the left of inventory-bearing info panels.
##
## Shows detailed information about a single item stack: display name, kind,
## material, quality, durability (HP bar), equipped slot, owner (clickable
## button to select and zoom), dye color, and quantity.
##
## Opened when the user clicks an item row in any inventory list (creature,
## structure, or ground pile info panels). Updated every frame by main.gd
## while visible — the underlying item can change (HP loss, equip/unequip)
## between frames.
##
## Added directly to the info_panel_layer CanvasLayer by main.gd, which also
## sets the anchor/offset positioning (left of the right-edge info panels,
## same scheme as the crafting/logistics detail panels).
##
## See also: creature_info_panel.gd, structure_info_panel.gd,
## ground_pile_info_panel.gd for the inventory click sources,
## main.gd which wires everything together,
## sim_bridge.rs get_item_detail() for the data source.

extends PanelContainer

signal owner_clicked(creature_id: String)
signal panel_closed

var _item_stack_id: int = -1

var _name_label: Label
var _kind_label: Label
var _material_row: HBoxContainer
var _material_label: Label
var _quality_row: HBoxContainer
var _quality_label: Label
var _durability_row: VBoxContainer
var _hp_bar: ProgressBar
var _hp_label: Label
var _equipped_row: HBoxContainer
var _equipped_label: Label
var _owner_row: HBoxContainer
var _owner_button: Button
var _dye_row: HBoxContainer
var _dye_label: Label
var _quantity_row: HBoxContainer
var _quantity_label: Label


func _ready() -> void:
	visible = false
	custom_minimum_size.x = 280

	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	margin.add_theme_constant_override("margin_right", 12)
	margin.add_theme_constant_override("margin_top", 12)
	margin.add_theme_constant_override("margin_bottom", 12)
	add_child(margin)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 6)
	margin.add_child(vbox)

	# Header with title and close button.
	var header := HBoxContainer.new()
	vbox.add_child(header)

	_name_label = Label.new()
	_name_label.add_theme_font_size_override("font_size", 18)
	_name_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_name_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	header.add_child(_name_label)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close_pressed)
	header.add_child(close_btn)

	vbox.add_child(HSeparator.new())

	# Kind.
	_kind_label = Label.new()
	_kind_label.add_theme_color_override("font_color", Color(0.7, 0.7, 0.7))
	vbox.add_child(_kind_label)

	# Material (hidden when empty).
	_material_row = _build_row(vbox, "Material:")
	_material_label = _material_row.get_child(1)

	# Quality (hidden when no label).
	_quality_row = _build_row(vbox, "Quality:")
	_quality_label = _quality_row.get_child(1)

	# Quantity.
	_quantity_row = _build_row(vbox, "Quantity:")
	_quantity_label = _quantity_row.get_child(1)

	# Durability.
	_durability_row = VBoxContainer.new()
	_durability_row.add_theme_constant_override("separation", 2)
	vbox.add_child(_durability_row)

	_hp_label = Label.new()
	_durability_row.add_child(_hp_label)

	_hp_bar = ProgressBar.new()
	_hp_bar.custom_minimum_size.y = 16
	_hp_bar.show_percentage = false
	_durability_row.add_child(_hp_bar)

	# Equipped slot (hidden when not equipped).
	_equipped_row = _build_row(vbox, "Equipped:")
	_equipped_label = _equipped_row.get_child(1)

	# Owner (clickable button, hidden when no owner).
	_owner_row = HBoxContainer.new()
	_owner_row.add_theme_constant_override("separation", 6)
	vbox.add_child(_owner_row)

	var owner_prefix := Label.new()
	owner_prefix.text = "Owner:"
	_owner_row.add_child(owner_prefix)

	_owner_button = Button.new()
	_owner_button.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_owner_button.alignment = HORIZONTAL_ALIGNMENT_LEFT
	_owner_button.pressed.connect(_on_owner_pressed)
	_owner_row.add_child(_owner_button)

	# Dye color (hidden when no dye).
	_dye_row = _build_row(vbox, "Dye:")
	_dye_label = _dye_row.get_child(1)


## Build a simple "Label: Value" row and add it to the parent.
## Returns the HBoxContainer; the value Label is child(1).
func _build_row(parent: VBoxContainer, prefix_text: String) -> HBoxContainer:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 6)
	parent.add_child(row)

	var prefix := Label.new()
	prefix.text = prefix_text
	row.add_child(prefix)

	var value := Label.new()
	value.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(value)

	return row


## Show the panel for a given item stack.
func show_item(item_stack_id: int, info: Dictionary) -> void:
	_item_stack_id = item_stack_id
	_update(info)
	visible = true


## Update the panel with fresh data (called every frame by main.gd).
func update_item(info: Dictionary) -> void:
	if info.is_empty():
		# Item no longer exists (consumed, broken, etc.) — close panel.
		hide_panel()
		return
	_update(info)


func hide_panel() -> void:
	visible = false
	_item_stack_id = -1


func get_item_stack_id() -> int:
	return _item_stack_id


func _update(info: Dictionary) -> void:
	_name_label.text = info.get("display_name", "?")
	_kind_label.text = info.get("kind", "?")

	# Material.
	var mat: String = info.get("material", "")
	_material_row.visible = not mat.is_empty()
	_material_label.text = mat

	# Quality.
	var qlabel: String = info.get("quality_label", "")
	var qval: int = info.get("quality", 0)
	_quality_row.visible = not qlabel.is_empty()
	if not qlabel.is_empty():
		_quality_label.text = "%s (%d)" % [qlabel, qval]

	# Quantity.
	var qty: int = info.get("quantity", 1)
	_quantity_row.visible = qty > 1
	_quantity_label.text = str(qty)

	# Durability.
	var max_hp: int = info.get("max_hp", 0)
	var current_hp: int = info.get("current_hp", 0)
	_durability_row.visible = max_hp > 0
	if max_hp > 0:
		_hp_bar.max_value = max_hp
		_hp_bar.value = current_hp
		var condition: String = info.get("condition", "")
		if condition.is_empty():
			_hp_label.text = "Durability: %d / %d" % [current_hp, max_hp]
		else:
			_hp_label.text = "Durability: %d / %d %s" % [current_hp, max_hp, condition]
		# Color the bar based on condition.
		if condition == "(damaged)":
			_hp_bar.modulate = Color(1.0, 0.3, 0.3)
		elif condition == "(worn)":
			_hp_bar.modulate = Color(1.0, 0.8, 0.3)
		else:
			_hp_bar.modulate = Color(0.3, 1.0, 0.3)

	# Equipped slot.
	var slot: String = info.get("equipped_slot", "")
	_equipped_row.visible = not slot.is_empty()
	_equipped_label.text = slot

	# Owner.
	var owner_name: String = info.get("owner_name", "")
	_owner_row.visible = not owner_name.is_empty()
	if not owner_name.is_empty():
		_owner_button.text = owner_name
		var oid: String = info.get("owner_id", "")
		_owner_button.set_meta("owner_id", oid)

	# Dye color.
	var dye: String = info.get("dye_color", "")
	_dye_row.visible = not dye.is_empty()
	_dye_label.text = dye


func _on_close_pressed() -> void:
	hide_panel()
	panel_closed.emit()


func _on_owner_pressed() -> void:
	var oid: String = _owner_button.get_meta("owner_id", "")
	if not oid.is_empty():
		owner_clicked.emit(oid)
