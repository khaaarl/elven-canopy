## Full-screen task list overlay showing active group activities and tasks.
##
## The panel has four sections from top to bottom:
## - **Group Activities** — multi-creature coordination (dances, choirs).
##   Each card shows kind, phase, participant count or progress bar, and a
##   per-participant list with creature name buttons (zoom on click) and
##   status labels (Volunteered / Traveling / Arrived).
## - **Player Directives** — tasks the player explicitly created (build, goto,
##   furnish). Origin string: "PlayerDirected".
## - **Automated Management** — tasks created by management systems (not yet
##   used). Origin string: "Automated".
## - **Autonomous Decisions** — tasks creatures create on their own (eat, sleep).
##   Origin string: "Autonomous".
##
## Each task card shows the task kind, state, progress (for Build/Furnish tasks),
## a "Zoom to Site" button, and per-assignee zoom buttons showing creature
## names (Vaelith names for elves, species + short ID for unnamed creatures).
##
## Data flow: main.gd calls update_tasks(data) and update_activities(data) each
## frame while the panel is visible, passing the results of
## bridge.get_active_tasks() and bridge.get_active_activities(). Both use a
## reconciliation pattern — dictionaries map id_full to card nodes,
## creating/updating/removing cards as data changes.
##
## Signals:
## - zoom_to_creature(creature_id) — zoom camera to an assigned creature
## - zoom_to_location(x, y, z) — zoom camera to the task's work site
## - panel_closed — emitted when the panel is hidden (ESC or close button)
##
## ESC handling: when visible, consumes ESC in _unhandled_input and closes.
## This sits in the ESC precedence chain between selection_controller and
## escape_menu (see main.gd docstring).
##
## See also: main.gd (creates and wires this panel), sim_bridge.rs for
## get_active_tasks() and get_active_activities(), action_toolbar.gd for the
## "Tasks [T]" button, selection_controller.gd for select_creature_by_id()
## used by zoom-to-assignee.

extends ColorRect

signal zoom_to_creature(creature_id: String)
signal zoom_to_location(x: float, y: float, z: float)
signal panel_closed

## Maps task id_full (String) -> PanelContainer card node.
var _task_rows: Dictionary = {}
## Maps activity id_full (String) -> PanelContainer card node.
var _activity_rows: Dictionary = {}
## The three section containers, keyed by origin string.
var _section_player: VBoxContainer
var _section_automated: VBoxContainer
var _section_autonomous: VBoxContainer
## Activities section container.
var _section_activities: VBoxContainer
var _label_activities_empty: Label
## Per-section "(none)" labels.
var _label_player_empty: Label
var _label_automated_empty: Label
var _label_autonomous_empty: Label
## Top-level empty label and sections container.
var _empty_label: Label
var _sections_vbox: VBoxContainer


func _ready() -> void:
	# Full-screen semi-transparent overlay.
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	color = Color(0.10, 0.12, 0.08, 0.90)

	var margin := MarginContainer.new()
	margin.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	margin.add_theme_constant_override("margin_left", 60)
	margin.add_theme_constant_override("margin_right", 60)
	margin.add_theme_constant_override("margin_top", 40)
	margin.add_theme_constant_override("margin_bottom", 40)
	add_child(margin)

	var outer_vbox := VBoxContainer.new()
	outer_vbox.add_theme_constant_override("separation", 12)
	margin.add_child(outer_vbox)

	# Header row.
	var header_hbox := HBoxContainer.new()
	outer_vbox.add_child(header_hbox)

	var title := Label.new()
	title.text = "Tasks"
	title.add_theme_font_size_override("font_size", 28)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header_hbox.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.custom_minimum_size = Vector2(40, 40)
	close_btn.pressed.connect(hide_panel)
	header_hbox.add_child(close_btn)

	var sep := HSeparator.new()
	outer_vbox.add_child(sep)

	# Empty-state label (shown when no tasks exist at all).
	_empty_label = Label.new()
	_empty_label.text = "No active tasks."
	_empty_label.add_theme_font_size_override("font_size", 18)
	_empty_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	outer_vbox.add_child(_empty_label)

	# Scrollable task list with grouped sections.
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	outer_vbox.add_child(scroll)

	_sections_vbox = VBoxContainer.new()
	_sections_vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_sections_vbox.add_theme_constant_override("separation", 8)
	scroll.add_child(_sections_vbox)

	# Group Activities section (above tasks).
	_section_activities = _create_section("Group Activities")
	_label_activities_empty = _section_activities.get_meta("empty_label")

	# Separator.
	_sections_vbox.add_child(HSeparator.new())

	# Player Directives section.
	_section_player = _create_section("Player Directives")
	_label_player_empty = _section_player.get_meta("empty_label")

	# Separator.
	_sections_vbox.add_child(HSeparator.new())

	# Automated Management section.
	_section_automated = _create_section("Automated Management")
	_label_automated_empty = _section_automated.get_meta("empty_label")

	# Separator.
	_sections_vbox.add_child(HSeparator.new())

	# Autonomous Decisions section.
	_section_autonomous = _create_section("Autonomous Decisions")
	_label_autonomous_empty = _section_autonomous.get_meta("empty_label")

	# Start hidden.
	visible = false


