## Unit tests for ground_pile_info_panel.gd inventory display (B-shared-inventory).
##
## Verifies the _last_inventory null-vs-[] cache correctly invalidates when
## switching between piles, preventing stale inventory buttons from lingering.
##
## See also: ground_pile_info_panel.gd for the implementation,
## test_creature_info_panel.gd for parallel creature panel tests.
extends GutTest

const GroundPileInfoPanel = preload("res://scripts/ground_pile_info_panel.gd")

var _panel: PanelContainer


func before_each() -> void:
	_panel = GroundPileInfoPanel.new()
	add_child_autofree(_panel)


func test_show_pile_with_items_displays_inventory_buttons() -> void:
	var info := _minimal_info()
	info["inventory"] = [
		{"kind": "Log", "quantity": 3, "item_stack_id": 10},
	]
	_panel.show_pile(info)
	# 1 empty label (hidden) + 1 item button = 2 children.
	assert_eq(_panel._inventory_container.get_child_count(), 2)
	assert_false(_panel._inventory_empty_label.visible)


func test_switching_to_pile_with_empty_inventory_clears_buttons() -> void:
	# Show pile A with items.
	var info_a := _minimal_info()
	info_a["inventory"] = [
		{"kind": "Log", "quantity": 3, "item_stack_id": 10},
		{"kind": "Berries", "quantity": 5, "item_stack_id": 11},
	]
	_panel.show_pile(info_a)
	assert_eq(_panel._inventory_container.get_child_count(), 3)

	# Switch to pile B with empty inventory.
	var info_b := _minimal_info()
	info_b["inventory"] = []
	_panel.show_pile(info_b)
	# Should only have the empty label, no stale buttons from pile A.
	assert_eq(_panel._inventory_container.get_child_count(), 1)
	assert_true(_panel._inventory_empty_label.visible)


func test_switching_between_piles_with_different_items() -> void:
	var info_a := _minimal_info()
	info_a["inventory"] = [
		{"kind": "Log", "quantity": 3, "item_stack_id": 10},
	]
	_panel.show_pile(info_a)
	assert_eq(_panel._inventory_container.get_child(1).text, "Log: 3")

	var info_b := _minimal_info()
	info_b["inventory"] = [
		{"kind": "Berries", "quantity": 5, "item_stack_id": 20},
	]
	_panel.show_pile(info_b)
	assert_eq(_panel._inventory_container.get_child_count(), 2)
	assert_eq(_panel._inventory_container.get_child(1).text, "Berries: 5")


# -- Helpers -----------------------------------------------------------------


func _minimal_info() -> Dictionary:
	return {
		"x": 10,
		"y": 51,
		"z": 10,
		"inventory": [],
	}
