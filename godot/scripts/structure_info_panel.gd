## Structure info panel displayed on the right side of the screen.
##
## Shows information about the currently selected structure. Built
## programmatically as a PanelContainer with labels and a Zoom button.
## Sits on the CanvasLayer alongside the creature info panel.
##
## The panel is ~25% screen width, full height, anchored to the right edge.
## Shows type, ID, dimensions, and position. Updated every frame by main.gd.
##
## See also: selection_controller.gd which triggers show/hide,
## creature_info_panel.gd for the creature equivalent,
## main.gd which wires everything together.

extends PanelContainer

signal zoom_requested(x: float, y: float, z: float)
signal panel_closed

var _type_label: Label
var _id_label: Label
var _dimensions_label: Label
var _position_label: Label
var _zoom_button: Button
var _anchor_x: float = 0.0
var _anchor_y: float = 0.0
var _anchor_z: float = 0.0


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
	title.text = "Structure Info"
	title.add_theme_font_size_override("font_size", 20)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close_pressed)
	header.add_child(close_btn)

	# Separator.
	vbox.add_child(HSeparator.new())

	# Type.
	_type_label = Label.new()
	vbox.add_child(_type_label)

	# ID.
	_id_label = Label.new()
	vbox.add_child(_id_label)

	# Dimensions.
	_dimensions_label = Label.new()
	vbox.add_child(_dimensions_label)

	# Position.
	_position_label = Label.new()
	vbox.add_child(_position_label)

	# Spacer to push the zoom button toward the bottom-ish area.
	var spacer := Control.new()
	spacer.size_flags_vertical = Control.SIZE_EXPAND_FILL
	vbox.add_child(spacer)

	# Zoom button.
	_zoom_button = Button.new()
	_zoom_button.text = "Zoom"
	_zoom_button.pressed.connect(_on_zoom_pressed)
	vbox.add_child(_zoom_button)

	visible = false


func show_structure(info: Dictionary) -> void:
	_update_info(info)
	visible = true


func update_info(info: Dictionary) -> void:
	_update_info(info)


func hide_panel() -> void:
	visible = false


func _update_info(info: Dictionary) -> void:
	var build_type: String = info.get("build_type", "?")
	var sid: int = info.get("id", 0)
	var w: int = info.get("width", 0)
	var d: int = info.get("depth", 0)
	var h: int = info.get("height", 0)
	_anchor_x = info.get("anchor_x", 0)
	_anchor_y = info.get("anchor_y", 0)
	_anchor_z = info.get("anchor_z", 0)

	_type_label.text = "Type: %s" % build_type
	_id_label.text = "ID: #%d" % sid
	_dimensions_label.text = "Dimensions: %d x %d x %d" % [w, d, h]
	_position_label.text = (
		"Position: (%d, %d, %d)" % [int(_anchor_x), int(_anchor_y), int(_anchor_z)]
	)


func _on_zoom_pressed() -> void:
	zoom_requested.emit(_anchor_x, _anchor_y, _anchor_z)


func _on_close_pressed() -> void:
	hide_panel()
	panel_closed.emit()
