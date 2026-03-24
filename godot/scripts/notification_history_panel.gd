## Full-height right-side panel showing a scrollable log of all past
## notifications.
##
## Opened by clicking the notification bell button; closing it (via the X
## button or pressing Escape) hides the panel. When the panel is opened, it
## emits `panel_opened` so main.gd can mark all notifications as read.
##
## Each notification entry shows the tick number and message text in a style
## matching the toast display. Entries are listed newest-first. New entries
## are appended live while the panel is visible.
##
## Uses the _match_viewport_height() pattern required for ScrollContainer
## inside a PanelContainer (see docs/godot_scroll_sizing.md).
##
## See also: notification_bell.gd (bell button), notification_display.gd
## (toast display), main.gd (wiring and polling).

extends PanelContainer

signal panel_opened
signal panel_closed

## Maximum number of entries to keep. Oldest are evicted when exceeded.
const MAX_ENTRIES := 500

## Internal list of notification dicts {id, tick, message} in insertion order.
## Newest entries are appended at the end; the scroll container shows them in
## reverse (newest at top).
var _entries: Array = []

var _scroll: ScrollContainer
var _entry_container: VBoxContainer
var _title_label: Label
var _close_btn: Button
var _empty_label: Label


func _ready() -> void:
	# Anchor to the right edge, full height. Use explicit offsets to
	# guarantee right-flush placement (PRESET_RIGHT_WIDE alone doesn't
	# set grow_horizontal, so the panel can end up off-screen).
	anchor_left = 1.0
	anchor_top = 0.0
	anchor_right = 1.0
	anchor_bottom = 1.0
	offset_left = -340
	offset_right = 0
	offset_top = 0
	offset_bottom = 0
	# PanelContainer shrinks to content minimum, and ScrollContainer has
	# zero minimum height — force full viewport height so the scroll area
	# is visible (see docs/godot_scroll_sizing.md).
	_match_viewport_height()
	get_viewport().size_changed.connect(_match_viewport_height)

	# Semi-transparent dark background matching existing panels.
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.08, 0.08, 0.08, 0.92)
	style.corner_radius_top_left = 4
	style.corner_radius_bottom_left = 4
	style.content_margin_left = 12
	style.content_margin_right = 12
	style.content_margin_top = 12
	style.content_margin_bottom = 12
	add_theme_stylebox_override("panel", style)

	var root_vbox := VBoxContainer.new()
	root_vbox.add_theme_constant_override("separation", 8)
	add_child(root_vbox)

	# -- Header row: title + close button --
	var header := HBoxContainer.new()
	root_vbox.add_child(header)

	_title_label = Label.new()
	_title_label.text = "Notifications"
	_title_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_title_label.add_theme_font_size_override("font_size", 16)
	header.add_child(_title_label)

	_close_btn = Button.new()
	_close_btn.text = "X"
	_close_btn.custom_minimum_size = Vector2(28, 28)
	_close_btn.pressed.connect(hide_panel)
	header.add_child(_close_btn)

	# -- Separator --
	var sep := HSeparator.new()
	root_vbox.add_child(sep)

	# -- Scrollable notification list --
	_scroll = ScrollContainer.new()
	_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	root_vbox.add_child(_scroll)

	_entry_container = VBoxContainer.new()
	_entry_container.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_entry_container.add_theme_constant_override("separation", 4)
	_scroll.add_child(_entry_container)

	# Placeholder shown when there are no notifications.
	_empty_label = Label.new()
	_empty_label.text = "No notifications yet."
	_empty_label.add_theme_color_override("font_color", Color(0.5, 0.5, 0.5))
	_entry_container.add_child(_empty_label)

	visible = false


func _unhandled_key_input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		hide_panel()
		get_viewport().set_input_as_handled()


## Add a batch of notification entries (from polling). Each entry is a dict
## with keys: id (int), tick (int), message (String).
func add_entries(entries: Array) -> void:
	for entry in entries:
		_entries.append(entry)
		_add_entry_ui(entry)
	# Evict oldest entries if we've exceeded the cap.
	while _entries.size() > MAX_ENTRIES:
		_entries.pop_front()
		# Oldest UI entry is at the end (newest-first order), just before
		# the _empty_label which is always the last child.
		var last_entry_idx := _entry_container.get_child_count() - 2
		if last_entry_idx >= 0:
			var oldest := _entry_container.get_child(last_entry_idx)
			_entry_container.remove_child(oldest)
			oldest.queue_free()
	if _entries.size() > 0 and _empty_label.visible:
		_empty_label.visible = false


## Show the panel and emit panel_opened so main.gd can mark notifications read.
func show_panel() -> void:
	visible = true
	panel_opened.emit()
	# Scroll to top (newest entries are at the top).
	if _scroll:
		_scroll.scroll_vertical = 0


func hide_panel() -> void:
	visible = false
	panel_closed.emit()


func toggle() -> void:
	if visible:
		hide_panel()
	else:
		show_panel()


func _add_entry_ui(entry: Dictionary) -> void:
	var panel := PanelContainer.new()
	var entry_style := StyleBoxFlat.new()
	entry_style.bg_color = Color(0.14, 0.14, 0.14, 0.85)
	entry_style.set_corner_radius_all(3)
	entry_style.content_margin_left = 8
	entry_style.content_margin_right = 8
	entry_style.content_margin_top = 6
	entry_style.content_margin_bottom = 6
	panel.add_theme_stylebox_override("panel", entry_style)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 2)
	panel.add_child(vbox)

	# Tick label (small, dimmed).
	var tick_val: int = entry.get("tick", 0)
	var tick_label := Label.new()
	tick_label.text = "Tick %d" % tick_val
	tick_label.add_theme_color_override("font_color", Color(0.5, 0.5, 0.5))
	tick_label.add_theme_font_size_override("font_size", 11)
	vbox.add_child(tick_label)

	# Message label.
	var msg: String = entry.get("message", "")
	var msg_label := Label.new()
	msg_label.text = msg
	msg_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	vbox.add_child(msg_label)

	# Insert at top (newest first). The _empty_label drifts to the end
	# as entries are inserted at index 0, which is fine since it's hidden
	# once any entries exist.
	_entry_container.add_child(panel)
	_entry_container.move_child(panel, 0)


func _match_viewport_height() -> void:
	custom_minimum_size.y = get_viewport().get_visible_rect().size.y
