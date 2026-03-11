## Keyboard shortcuts help overlay.
##
## Full-screen panel listing all keyboard and mouse controls, organized by
## category. Toggled via the ? key or the "? Help" toolbar button. Follows
## the same full-screen overlay pattern as task_panel.gd, structure_list_panel.gd,
## and units_panel.gd (ColorRect with ESC to close, CanvasLayer layer 2).
##
## The binding list is static — all shortcuts are hardcoded here rather than
## introspected from input maps, since many bindings are handled via direct
## keycode checks rather than Godot input actions.
##
## See also: action_toolbar.gd for the toolbar button and ? key handler,
## main.gd for wiring and panel creation.

extends ColorRect

signal panel_closed


func _ready() -> void:
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	color = Color(0.10, 0.12, 0.08, 0.90)
	visible = false

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
	title.text = "Keyboard Shortcuts"
	title.add_theme_font_size_override("font_size", 28)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header_hbox.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "Close [ESC]"
	close_btn.pressed.connect(hide_panel)
	header_hbox.add_child(close_btn)

	# Scrollable content.
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	outer_vbox.add_child(scroll)

	var content := VBoxContainer.new()
	content.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	content.add_theme_constant_override("separation", 20)
	scroll.add_child(content)

	_build_sections(content)


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


func _build_sections(parent: VBoxContainer) -> void:
	# Two-column layout.
	var columns := HBoxContainer.new()
	columns.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	columns.add_theme_constant_override("separation", 40)
	parent.add_child(columns)

	var left := VBoxContainer.new()
	left.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left.add_theme_constant_override("separation", 20)
	columns.add_child(left)

	var right := VBoxContainer.new()
	right.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	right.add_theme_constant_override("separation", 20)
	columns.add_child(right)

	# Left column.
	_add_section(
		left,
		"Camera",
		[
			["W / A / S / D", "Move camera"],
			["Q / E", "Rotate camera"],
			["Arrow Keys", "Rotate / tilt camera"],
			["+  /  -", "Zoom in / out"],
			["Scroll Wheel", "Zoom in / out"],
			["Middle Mouse Drag", "Orbit camera"],
			["Page Up", "Move view up"],
			["Page Down", "Move view down"],
		]
	)

	_add_section(
		left,
		"Speed",
		[
			["Space", "Pause / resume"],
			["1", "Normal speed (1x)"],
			["2", "Fast speed (2x)"],
			["3", "Very fast speed (5x)"],
		]
	)

	_add_section(
		left,
		"Construction Mode",
		[
			["B", "Enter / exit build mode"],
			["P", "Platform mode"],
			["G", "Building mode"],
			["L", "Ladder mode"],
			["C", "Carve mode"],
			["Left Click + Drag", "Place construction"],
			["Enter", "Confirm placement"],
			["Right Click", "Cancel placement"],
		]
	)

	# Right column.
	_add_section(
		right,
		"Panels",
		[
			["T", "Tasks panel"],
			["U", "Units panel"],
			["M", "Military groups"],
			["I", "Tree info"],
			["?", "This help panel"],
			["F12", "Toggle debug tools"],
		]
	)

	_add_section(
		right,
		"Selection",
		[
			["Left Click", "Select creature / structure / pile"],
			["Shift + Click/Drag", "Add to selection"],
			["Right Click", "Context command (attack / move)"],
			["F + Click", "Attack-move to location"],
			["ESC", "Deselect"],
		]
	)

	_add_section(
		right,
		"General",
		[
			["ESC", "Close panel / cancel / pause menu"],
		]
	)


func _add_section(parent: VBoxContainer, title: String, bindings: Array) -> void:
	var section := VBoxContainer.new()
	section.add_theme_constant_override("separation", 4)
	parent.add_child(section)

	var header := Label.new()
	header.text = title
	header.add_theme_font_size_override("font_size", 20)
	header.add_theme_color_override("font_color", Color(0.75, 0.90, 0.65))
	section.add_child(header)

	var sep := HSeparator.new()
	section.add_child(sep)

	for binding in bindings:
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 8)
		section.add_child(row)

		var key_label := Label.new()
		key_label.text = binding[0]
		key_label.custom_minimum_size.x = 180
		key_label.add_theme_font_size_override("font_size", 15)
		key_label.add_theme_color_override("font_color", Color(0.95, 0.85, 0.55))
		row.add_child(key_label)

		var desc_label := Label.new()
		desc_label.text = binding[1]
		desc_label.add_theme_font_size_override("font_size", 15)
		desc_label.add_theme_color_override("font_color", Color(0.80, 0.80, 0.80))
		row.add_child(desc_label)
