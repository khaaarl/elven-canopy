## Unit tests for item_detail_panel.gd.
##
## Tests the panel's show/hide lifecycle, field population from info
## dictionaries, row visibility logic (hidden when data is empty/default),
## and owner click signal emission.
extends GutTest

var _panel: PanelContainer


func before_each() -> void:
	var script = load("res://scripts/item_detail_panel.gd")
	_panel = PanelContainer.new()
	_panel.set_script(script)
	add_child_autofree(_panel)


# -- Show / hide lifecycle ----------------------------------------------------


func test_initially_hidden() -> void:
	assert_false(_panel.visible, "Panel should start hidden")
	assert_eq(_panel.get_item_stack_id(), -1)


func test_show_item_makes_visible() -> void:
	var info := _make_info()
	_panel.show_item(42, info)
	assert_true(_panel.visible)
	assert_eq(_panel.get_item_stack_id(), 42)


func test_hide_panel_clears_state() -> void:
	_panel.show_item(42, _make_info())
	_panel.hide_panel()
	assert_false(_panel.visible)
	assert_eq(_panel.get_item_stack_id(), -1)


func test_update_item_empty_dict_hides_panel() -> void:
	_panel.show_item(42, _make_info())
	_panel.update_item({})
	assert_false(_panel.visible, "Empty info should auto-close the panel")


func test_update_item_non_empty_stays_visible() -> void:
	_panel.show_item(42, _make_info())
	_panel.update_item(_make_info())
	assert_true(_panel.visible)


# -- Row visibility logic -----------------------------------------------------


func test_material_row_visibility() -> void:
	var info := _make_info()
	info["material"] = ""
	_panel.show_item(1, info)
	assert_false(_panel._material_row.visible, "Material row should hide when empty")

	info["material"] = "Oak"
	_panel.show_item(1, info)
	assert_true(_panel._material_row.visible)
	assert_eq(_panel._material_label.text, "Oak")


func test_quality_row_visibility() -> void:
	var info := _make_info()
	info["quality_label"] = ""
	_panel.show_item(1, info)
	assert_false(_panel._quality_row.visible)

	info["quality_label"] = "Fine"
	info["quality"] = 0
	_panel.show_item(1, info)
	assert_true(_panel._quality_row.visible)
	assert_eq(_panel._quality_label.text, "Fine (0)")


func test_durability_hidden_when_max_hp_zero() -> void:
	var info := _make_info()
	info["max_hp"] = 0
	info["current_hp"] = 0
	_panel.show_item(1, info)
	assert_false(_panel._durability_row.visible, "Indestructible items hide durability")


func test_durability_visible_with_hp() -> void:
	var info := _make_info()
	info["max_hp"] = 100
	info["current_hp"] = 75
	info["condition"] = ""
	_panel.show_item(1, info)
	assert_true(_panel._durability_row.visible)
	assert_eq(_panel._hp_label.text, "Durability: 75 / 100")


func test_durability_shows_condition_label() -> void:
	var info := _make_info()
	info["max_hp"] = 100
	info["current_hp"] = 30
	info["condition"] = "(damaged)"
	_panel.show_item(1, info)
	assert_eq(_panel._hp_label.text, "Durability: 30 / 100 (damaged)")


func test_equipped_row_visibility() -> void:
	var info := _make_info()
	info["equipped_slot"] = ""
	_panel.show_item(1, info)
	assert_false(_panel._equipped_row.visible)

	info["equipped_slot"] = "Torso"
	_panel.show_item(1, info)
	assert_true(_panel._equipped_row.visible)
	assert_eq(_panel._equipped_label.text, "Torso")


func test_owner_row_visibility() -> void:
	var info := _make_info()
	info["owner_name"] = ""
	info["owner_id"] = ""
	_panel.show_item(1, info)
	assert_false(_panel._owner_row.visible)

	info["owner_name"] = "Aelindra"
	info["owner_id"] = "abc-123"
	_panel.show_item(1, info)
	assert_true(_panel._owner_row.visible)
	assert_eq(_panel._owner_button.text, "Aelindra")


func test_dye_row_visibility() -> void:
	var info := _make_info()
	info["dye_color"] = ""
	_panel.show_item(1, info)
	assert_false(_panel._dye_row.visible)

	info["dye_color"] = "Red"
	_panel.show_item(1, info)
	assert_true(_panel._dye_row.visible)
	assert_eq(_panel._dye_label.text, "Red")


func test_quantity_row_visibility() -> void:
	var info := _make_info()
	info["quantity"] = 1
	_panel.show_item(1, info)
	assert_false(_panel._quantity_row.visible)

	info["quantity"] = 5
	_panel.show_item(1, info)
	assert_true(_panel._quantity_row.visible)
	assert_eq(_panel._quantity_label.text, "5")


func test_reserved_row_hidden_when_not_reserved() -> void:
	var info := _make_info()
	_panel.show_item(1, info)
	assert_false(_panel._reserved_row.visible, "Row should hide when no reservation")


func test_reserved_row_shows_kind_and_state() -> void:
	var info := _make_info()
	info["reserved_task_kind"] = "Haul"
	info["reserved_task_state"] = "In Progress"
	_panel.show_item(1, info)
	assert_true(_panel._reserved_row.visible)
	assert_eq(_panel._reserved_kind_label.text, "Reserved: Haul (In Progress)")


