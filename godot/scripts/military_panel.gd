## Military groups management panel (right-side overlay).
##
## Two-page panel opened via the Military [M] button:
## - **Summary page:** Lists all military groups with member counts. Click a row
##   to navigate to the group detail page. "New Group" button at the bottom.
## - **Detail page:** Shows group members, engagement style controls (weapon
##   preference, ammo exhaustion, initiative, disengage threshold), rename
##   button, delete button (non-civilian only), and per-member reassign.
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
var _initiative_btn: Button
var _weapon_btn: Button
var _ammo_btn: Button
var _disengage_label: Label
var _disengage_slider: HSlider
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

	# Engagement style controls.
	var style_vbox := VBoxContainer.new()
	style_vbox.add_theme_constant_override("separation", 4)
	_detail_vbox.add_child(style_vbox)

	# Initiative (cycle button: Aggressive / Defensive / Passive).
	var init_row := HBoxContainer.new()
	init_row.add_theme_constant_override("separation", 6)
	style_vbox.add_child(init_row)
	var init_label := Label.new()
	init_label.text = "Initiative:"
	init_label.custom_minimum_size.x = 100
	init_row.add_child(init_label)
	_initiative_btn = Button.new()
	_initiative_btn.text = "Aggressive"
	_initiative_btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_initiative_btn.pressed.connect(_cycle_initiative)
	init_row.add_child(_initiative_btn)

	# Weapon preference (cycle button: PreferRanged / PreferMelee).
	var weapon_row := HBoxContainer.new()
	weapon_row.add_theme_constant_override("separation", 6)
	style_vbox.add_child(weapon_row)
	var weapon_label := Label.new()
	weapon_label.text = "Weapon:"
	weapon_label.custom_minimum_size.x = 100
	weapon_row.add_child(weapon_label)
	_weapon_btn = Button.new()
	_weapon_btn.text = "Prefer Ranged"
	_weapon_btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_weapon_btn.pressed.connect(_cycle_weapon)
	weapon_row.add_child(_weapon_btn)

	# Ammo exhaustion (cycle button: SwitchToMelee / Flee).
	var ammo_row := HBoxContainer.new()
	ammo_row.add_theme_constant_override("separation", 6)
	style_vbox.add_child(ammo_row)
	var ammo_label := Label.new()
	ammo_label.text = "No Ammo:"
	ammo_label.custom_minimum_size.x = 100
	ammo_row.add_child(ammo_label)
	_ammo_btn = Button.new()
	_ammo_btn.text = "Switch to Melee"
	_ammo_btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_ammo_btn.pressed.connect(_cycle_ammo)
	ammo_row.add_child(_ammo_btn)

	# Disengage threshold slider (0–100).
	var disengage_row := HBoxContainer.new()
	disengage_row.add_theme_constant_override("separation", 6)
	style_vbox.add_child(disengage_row)
	_disengage_label = Label.new()
	_disengage_label.text = "Disengage: 0%"
	_disengage_label.custom_minimum_size.x = 100
	disengage_row.add_child(_disengage_label)
	_disengage_slider = HSlider.new()
	_disengage_slider.min_value = 0
	_disengage_slider.max_value = 100
	_disengage_slider.step = 5
	_disengage_slider.value = 0
	_disengage_slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_disengage_slider.value_changed.connect(_on_disengage_changed)
	disengage_row.add_child(_disengage_slider)

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
	var initiative: String = g.get("initiative", "Passive")
	name_btn.text = "%s [%s]" % [display_name, _initiative_display(initiative)]

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

	# Update engagement style controls from group data.
	var initiative: String = group_data.get("initiative", "Passive")
	_initiative_btn.text = _initiative_display(initiative)

	var weapon: String = group_data.get("weapon_preference", "PreferRanged")
	_weapon_btn.text = _weapon_display(weapon)

	var ammo: String = group_data.get("ammo_exhausted", "SwitchToMelee")
	_ammo_btn.text = _ammo_display(ammo)

	var disengage: int = group_data.get("disengage_threshold_pct", 0)
	_disengage_slider.set_value_no_signal(disengage)
	_disengage_label.text = "Disengage: %d%%" % disengage

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


# --- Engagement style ---


func _send_style_field(field: String, value: String) -> void:
	if not _bridge or _detail_group_id < 0:
		return
	# Read current values, override the changed field, then send.
	var groups: Array = _bridge.get_military_groups()
	var group_data: Dictionary = {}
	for g in groups:
		if g.get("id", -1) == _detail_group_id:
			group_data = g
			break
	if group_data.is_empty():
		return
	var wp: String = group_data.get("weapon_preference", "PreferRanged")
	var ae: String = group_data.get("ammo_exhausted", "SwitchToMelee")
	var init: String = group_data.get("initiative", "Passive")
	var dis: int = int(group_data.get("disengage_threshold_pct", 0))
	match field:
		"weapon_preference":
			wp = value
		"ammo_exhausted":
			ae = value
		"initiative":
			init = value
	_bridge.set_group_engagement_style(_detail_group_id, wp, ae, init, dis)
	_refresh_timer = 0.4


func _cycle_initiative() -> void:
	var current: String = _initiative_btn.text
	var next: String
	match current:
		"Aggressive":
			next = "Defensive"
		"Defensive":
			next = "Passive"
		_:
			next = "Aggressive"
	_initiative_btn.text = next
	_send_style_field("initiative", next)


func _cycle_weapon() -> void:
	var current: String = _weapon_btn.text
	var next_val: String
	if current == "Prefer Ranged":
		next_val = "PreferMelee"
	else:
		next_val = "PreferRanged"
	_weapon_btn.text = _weapon_display(next_val)
	_send_style_field("weapon_preference", next_val)


func _cycle_ammo() -> void:
	var current: String = _ammo_btn.text
	var next_val: String
	if current == "Switch to Melee":
		next_val = "Flee"
	else:
		next_val = "SwitchToMelee"
	_ammo_btn.text = _ammo_display(next_val)
	_send_style_field("ammo_exhausted", next_val)


func _on_disengage_changed(value: float) -> void:
	if not _bridge or _detail_group_id < 0:
		return
	_disengage_label.text = "Disengage: %d%%" % int(value)
	# Read current values and send with new threshold.
	var groups: Array = _bridge.get_military_groups()
	var group_data: Dictionary = {}
	for g in groups:
		if g.get("id", -1) == _detail_group_id:
			group_data = g
			break
	if group_data.is_empty():
		return
	_bridge.set_group_engagement_style(
		_detail_group_id,
		group_data.get("weapon_preference", "PreferRanged"),
		group_data.get("ammo_exhausted", "SwitchToMelee"),
		group_data.get("initiative", "Passive"),
		int(value)
	)
	_refresh_timer = 0.4


## Display helpers for engagement style values.
func _initiative_display(val: String) -> String:
	return val


func _weapon_display(val: String) -> String:
	match val:
		"PreferRanged":
			return "Prefer Ranged"
		"PreferMelee":
			return "Prefer Melee"
		_:
			return val


func _ammo_display(val: String) -> String:
	match val:
		"SwitchToMelee":
			return "Switch to Melee"
		"Flee":
			return "Flee"
		_:
			return val


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
		btn.text = "%s [%s]" % [g.get("name", ""), _initiative_display(g.get("initiative", ""))]
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
