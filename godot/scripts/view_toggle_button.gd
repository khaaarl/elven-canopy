## Custom-drawn toggle button for the view toolbar.
##
## A square Control that draws its own background and delegates icon rendering
## to an external callable set via set_draw_callback(). The callable receives
## (center: Vector2, size: float, is_active: bool) and should use the owning
## Control's draw_* methods (passed implicitly since it runs inside _draw).
##
## Toggle state is tracked by the `active` property. Clicking the button
## flips the state and emits `toggled(is_active)`. Hover feedback is provided
## via a lighter background tint.
##
## Used exclusively by view_toolbar.gd — not meant to be instantiated on its
## own in a scene tree.

extends Control

signal toggled(is_active: bool)

const BUTTON_SIZE: int = 36

## Whether the toggle is currently active (pressed-in).
var active: bool = false

## Tooltip string set by the parent toolbar.
var tooltip_text_value: String = ""

## Callable invoked during _draw to render the icon.
## Signature: (control: Control, center: Vector2, size: float, is_active: bool) -> void
var _draw_callback: Callable

var _hovered: bool = false


func _ready() -> void:
	custom_minimum_size = Vector2(BUTTON_SIZE, BUTTON_SIZE)
	mouse_filter = Control.MOUSE_FILTER_STOP
	mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	tooltip_text = tooltip_text_value


func _get_minimum_size() -> Vector2:
	return Vector2(BUTTON_SIZE, BUTTON_SIZE)


func set_draw_callback(callback: Callable) -> void:
	_draw_callback = callback
	queue_redraw()


func _draw() -> void:
	var rect := Rect2(Vector2.ZERO, Vector2(BUTTON_SIZE, BUTTON_SIZE))
	var bg_color: Color
	if active:
		bg_color = Color(0.15, 0.22, 0.18, 0.9)
	else:
		bg_color = Color(0.12, 0.12, 0.14, 0.7)
	if _hovered:
		bg_color = bg_color.lightened(0.15)
	draw_rect(rect, bg_color, true, -1.0, false)
	# Rounded-corner border for subtle shape.
	var border_color := Color(0.3, 0.35, 0.32, 0.5) if active else Color(0.25, 0.25, 0.28, 0.4)
	if _hovered:
		border_color = border_color.lightened(0.1)
	draw_rect(rect, border_color, false, 1.0, false)

	if _draw_callback.is_valid():
		var center := Vector2(BUTTON_SIZE * 0.5, BUTTON_SIZE * 0.5)
		_draw_callback.call(self, center, float(BUTTON_SIZE), active)


func _gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_LEFT and mb.pressed:
			active = not active
			toggled.emit(active)
			queue_redraw()
			accept_event()


func _notification(what: int) -> void:
	if what == NOTIFICATION_MOUSE_ENTER:
		_hovered = true
		queue_redraw()
	elif what == NOTIFICATION_MOUSE_EXIT:
		_hovered = false
		queue_redraw()
