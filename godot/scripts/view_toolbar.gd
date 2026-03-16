## Right-edge toolbar for view mode toggles.
##
## A semi-transparent vertical strip anchored to the right side of the screen,
## vertically centered. Each toggle is a ViewToggleButton (view_toggle_button.gd)
## with a procedurally drawn icon. New toggles are added via add_toggle(), which
## accepts a tooltip string and a draw callback for the icon.
##
## Created programmatically by the main scene — no .tscn required.
##
## See also: view_toggle_button.gd for the individual toggle control,
## action_toolbar.gd for the top toolbar.

extends MarginContainer

const ViewToggleButton := preload("res://scripts/view_toggle_button.gd")

const PADDING: int = 4

var _vbox: VBoxContainer
var _buttons: Array = []


func _ready() -> void:
	# Anchor to right edge, vertically centered.
	anchor_left = 1.0
	anchor_right = 1.0
	anchor_top = 0.5
	anchor_bottom = 0.5
	grow_horizontal = Control.GROW_DIRECTION_BEGIN
	grow_vertical = Control.GROW_DIRECTION_BOTH

	# Margin from the right edge.
	add_theme_constant_override("margin_left", PADDING)
	add_theme_constant_override("margin_right", PADDING)
	add_theme_constant_override("margin_top", PADDING)
	add_theme_constant_override("margin_bottom", PADDING)

	# Semi-transparent dark background with rounded corners.
	var bg := StyleBoxFlat.new()
	bg.bg_color = Color(0.08, 0.08, 0.1, 0.75)
	bg.corner_radius_top_left = 6
	bg.corner_radius_top_right = 6
	bg.corner_radius_bottom_left = 6
	bg.corner_radius_bottom_right = 6
	bg.content_margin_left = PADDING
	bg.content_margin_right = PADDING
	bg.content_margin_top = PADDING
	bg.content_margin_bottom = PADDING
	add_theme_stylebox_override("panel", bg)

	_vbox = VBoxContainer.new()
	_vbox.add_theme_constant_override("separation", 4)
	add_child(_vbox)

	# Small offset from the screen edge.
	offset_right = -6.0
	offset_left = -6.0


func add_toggle(tooltip_label: String, draw_callback: Callable) -> Control:
	var btn := ViewToggleButton.new()
	btn.tooltip_text_value = tooltip_label
	btn.set_draw_callback(draw_callback)
	_vbox.add_child(btn)
	_buttons.append(btn)
	return btn
