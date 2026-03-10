## Military groups management panel (right-side overlay).
##
## Two-page panel opened via the Military [M] button:
## - **Summary page:** Lists all military groups with member counts. Click a row
##   to navigate to the group detail page. "New Group" button at the bottom.
## - **Detail page:** Shows group members, hostile response toggle (Fight/Flee),
##   rename button, delete button (non-civilian only), and per-member reassign.
##
## Toggle behavior: [M] opens/closes the panel. ESC from detail navigates back
## to summary; ESC from summary closes. Opening this panel closes the creature
## info panel and vice versa (they share the right-side screen space).
##
## Data flow: summary calls bridge.get_military_groups() on open and every 0.5s.
## Detail calls bridge.get_military_group_members(group_id) with the same cadence.
##
## See also: creature_info_panel.gd (shares screen space), main.gd (wiring),
## action_toolbar.gd (Military button), sim_bridge.rs (Rust API).

extends PanelContainer

signal panel_closed
signal creature_selected(creature_id: String)

## Which page is showing: "summary" or "detail".
var _page: String = "summary"
var _detail_group_id: int = -1
var _refresh_timer: float = 0.0

var _bridge: SimBridge = null

# Summary page widgets.
var _summary_vbox: VBoxContainer
var _summary_scroll: ScrollContainer
var _summary_list: VBoxContainer
var _new_group_btn: Button

# Detail page widgets.
var _detail_vbox: VBoxContainer
var _detail_header_row: HBoxContainer
var _detail_name_label: Label
var _detail_name_edit: LineEdit
var _detail_rename_btn: Button
var _detail_back_btn: Button
var _is_renaming: bool = false
var _detail_scroll: ScrollContainer
var _detail_member_list: VBoxContainer
var _fight_btn: Button
var _flee_btn: Button
var _delete_btn: Button


