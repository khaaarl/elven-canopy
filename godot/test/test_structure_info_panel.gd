## Unit tests for structure_info_panel.gd inventory display (B-shared-inventory).
##
## Verifies the _last_inventory null-vs-[] cache correctly invalidates when
## switching between structures, preventing stale inventory buttons from
## lingering.
##
## See also: structure_info_panel.gd for the implementation,
## test_creature_info_panel.gd for parallel creature panel tests.
extends GutTest

const StructureInfoPanel = preload("res://scripts/structure_info_panel.gd")

var _panel: PanelContainer


func before_each() -> void:
	_panel = StructureInfoPanel.new()
	add_child_autofree(_panel)


func test_show_structure_with_items_displays_inventory_buttons() -> void:
	var info := _minimal_info()
	info["inventory"] = [
		{"kind": "Log", "quantity": 3, "item_stack_id": 10},
	]
	_panel.show_structure(info)
	# 1 empty label (hidden) + 1 item button = 2 children.
	assert_eq(_panel._inventory_container.get_child_count(), 2)
	assert_false(_panel._inventory_empty_label.visible)


func test_switching_to_structure_with_empty_inventory_clears_buttons() -> void:
	# Show structure A with items.
	var info_a := _minimal_info()
	info_a["id"] = 1
	info_a["inventory"] = [
		{"kind": "Log", "quantity": 3, "item_stack_id": 10},
		{"kind": "Berries", "quantity": 5, "item_stack_id": 11},
	]
	_panel.show_structure(info_a)
	assert_eq(_panel._inventory_container.get_child_count(), 3)

	# Switch to structure B with empty inventory.
	var info_b := _minimal_info()
	info_b["id"] = 2
	info_b["inventory"] = []
	_panel.show_structure(info_b)
	# Should only have the empty label, no stale buttons from structure A.
	assert_eq(_panel._inventory_container.get_child_count(), 1)
	assert_true(_panel._inventory_empty_label.visible)


func test_switching_between_structures_with_different_items() -> void:
	var info_a := _minimal_info()
	info_a["id"] = 1
	info_a["inventory"] = [
		{"kind": "Log", "quantity": 3, "item_stack_id": 10},
	]
	_panel.show_structure(info_a)
	assert_eq(_panel._inventory_container.get_child(1).text, "Log: 3")

	var info_b := _minimal_info()
	info_b["id"] = 2
	info_b["inventory"] = [
		{"kind": "Berries", "quantity": 5, "item_stack_id": 20},
	]
	_panel.show_structure(info_b)
	assert_eq(_panel._inventory_container.get_child_count(), 2)
	assert_eq(_panel._inventory_container.get_child(1).text, "Berries: 5")


# -- Helpers -----------------------------------------------------------------


## Minimal info dictionary with required fields to avoid errors in show_structure.
func _minimal_info() -> Dictionary:
	return {
		"id": 0,
		"build_type": "Platform",
		"name": "Test",
		"width": 3,
		"depth": 3,
		"height": 1,
		"anchor_x": 10,
		"anchor_y": 51,
		"anchor_z": 10,
		"furnishing": "",
		"furniture_noun": "",
		"furniture_count": 0,
		"planned_furniture_count": 0,
		"is_furnishing": false,
		"assigned_elf_id": "",
		"assigned_elf_name": "",
		"inventory": [],
		"logistics_priority": -1,
		"logistics_wants": [],
		"crafting_enabled": false,
		"active_recipes": [],
		"active_recipe_keys": [],
		"greenhouse_species": "",
		"greenhouse_enabled": false,
	}