## Create a section with a header label, card container, and empty label.
## Returns the card VBoxContainer (stores the empty label in meta).
func _create_section(header_text: String) -> VBoxContainer:
	var header := Label.new()
	header.text = header_text
	header.add_theme_font_size_override("font_size", 20)
	header.add_theme_color_override("font_color", Color(0.85, 0.80, 0.65))
	_sections_vbox.add_child(header)

	var container := VBoxContainer.new()
	container.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	container.add_theme_constant_override("separation", 8)
	_sections_vbox.add_child(container)

	var empty_label := Label.new()
	empty_label.text = "(none)"
	empty_label.add_theme_font_size_override("font_size", 14)
	empty_label.add_theme_color_override("font_color", Color(0.6, 0.6, 0.5))
	_sections_vbox.add_child(empty_label)

	container.set_meta("empty_label", empty_label)
	return container


func show_panel() -> void:
	visible = true


func hide_panel() -> void:
	visible = false
	panel_closed.emit()


func toggle() -> void:
	if visible:
		hide_panel()
	else:
		show_panel()


func _unhandled_input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey:
		var key := event as InputEventKey
		if key.pressed and key.keycode == KEY_ESCAPE:
			hide_panel()
			get_viewport().set_input_as_handled()


## Return the section VBoxContainer for a given origin string.
func _section_for_origin(origin: String) -> VBoxContainer:
	if origin == "Automated":
		return _section_automated
	if origin == "Autonomous":
		return _section_autonomous
	return _section_player


## Called each frame by main.gd with the result of bridge.get_active_tasks().
## Uses reconciliation: creates new cards, updates existing ones, removes stale.
func update_tasks(data: Array) -> void:
	# Collect ids present in this frame's data.
	var seen_ids: Dictionary = {}
	for i in data.size():
		var task: Dictionary = data[i]
		var id_full: String = task.get("id_full", "")
		var origin: String = task.get("origin", "PlayerDirected")
		seen_ids[id_full] = true

		if _task_rows.has(id_full):
			var card: PanelContainer = _task_rows[id_full]
			# If the card moved sections (shouldn't normally happen), reparent.
			var target_section := _section_for_origin(origin)
			if card.get_parent() != target_section:
				card.get_parent().remove_child(card)
				target_section.add_child(card)
			_update_card(card, task)
		else:
			var card := _create_card(task)
			var section := _section_for_origin(origin)
			section.add_child(card)
			_task_rows[id_full] = card

	# Remove cards for tasks no longer in data.
	var to_remove: Array = []
	for id_full in _task_rows:
		if not seen_ids.has(id_full):
			to_remove.append(id_full)
	for id_full in to_remove:
		var card: PanelContainer = _task_rows[id_full]
		card.get_parent().remove_child(card)
		card.queue_free()
		_task_rows.erase(id_full)

	# Show/hide per-section empty labels and top-level empty state.
	_label_player_empty.visible = _section_player.get_child_count() == 0
	_label_automated_empty.visible = _section_automated.get_child_count() == 0
	_label_autonomous_empty.visible = _section_autonomous.get_child_count() == 0

	var all_empty := _task_rows.is_empty() and _activity_rows.is_empty()
	_empty_label.visible = all_empty
	_sections_vbox.visible = not all_empty


func _create_card(task: Dictionary) -> PanelContainer:
	var card := PanelContainer.new()

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 4)
	card.add_child(vbox)

	# Header row: Kind+State label, progress bar, zoom-to-site button.
	var header := HBoxContainer.new()
	header.add_theme_constant_override("separation", 12)
	vbox.add_child(header)

	var kind_label := Label.new()
	kind_label.name = "KindLabel"
	kind_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(kind_label)

	var progress_bar := ProgressBar.new()
	progress_bar.name = "ProgressBar"
	progress_bar.custom_minimum_size = Vector2(150, 20)
	progress_bar.show_percentage = true
	header.add_child(progress_bar)

	var zoom_site_btn := Button.new()
	zoom_site_btn.name = "ZoomSiteBtn"
	zoom_site_btn.text = "Zoom to Site"
	zoom_site_btn.pressed.connect(
		func():
			var lx: float = task.get("location_x", 0)
			var ly: float = task.get("location_y", 0)
			var lz: float = task.get("location_z", 0)
			zoom_to_location.emit(lx, ly, lz)
	)
	header.add_child(zoom_site_btn)

	# Assignees row.
	var assignee_row := HBoxContainer.new()
	assignee_row.name = "AssigneeRow"
	assignee_row.add_theme_constant_override("separation", 8)
	vbox.add_child(assignee_row)

	var assigned_label := Label.new()
	assigned_label.name = "AssignedLabel"
	assigned_label.text = "Assigned:"
	assignee_row.add_child(assigned_label)

	_update_card(card, task)
	return card