func _ready() -> void:
	set_anchors_preset(PRESET_RIGHT_WIDE)
	custom_minimum_size.x = 320
	# PanelContainer shrinks to content minimum, and ScrollContainer has zero
	# minimum height — force full viewport height so the scroll area is visible.
	_match_viewport_height()
	get_viewport().size_changed.connect(_match_viewport_height)

	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	margin.add_theme_constant_override("margin_right", 12)
	margin.add_theme_constant_override("margin_top", 12)
	margin.add_theme_constant_override("margin_bottom", 12)
	add_child(margin)

	var root_vbox := VBoxContainer.new()
	root_vbox.add_theme_constant_override("separation", 8)
	margin.add_child(root_vbox)

	# --- Summary page ---
	_summary_vbox = VBoxContainer.new()
	_summary_vbox.add_theme_constant_override("separation", 8)
	_summary_vbox.size_flags_vertical = Control.SIZE_EXPAND_FILL
	root_vbox.add_child(_summary_vbox)

	var summary_header := HBoxContainer.new()
	_summary_vbox.add_child(summary_header)

	var summary_title := Label.new()
	summary_title.text = "Military Groups"
	summary_title.add_theme_font_size_override("font_size", 20)
	summary_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	summary_header.add_child(summary_title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close)
	summary_header.add_child(close_btn)

	_summary_vbox.add_child(HSeparator.new())

	_summary_scroll = ScrollContainer.new()
	_summary_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_summary_vbox.add_child(_summary_scroll)

	_summary_list = VBoxContainer.new()
	_summary_list.add_theme_constant_override("separation", 4)
	_summary_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_summary_scroll.add_child(_summary_list)

	_new_group_btn = Button.new()
	_new_group_btn.text = "+ New Group"
	_new_group_btn.pressed.connect(_on_new_group)
	_summary_vbox.add_child(_new_group_btn)

	# --- Detail page ---
	_detail_vbox = VBoxContainer.new()
	_detail_vbox.add_theme_constant_override("separation", 8)
	_detail_vbox.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_detail_vbox.visible = false
	root_vbox.add_child(_detail_vbox)

	# Detail header: back button + name label/edit + rename button + close button.
	_detail_header_row = HBoxContainer.new()
	_detail_header_row.add_theme_constant_override("separation", 4)
	_detail_vbox.add_child(_detail_header_row)

	_detail_back_btn = Button.new()
	_detail_back_btn.text = "<"
	_detail_back_btn.pressed.connect(_show_summary)
	_detail_header_row.add_child(_detail_back_btn)

	_detail_name_label = Label.new()
	_detail_name_label.add_theme_font_size_override("font_size", 18)
	_detail_name_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_detail_header_row.add_child(_detail_name_label)

	_detail_name_edit = LineEdit.new()
	_detail_name_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_detail_name_edit.visible = false
	_detail_name_edit.text_submitted.connect(_on_rename_confirmed)
	_detail_header_row.add_child(_detail_name_edit)

	_detail_rename_btn = Button.new()
	_detail_rename_btn.text = "Rename"
	_detail_rename_btn.pressed.connect(_on_rename_pressed)
	_detail_header_row.add_child(_detail_rename_btn)

	var detail_close := Button.new()
	detail_close.text = "X"
	detail_close.pressed.connect(_on_close)
	_detail_header_row.add_child(detail_close)

	_detail_vbox.add_child(HSeparator.new())

	# Hostile response toggles.
	var response_row := HBoxContainer.new()
	response_row.add_theme_constant_override("separation", 6)
	_detail_vbox.add_child(response_row)

	var response_label := Label.new()
	response_label.text = "Response:"
	response_row.add_child(response_label)

	_fight_btn = Button.new()
	_fight_btn.text = "Fight"
	_fight_btn.toggle_mode = true
	_fight_btn.pressed.connect(func(): _set_response("Fight"))
	response_row.add_child(_fight_btn)

	_flee_btn = Button.new()
	_flee_btn.text = "Flee"
	_flee_btn.toggle_mode = true
	_flee_btn.pressed.connect(func(): _set_response("Flee"))
	response_row.add_child(_flee_btn)

	# Delete button.
	_delete_btn = Button.new()
	_delete_btn.text = "Delete Group"
	_delete_btn.pressed.connect(_on_delete_group)
	_detail_vbox.add_child(_delete_btn)

	_detail_vbox.add_child(HSeparator.new())

	# Members header.
	var members_title := Label.new()
	members_title.text = "Members"
	members_title.add_theme_font_size_override("font_size", 16)
	_detail_vbox.add_child(members_title)

	# Scrollable member list.
	_detail_scroll = ScrollContainer.new()
	_detail_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_detail_vbox.add_child(_detail_scroll)

	_detail_member_list = VBoxContainer.new()
	_detail_member_list.add_theme_constant_override("separation", 2)
	_detail_member_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_detail_scroll.add_child(_detail_member_list)

	visible = false


func setup(bridge: SimBridge) -> void:
	_bridge = bridge


func toggle() -> void:
	if visible:
		_close_panel()
	else:
		_open_panel()


func is_visible_panel() -> bool:
	return visible


func _open_panel() -> void:
	_page = "summary"
	_summary_vbox.visible = true
	_detail_vbox.visible = false
	visible = true
	_refresh_summary()


func _close_panel() -> void:
	visible = false
	_is_renaming = false
	panel_closed.emit()


func _show_summary() -> void:
	_page = "summary"
	_summary_vbox.visible = true
	_detail_vbox.visible = false
	_is_renaming = false
	_refresh_summary()


func _show_detail(group_id: int) -> void:
	_page = "detail"
	_detail_group_id = group_id
	_summary_vbox.visible = false
	_detail_vbox.visible = true
	_is_renaming = false
	_detail_name_label.visible = true
	_detail_name_edit.visible = false
	_detail_rename_btn.text = "Rename"
	_refresh_detail()


## Navigate directly to a group's detail page (from creature info panel).
func show_group_detail(group_id: int) -> void:
	visible = true
	_show_detail(group_id)


func _unhandled_input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey:
		var key := event as InputEventKey
		if key.pressed and key.keycode == KEY_ESCAPE:
			if handle_esc():
				get_viewport().set_input_as_handled()


func _process(_delta: float) -> void:
	if not visible:
		return
	_refresh_timer += _delta
	if _refresh_timer >= 0.5:
		_refresh_timer = 0.0
		if _page == "summary":
			_refresh_summary()
		elif _page == "detail":
			_refresh_detail()