func test_reserved_assignee_visible_when_present() -> void:
	var info := _make_info()
	info["reserved_task_kind"] = "Craft"
	info["reserved_task_state"] = "In Progress"
	info["reserved_task_assignee"] = "Aelindra"
	_panel.show_item(1, info)
	assert_true(_panel._reserved_detail_label.visible)
	assert_eq(_panel._reserved_detail_label.text, "Assignee: Aelindra")


func test_reserved_assignee_hidden_when_absent() -> void:
	var info := _make_info()
	info["reserved_task_kind"] = "Haul"
	info["reserved_task_state"] = "Available"
	_panel.show_item(1, info)
	assert_false(
		_panel._reserved_detail_label.visible,
		"Assignee label should hide when no creature is assigned"
	)


func test_reserved_row_hides_on_transition_to_unreserved() -> void:
	var info := _make_info()
	info["reserved_task_kind"] = "Haul"
	info["reserved_task_state"] = "In Progress"
	info["reserved_task_assignee"] = "Aelindra"
	_panel.show_item(1, info)
	assert_true(_panel._reserved_row.visible)

	# Update with no reservation info — row should hide.
	_panel.update_item(_make_info())
	assert_false(_panel._reserved_row.visible)


func test_reserved_assignee_transition_between_names() -> void:
	var info := _make_info()
	info["reserved_task_kind"] = "Craft"
	info["reserved_task_state"] = "In Progress"
	info["reserved_task_assignee"] = "Aelindra"
	_panel.show_item(1, info)
	assert_eq(_panel._reserved_detail_label.text, "Assignee: Aelindra")

	info["reserved_task_assignee"] = "Brindle"
	_panel.update_item(info)
	assert_eq(_panel._reserved_detail_label.text, "Assignee: Brindle")


func test_reserved_assignee_removed_while_still_reserved() -> void:
	var info := _make_info()
	info["reserved_task_kind"] = "Haul"
	info["reserved_task_state"] = "In Progress"
	info["reserved_task_assignee"] = "Aelindra"
	_panel.show_item(1, info)
	assert_true(_panel._reserved_detail_label.visible)

	# Task still reserved, but creature unassigned.
	var info2 := _make_info()
	info2["reserved_task_kind"] = "Haul"
	info2["reserved_task_state"] = "Available"
	_panel.update_item(info2)
	assert_true(_panel._reserved_row.visible)
	assert_false(_panel._reserved_detail_label.visible)
	assert_eq(_panel._reserved_kind_label.text, "Reserved: Haul (Available)")


func test_reserved_row_updates_via_update_item() -> void:
	_panel.show_item(1, _make_info())
	assert_false(_panel._reserved_row.visible)

	# Add reservation via update_item (not show_item).
	var info := _make_info()
	info["reserved_task_kind"] = "Build"
	info["reserved_task_state"] = "In Progress"
	info["reserved_task_assignee"] = "Caelith"
	_panel.update_item(info)
	assert_true(_panel._reserved_row.visible)
	assert_eq(_panel._reserved_kind_label.text, "Reserved: Build (In Progress)")
	assert_eq(_panel._reserved_detail_label.text, "Assignee: Caelith")


# -- Signal emission ----------------------------------------------------------


func test_owner_clicked_emits_correct_id() -> void:
	var info := _make_info()
	info["owner_name"] = "Aelindra"
	info["owner_id"] = "abc-123"
	_panel.show_item(1, info)
	watch_signals(_panel)
	_panel._on_owner_pressed()
	assert_signal_emitted_with_parameters(_panel, "owner_clicked", ["abc-123"])


func test_owner_pressed_no_owner_does_not_emit() -> void:
	var info := _make_info()
	info["owner_name"] = ""
	info["owner_id"] = ""
	_panel.show_item(1, info)
	watch_signals(_panel)
	_panel._on_owner_pressed()
	assert_signal_not_emitted(_panel, "owner_clicked")


func test_panel_closed_emits_signal() -> void:
	_panel.show_item(1, _make_info())
	watch_signals(_panel)
	_panel._on_close_pressed()
	assert_signal_emitted(_panel, "panel_closed")


func test_name_and_kind_labels_populated() -> void:
	_panel.show_item(1, _make_info())
	assert_eq(_panel._name_label.text, "Fine Oak Bow")
	assert_eq(_panel._kind_label.text, "Bow")


func test_hp_bar_values_and_color() -> void:
	var info := _make_info()
	info["max_hp"] = 100
	info["current_hp"] = 75
	info["condition"] = ""
	_panel.show_item(1, info)
	assert_eq(_panel._hp_bar.max_value, 100.0)
	assert_eq(_panel._hp_bar.value, 75.0)
	# Good condition → green modulate.
	assert_eq(_panel._hp_bar.modulate, Color(0.3, 1.0, 0.3))

	info["current_hp"] = 50
	info["condition"] = "(worn)"
	_panel.show_item(2, info)
	assert_eq(_panel._hp_bar.modulate, Color(1.0, 0.8, 0.3))

	info["current_hp"] = 20
	info["condition"] = "(damaged)"
	_panel.show_item(3, info)
	assert_eq(_panel._hp_bar.modulate, Color(1.0, 0.3, 0.3))


# -- Helpers ------------------------------------------------------------------


func _make_info() -> Dictionary:
	return {
		"display_name": "Fine Oak Bow",
		"kind": "Bow",
		"material": "Oak",
		"quality": 0,
		"quality_label": "Fine",
		"current_hp": 50,
		"max_hp": 100,
		"condition": "",
		"equipped_slot": "",
		"owner_id": "",
		"owner_name": "",
		"dye_color": "",
		"quantity": 1,
	}