func _update_card(card: PanelContainer, task: Dictionary) -> void:
	var kind: String = task.get("kind", "?")
	var state: String = task.get("state", "?")
	var progress: float = task.get("progress", 0.0)
	var total_cost: float = task.get("total_cost", 0.0)

	var kind_label: Label = card.find_child("KindLabel", true, false)
	if kind_label:
		kind_label.text = "%s \u2014 %s" % [kind, state]

	var progress_bar: ProgressBar = card.find_child("ProgressBar", true, false)
	if progress_bar:
		if total_cost > 0.0:
			progress_bar.visible = true
			progress_bar.max_value = total_cost
			progress_bar.value = progress
		else:
			progress_bar.visible = false

	# Rebuild assignee buttons only when the assignee list changes.
	# Buttons fire on mouse release, so recreating them every frame would
	# destroy the button between press and release, swallowing the click.
	var assignee_row: HBoxContainer = card.find_child("AssigneeRow", true, false)
	if assignee_row:
		var assignees: Array = task.get("assignees", [])
		var fingerprint := _assignee_fingerprint(assignees)
		var prev: String = card.get_meta("assignee_fp", "")
		if fingerprint != prev:
			card.set_meta("assignee_fp", fingerprint)
			if assignees.is_empty():
				assignee_row.visible = false
			else:
				assignee_row.visible = true
			# Remove old buttons (keep the "Assigned:" label at index 0).
			while assignee_row.get_child_count() > 1:
				var child := assignee_row.get_child(assignee_row.get_child_count() - 1)
				assignee_row.remove_child(child)
				child.queue_free()
			for j in assignees.size():
				var a: Dictionary = assignees[j]
				var sp: String = a.get("species", "?")
				var cid: String = a.get("creature_id", "")
				var id_short: String = a.get("id_short", "?")
				var creature_name: String = a.get("name", "")
				var btn := Button.new()
				if creature_name != "":
					btn.text = creature_name
				else:
					btn.text = "%s %s" % [sp, id_short]
				btn.pressed.connect(func(): zoom_to_creature.emit(cid))
				assignee_row.add_child(btn)


## Build a short string fingerprint of the assignee list for change detection.
## Includes name so buttons rebuild when a creature gets named.
func _assignee_fingerprint(assignees: Array) -> String:
	if assignees.is_empty():
		return ""
	var parts: PackedStringArray = PackedStringArray()
	for i in assignees.size():
		var a: Dictionary = assignees[i]
		var creature_name: String = a.get("name", "")
		parts.append("%s:%s" % [a.get("id_short", ""), creature_name])
	return ",".join(parts)


# ===========================================================================
# Group Activities
# ===========================================================================


## Called each frame by main.gd with the result of bridge.get_active_activities().
func update_activities(data: Array) -> void:
	var seen_ids: Dictionary = {}
	for i in data.size():
		var activity: Dictionary = data[i]
		var id_full: String = activity.get("id_full", "")
		seen_ids[id_full] = true

		if _activity_rows.has(id_full):
			_update_activity_card(_activity_rows[id_full], activity)
		else:
			var card := _create_activity_card(activity)
			_section_activities.add_child(card)
			_activity_rows[id_full] = card

	# Remove cards for activities no longer present.
	var to_remove: Array = []
	for id_full in _activity_rows:
		if not seen_ids.has(id_full):
			to_remove.append(id_full)
	for id_full in to_remove:
		var card: PanelContainer = _activity_rows[id_full]
		card.get_parent().remove_child(card)
		card.queue_free()
		_activity_rows.erase(id_full)

	_label_activities_empty.visible = _section_activities.get_child_count() == 0

	# Update top-level empty state (consider both tasks and activities).
	var all_empty := _task_rows.is_empty() and _activity_rows.is_empty()
	_empty_label.visible = all_empty
	_sections_vbox.visible = not all_empty