func handle_esc() -> bool:
	if not visible:
		return false
	if _is_renaming:
		_cancel_rename()
		return true
	if _page == "detail":
		_show_summary()
		return true
	_close_panel()
	return true


# --- Summary page ---


func _refresh_summary() -> void:
	if not _bridge:
		return
	var groups: Array = _bridge.get_military_groups()

	# Reuse/add/remove rows.
	var child_count := _summary_list.get_child_count()
	while child_count > groups.size():
		child_count -= 1
		_summary_list.get_child(child_count).queue_free()

	for i in range(groups.size()):
		var g: Dictionary = groups[i]
		var row: HBoxContainer
		if i < _summary_list.get_child_count():
			row = _summary_list.get_child(i) as HBoxContainer
		else:
			row = _create_summary_row()
			_summary_list.add_child(row)
		_update_summary_row(row, g)


func _create_summary_row() -> HBoxContainer:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 6)

	var name_btn := Button.new()
	name_btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	name_btn.alignment = HORIZONTAL_ALIGNMENT_LEFT
	row.add_child(name_btn)

	var count_label := Label.new()
	count_label.custom_minimum_size.x = 40
	count_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	row.add_child(count_label)

	return row


func _update_summary_row(row: HBoxContainer, g: Dictionary) -> void:
	var name_btn: Button = row.get_child(0) as Button
	var count_label: Label = row.get_child(1) as Label

	var display_name: String = g.get("name", "")
	if g.get("is_civilian", false):
		display_name += " (default)"
	var response: String = g.get("hostile_response", "")
	name_btn.text = "%s [%s]" % [display_name, response]

	var group_id: int = g.get("id", -1)
	# Reconnect the button signal.
	for conn in name_btn.pressed.get_connections():
		name_btn.pressed.disconnect(conn["callable"])
	name_btn.pressed.connect(func(): _show_detail(group_id))

	count_label.text = str(g.get("member_count", 0))


func _on_new_group() -> void:
	if not _bridge:
		return
	# Count existing groups to generate a default name.
	var groups: Array = _bridge.get_military_groups()
	var name := "Group %d" % (groups.size() + 1)
	_bridge.create_military_group(name)
	# Refresh after a short delay to pick up the new group.
	_refresh_timer = 0.4


# --- Detail page ---


func _refresh_detail() -> void:
	if not _bridge or _detail_group_id < 0:
		return

	# Refresh group metadata.
	var groups: Array = _bridge.get_military_groups()
	var group_data: Dictionary = {}
	for g in groups:
		if g.get("id", -1) == _detail_group_id:
			group_data = g
			break
	if group_data.is_empty():
		# Group was deleted — go back to summary.
		_show_summary()
		return

	if not _is_renaming:
		_detail_name_label.text = group_data.get("name", "")
	var response: String = group_data.get("hostile_response", "Flee")
	_fight_btn.button_pressed = (response == "Fight")
	_flee_btn.button_pressed = (response == "Flee")

	# Hide delete for civilian group.
	_delete_btn.visible = not group_data.get("is_civilian", false)

	# Refresh member list.
	var members: Array = _bridge.get_military_group_members(_detail_group_id)
	var child_count := _detail_member_list.get_child_count()
	while child_count > members.size():
		child_count -= 1
		_detail_member_list.get_child(child_count).queue_free()

	for i in range(members.size()):
		var m: Dictionary = members[i]
		var row: HBoxContainer
		if i < _detail_member_list.get_child_count():
			row = _detail_member_list.get_child(i) as HBoxContainer
		else:
			row = _create_member_row()
			_detail_member_list.add_child(row)
		_update_member_row(row, m)


func _create_member_row() -> HBoxContainer:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 6)

	var name_label := Label.new()
	name_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(name_label)

	var reassign_btn := Button.new()
	reassign_btn.text = "Reassign"
	row.add_child(reassign_btn)

	return row


func _update_member_row(row: HBoxContainer, m: Dictionary) -> void:
	var name_label: Label = row.get_child(0) as Label
	var reassign_btn: Button = row.get_child(1) as Button

	var creature_name: String = m.get("name", "")
	if creature_name.is_empty():
		creature_name = "%s" % m.get("species", "Unknown")
	name_label.text = creature_name

	var creature_id: String = m.get("creature_id", "")
	# Reconnect reassign button.
	for conn in reassign_btn.pressed.get_connections():
		reassign_btn.pressed.disconnect(conn["callable"])
	reassign_btn.pressed.connect(func(): _open_reassign_overlay(creature_id, creature_name))


