## Toast-style notification display for player-visible event messages.
##
## Manages a vertical stack of short-lived notification toasts in the
## bottom-right corner of the screen. Each toast appears, stays for a
## configurable duration, then fades out and is removed. Multiple toasts
## stack upward (newest at bottom). The display sits on the base CanvasLayer
## (layer 1) so toasts render below overlay panels, info panels, and tooltips.
##
## Usage: main.gd polls the bridge for new notifications each frame via
## get_notifications_after() and calls push_notification() for each new one.
## Notifications are created sim-side (persisted in SimDb) and go through
## the full command pipeline, making them multiplayer-aware.
##
## See also: notification_bell.gd for the bell icon button,
## notification_history_panel.gd for the scrollable notification log,
## action_toolbar.gd for the debug button that triggers test notifications,
## main.gd for wiring and polling, sim_bridge.rs for the bridge methods
## (get_notifications_after, send_debug_notification).

extends VBoxContainer

## How long a toast stays fully visible before fading (seconds).
const DISPLAY_DURATION := 4.0

## How long the fade-out takes (seconds).
const FADE_DURATION := 1.0

## Maximum toasts visible at once. Oldest are removed if exceeded.
const MAX_TOASTS := 8


func _ready() -> void:
	# Anchor to bottom-right corner.
	set_anchors_preset(Control.PRESET_BOTTOM_RIGHT)
	anchor_left = 0.65
	anchor_top = 0.5
	anchor_right = 1.0
	anchor_bottom = 1.0
	offset_left = -10
	offset_top = 0
	offset_right = -10
	offset_bottom = -10
	grow_horizontal = Control.GROW_DIRECTION_BEGIN
	grow_vertical = Control.GROW_DIRECTION_BEGIN
	alignment = BoxContainer.ALIGNMENT_END
	add_theme_constant_override("separation", 4)
	mouse_filter = Control.MOUSE_FILTER_IGNORE


## Push a new notification toast onto the display.
func push_notification(text: String) -> void:
	# Enforce max toast count — remove oldest (topmost).
	while get_child_count() >= MAX_TOASTS:
		var oldest := get_child(0)
		remove_child(oldest)
		oldest.queue_free()

	var toast := _create_toast(text)
	add_child(toast)

	# Schedule fade-out after display duration. Tween is bound to the toast
	# so it's automatically killed if the toast is evicted early.
	var tween := toast.create_tween()
	tween.tween_interval(DISPLAY_DURATION)
	tween.tween_property(toast, "modulate:a", 0.0, FADE_DURATION)
	tween.tween_callback(toast.queue_free)


func _create_toast(text: String) -> PanelContainer:
	var panel := PanelContainer.new()
	panel.mouse_filter = Control.MOUSE_FILTER_IGNORE

	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.1, 0.1, 0.1, 0.85)
	style.set_corner_radius_all(4)
	style.set_content_margin_all(8)
	panel.add_theme_stylebox_override("panel", style)

	var label := Label.new()
	label.text = text
	label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	panel.add_child(label)

	return panel