func _create_activity_card(activity: Dictionary) -> PanelContainer:
	var card := PanelContainer.new()

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 4)
	card.add_child(vbox)

	# Header row: Kind + Phase, participant count or progress, zoom button.
	var header := HBoxContainer.new()
	header.add_theme_constant_override("separation", 12)
	vbox.add_child(header)

	var kind_label := Label.new()
	kind_label.name = "KindLabel"
	kind_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(kind_label)

	var count_label := Label.new()
	count_label.name = "CountLabel"
	count_label.add_theme_font_size_override("font_size", 14)
	count_label.add_theme_color_override("font_color", Color(0.7, 0.7, 0.6))
	header.add_child(count_label)

	var progress_bar := ProgressBar.new()
	progress_bar.name = "ProgressBar"
	progress_bar.custom_minimum_size = Vector2(150, 20)
	progress_bar.show_percentage = true
	header.add_child(progress_bar)

	var zoom_btn := Button.new()
	zoom_btn.name = "ZoomSiteBtn"
	zoom_btn.text = "Zoom to Site"
	zoom_btn.pressed.connect(
		func():
			var lx: float = activity.get("location_x", 0)
			var ly: float = activity.get("location_y", 0)
			var lz: float = activity.get("location_z", 0)
			zoom_to_location.emit(lx, ly, lz)
	)
	header.add_child(zoom_btn)

	# Participant list container.
	var participants_vbox := VBoxContainer.new()
	participants_vbox.name = "ParticipantsVBox"
	participants_vbox.add_theme_constant_override("separation", 2)
	vbox.add_child(participants_vbox)

	_update_activity_card(card, activity)
	return card


func _update_activity_card(card: PanelContainer, activity: Dictionary) -> void:
	var kind: String = activity.get("kind", "?")
	var phase: String = activity.get("phase", "?")
	var progress: float = activity.get("progress", 0.0)
	var total_cost: float = activity.get("total_cost", 0.0)
	var participants: Array = activity.get("participants", [])
	var min_count: int = activity.get("min_count", 0)
	var desired_count: int = activity.get("desired_count", 0)

	var kind_label: Label = card.find_child("KindLabel", true, false)
	if kind_label:
		kind_label.text = "%s (%s)" % [kind, phase]

	# Show participant count during Recruiting/Assembling, progress during Executing.
	var count_label: Label = card.find_child("CountLabel", true, false)
	var progress_bar: ProgressBar = card.find_child("ProgressBar", true, false)
	if phase == "Executing" or phase == "Paused":
		if count_label:
			count_label.visible = false
		if progress_bar and total_cost > 0.0:
			progress_bar.visible = true
			progress_bar.max_value = total_cost
			progress_bar.value = progress
		elif progress_bar:
			progress_bar.visible = false
	else:
		if progress_bar:
			progress_bar.visible = false
		if count_label:
			count_label.visible = true
			var target: int = desired_count if desired_count > 0 else min_count
			count_label.text = "%d/%d" % [participants.size(), target]

	# Rebuild participant rows when the list changes.
	var fingerprint := _participant_fingerprint(participants)
	var prev: String = card.get_meta("participant_fp", "")
	if fingerprint != prev:
		card.set_meta("participant_fp", fingerprint)
		var participants_vbox: VBoxContainer = card.find_child("ParticipantsVBox", true, false)
		if participants_vbox:
			# Clear old rows.
			for child in participants_vbox.get_children():
				participants_vbox.remove_child(child)
				child.queue_free()
			# Create new rows.
			for j in participants.size():
				var p: Dictionary = participants[j]
				var row := HBoxContainer.new()
				row.add_theme_constant_override("separation", 8)

				var bullet := Label.new()
				bullet.text = "\u2022"
				bullet.add_theme_font_size_override("font_size", 14)
				row.add_child(bullet)

				var cid: String = p.get("creature_id", "")
				var creature_name: String = p.get("name", "")
				var sp: String = p.get("species", "?")
				var status: String = p.get("status", "?")

				var name_btn := Button.new()
				name_btn.flat = true
				if creature_name != "":
					name_btn.text = creature_name
				else:
					name_btn.text = sp
				name_btn.pressed.connect(func(): zoom_to_creature.emit(cid))
				row.add_child(name_btn)

				var status_label := Label.new()
				status_label.text = status
				status_label.add_theme_font_size_override("font_size", 13)
				status_label.add_theme_color_override("font_color", _status_color(status))
				row.add_child(status_label)

				participants_vbox.add_child(row)


## Color-code participant status for visual clarity.
func _status_color(status: String) -> Color:
	match status:
		"Volunteered":
			return Color(0.6, 0.6, 0.5)
		"Traveling":
			return Color(0.7, 0.8, 0.5)
		"Arrived":
			return Color(0.5, 0.9, 0.5)
		_:
			return Color(0.7, 0.7, 0.7)


## Fingerprint for participant list change detection.
func _participant_fingerprint(participants: Array) -> String:
	if participants.is_empty():
		return ""
	var parts: PackedStringArray = PackedStringArray()
	for i in participants.size():
		var p: Dictionary = participants[i]
		parts.append(
			"%s:%s:%s" % [p.get("creature_id", ""), p.get("status", ""), p.get("name", "")]
		)
	return ",".join(parts)