# --- Hostile response ---


func _set_response(response: String) -> void:
	if not _bridge or _detail_group_id < 0:
		return
	_bridge.set_group_hostile_response(_detail_group_id, response)
	_refresh_timer = 0.4


# --- Rename ---


func _on_rename_pressed() -> void:
	if _is_renaming:
		_cancel_rename()
	else:
		_is_renaming = true
		_detail_name_label.visible = false
		_detail_name_edit.visible = true
		_detail_name_edit.text = _detail_name_label.text
		_detail_name_edit.grab_focus()
		_detail_rename_btn.text = "Cancel"


func _on_rename_confirmed(new_name: String) -> void:
	if not _bridge or _detail_group_id < 0:
		return
	if not new_name.strip_edges().is_empty():
		_bridge.rename_military_group(_detail_group_id, new_name.strip_edges())
	_cancel_rename()
	_refresh_timer = 0.4


func _cancel_rename() -> void:
	_is_renaming = false
	_detail_name_label.visible = true
	_detail_name_edit.visible = false
	_detail_rename_btn.text = "Rename"


# --- Delete ---


func _on_delete_group() -> void:
	if not _bridge or _detail_group_id < 0:
		return
	_bridge.delete_military_group(_detail_group_id)
	_show_summary()


# --- Reassignment overlay ---


func _open_reassign_overlay(creature_id: String, creature_name: String) -> void:
	if not _bridge:
		return
	var groups: Array = _bridge.get_military_groups()

	# Create modal overlay.
	var overlay := ColorRect.new()
	overlay.color = Color(0, 0, 0, 0.5)
	overlay.set_anchors_preset(PRESET_FULL_RECT)
	overlay.mouse_filter = Control.MOUSE_FILTER_STOP

	var center := CenterContainer.new()
	center.set_anchors_preset(PRESET_FULL_RECT)
	overlay.add_child(center)

	var panel := PanelContainer.new()
	panel.custom_minimum_size = Vector2(280, 200)
	center.add_child(panel)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 6)
	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	margin.add_theme_constant_override("margin_right", 12)
	margin.add_theme_constant_override("margin_top", 12)
	margin.add_theme_constant_override("margin_bottom", 12)
	panel.add_child(margin)
	margin.add_child(vbox)

	var title := Label.new()
	title.text = "Reassign %s" % creature_name
	title.add_theme_font_size_override("font_size", 16)
	vbox.add_child(title)

	vbox.add_child(HSeparator.new())

	# Civilian button.
	var civ_btn := Button.new()
	civ_btn.text = "Civilians"
	civ_btn.pressed.connect(
		func():
			_bridge.reassign_to_civilian(creature_id)
			overlay.queue_free()
			_refresh_timer = 0.4
	)
	vbox.add_child(civ_btn)

	# One button per non-civilian group.
	for g in groups:
		if g.get("is_civilian", false):
			continue
		var btn := Button.new()
		btn.text = "%s [%s]" % [g.get("name", ""), g.get("hostile_response", "")]
		var gid: int = g.get("id", -1)
		btn.pressed.connect(
			func():
				_bridge.reassign_military_group(creature_id, gid)
				overlay.queue_free()
				_refresh_timer = 0.4
		)
		vbox.add_child(btn)

	vbox.add_child(HSeparator.new())

	var cancel_btn := Button.new()
	cancel_btn.text = "Cancel"
	cancel_btn.pressed.connect(func(): overlay.queue_free())
	vbox.add_child(cancel_btn)

	# Add overlay to the scene tree above this panel.
	get_tree().root.add_child(overlay)

	# Handle ESC to close overlay.
	overlay.gui_input.connect(
		func(event: InputEvent):
			if event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
				overlay.queue_free()
				get_viewport().set_input_as_handled()
	)


func _match_viewport_height() -> void:
	custom_minimum_size.y = get_viewport().get_visible_rect().size.y


func _on_close() -> void:
	_close_panel()
