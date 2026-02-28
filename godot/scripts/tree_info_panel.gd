## Tree stats/info panel displayed on the right side of the screen.
##
## Shows information about the player's home tree: dimensions, growth level,
## health, mana, fruit, voxel breakdown, and carrying capacity. Built
## programmatically as a PanelContainer with labels and progress bars.
## Sits on the CanvasLayer alongside the spawn toolbar.
##
## The panel is 320px wide, full height, anchored to the right edge — same
## style as creature_info_panel.gd. Toggled by the "Tree [I]" toolbar
## button or the I hotkey. Mutual exclusion with creature_info_panel is
## handled by main.gd: opening this panel deselects any creature, and
## selecting a creature hides this panel.
##
## See also: creature_info_panel.gd for the creature equivalent,
## spawn_toolbar.gd which emits the "TreeInfo" action,
## main.gd which wires everything together,
## sim_bridge.rs get_home_tree_info() for the data source.

extends PanelContainer

signal panel_closed

var _height_label: Label
var _spread_label: Label
var _growth_label: Label
var _health_label: Label
var _mana_bar: ProgressBar
var _mana_label: Label
var _fruit_label: Label
var _fruit_rate_label: Label
var _trunk_label: Label
var _branch_label: Label
var _leaf_label: Label
var _root_label: Label
var _total_label: Label
var _capacity_bar: ProgressBar
var _capacity_label: Label


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
	title.text = "Tree Info"
	title.add_theme_font_size_override("font_size", 20)
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(title)

	var close_btn := Button.new()
	close_btn.text = "X"
	close_btn.pressed.connect(_on_close_pressed)
	header.add_child(close_btn)

	# Separator.
	vbox.add_child(HSeparator.new())

	# Dimensions.
	_height_label = Label.new()
	vbox.add_child(_height_label)

	_spread_label = Label.new()
	vbox.add_child(_spread_label)

	_growth_label = Label.new()
	vbox.add_child(_growth_label)

	_health_label = Label.new()
	vbox.add_child(_health_label)

	# Resources separator.
	var res_sep := HSeparator.new()
	vbox.add_child(res_sep)

	var res_title := Label.new()
	res_title.text = "Resources"
	res_title.add_theme_font_size_override("font_size", 16)
	vbox.add_child(res_title)

	# Mana gauge.
	var mana_row := HBoxContainer.new()
	mana_row.add_theme_constant_override("separation", 6)
	vbox.add_child(mana_row)

	var mana_title := Label.new()
	mana_title.text = "Mana:"
	mana_row.add_child(mana_title)

	_mana_bar = ProgressBar.new()
	_mana_bar.min_value = 0.0
	_mana_bar.max_value = 100.0
	_mana_bar.value = 0.0
	_mana_bar.show_percentage = false
	_mana_bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_mana_bar.custom_minimum_size.y = 20
	mana_row.add_child(_mana_bar)

	_mana_label = Label.new()
	_mana_label.text = "0 / 0"
	_mana_label.custom_minimum_size.x = 90
	_mana_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	mana_row.add_child(_mana_label)

	# Fruit.
	_fruit_label = Label.new()
	vbox.add_child(_fruit_label)

	_fruit_rate_label = Label.new()
	vbox.add_child(_fruit_rate_label)

	# Structure separator.
	var struct_sep := HSeparator.new()
	vbox.add_child(struct_sep)

	var struct_title := Label.new()
	struct_title.text = "Structure"
	struct_title.add_theme_font_size_override("font_size", 16)
	vbox.add_child(struct_title)

	# Voxel breakdown.
	_trunk_label = Label.new()
	vbox.add_child(_trunk_label)

	_branch_label = Label.new()
	vbox.add_child(_branch_label)

	_leaf_label = Label.new()
	vbox.add_child(_leaf_label)

	_root_label = Label.new()
	vbox.add_child(_root_label)

	_total_label = Label.new()
	vbox.add_child(_total_label)

	# Carrying capacity gauge.
	var cap_row := HBoxContainer.new()
	cap_row.add_theme_constant_override("separation", 6)
	vbox.add_child(cap_row)

	var cap_title := Label.new()
	cap_title.text = "Load:"
	cap_row.add_child(cap_title)

	_capacity_bar = ProgressBar.new()
	_capacity_bar.min_value = 0.0
	_capacity_bar.max_value = 100.0
	_capacity_bar.value = 0.0
	_capacity_bar.show_percentage = false
	_capacity_bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_capacity_bar.custom_minimum_size.y = 20
	cap_row.add_child(_capacity_bar)

	_capacity_label = Label.new()
	_capacity_label.text = "0 / 0"
	_capacity_label.custom_minimum_size.x = 90
	_capacity_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	cap_row.add_child(_capacity_label)

	visible = false


func show_panel() -> void:
	visible = true


func hide_panel() -> void:
	visible = false


func toggle() -> void:
	if visible:
		hide_panel()
		panel_closed.emit()
	else:
		show_panel()


func update_info(data: Dictionary) -> void:
	_height_label.text = "Height: %d" % data.get("height", 0)
	var sx: int = data.get("spread_x", 0)
	var sz: int = data.get("spread_z", 0)
	_spread_label.text = "Spread: %d x %d" % [sx, sz]
	_growth_label.text = "Growth Level: %d" % data.get("growth_level", 0)
	_health_label.text = "Health: %.0f" % float(data.get("health", 0))

	# Mana gauge.
	var mana_cap: float = float(data.get("mana_capacity", 1))
	if mana_cap <= 0.0:
		mana_cap = 1.0
	var mana_stored: float = float(data.get("mana_stored", 0))
	_mana_bar.max_value = mana_cap
	_mana_bar.value = mana_stored
	_mana_label.text = "%.0f / %.0f" % [mana_stored, mana_cap]

	# Fruit.
	_fruit_label.text = "Fruit: %d" % data.get("fruit_count", 0)
	_fruit_rate_label.text = "Production Rate: %.2f" % float(data.get("fruit_production_rate", 0))

	# Voxel breakdown.
	_trunk_label.text = "  Trunk: %d" % data.get("trunk_voxels", 0)
	_branch_label.text = "  Branch: %d" % data.get("branch_voxels", 0)
	_leaf_label.text = "  Leaf: %d" % data.get("leaf_voxels", 0)
	_root_label.text = "  Root: %d" % data.get("root_voxels", 0)
	_total_label.text = "  Total: %d" % data.get("total_voxels", 0)

	# Carrying capacity gauge.
	var cap: float = float(data.get("carrying_capacity", 1))
	if cap <= 0.0:
		cap = 1.0
	var load_val: float = float(data.get("current_load", 0))
	_capacity_bar.max_value = cap
	_capacity_bar.value = load_val
	_capacity_label.text = "%.0f / %.0f" % [load_val, cap]


func _unhandled_input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey and event.pressed and not event.echo:
		var key := event as InputEventKey
		if key.keycode == KEY_ESCAPE:
			hide_panel()
			panel_closed.emit()
			get_viewport().set_input_as_handled()


func _on_close_pressed() -> void:
	hide_panel()
	panel_closed.emit()
