## Ground pile info panel displayed on the right side of the screen.
##
## Shows information about the currently selected ground pile: title, position,
## and inventory contents. Built programmatically as a PanelContainer following
## the same pattern as structure_info_panel.gd. Anchored to the right edge,
## 320px minimum width, full height.
##
## Updated every frame by main.gd while visible (pile contents can change as
## creatures pick up or drop items). If the pile is removed (empty dict from
## bridge.get_ground_pile_info()), main.gd deselects and hides the panel.
##
## See also: selection_controller.gd which triggers show/hide via
## pile_selected/pile_deselected signals, main.gd which wires everything
## together, sim_bridge.rs for get_ground_pile_info().

extends PanelContainer

signal panel_closed

var _position_label: Label
var _inventory_label: Label


func _ready() -> void:
	# Anchor to the right edge, full height.
	set_anchors_preset(PRESET_RIGHT_WIDE)
	custom_minimum_size.x = 320

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

	var title := Label.new()
	title.text = "Ground Pile"
	title.add_theme_font_size_override("font_size", 20)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close_pressed)
	header.add_child(close_btn)

	# Separator.
	vbox.add_child(HSeparator.new())

	# Position.
	_position_label = Label.new()
	vbox.add_child(_position_label)

	# Inventory section.
	vbox.add_child(HSeparator.new())

	var inv_title := Label.new()
	inv_title.text = "Inventory"
	inv_title.add_theme_font_size_override("font_size", 16)
	vbox.add_child(inv_title)

	_inventory_label = Label.new()
	_inventory_label.text = "(empty)"
	vbox.add_child(_inventory_label)

	visible = false


func show_pile(info: Dictionary) -> void:
	_update_info(info)
	visible = true


func update_info(info: Dictionary) -> void:
	_update_info(info)


func hide_panel() -> void:
	visible = false


func _update_info(info: Dictionary) -> void:
	var px: int = info.get("x", 0)
	var py: int = info.get("y", 0)
	var pz: int = info.get("z", 0)
	_position_label.text = "Position: (%d, %d, %d)" % [px, py, pz]

	var inv: Array = info.get("inventory", [])
	if inv.is_empty():
		_inventory_label.text = "(empty)"
		return
	var lines: PackedStringArray = []
	for entry in inv:
		var kind: String = entry.get("kind", "?")
		var qty: int = entry.get("quantity", 0)
		lines.append("%s: %d" % [kind, qty])
	_inventory_label.text = "\n".join(lines)


func _on_close_pressed() -> void:
	hide_panel()
	panel_closed.emit()
