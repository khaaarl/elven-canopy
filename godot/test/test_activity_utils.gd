## Unit tests for ActivityUtils (godot/scripts/activity_utils.gd).
##
## Covers the canonical LABELS table and get_label() helper that maps
## sim task_kind strings to human-readable activity names used by
## units_panel.gd, group_info_panel.gd, and tooltip_controller.gd.
extends GutTest


func test_known_task_kinds() -> void:
	assert_eq(ActivityUtils.get_label("GoTo"), "Walking")
	assert_eq(ActivityUtils.get_label("Build"), "Building")
	assert_eq(ActivityUtils.get_label("Attack"), "Attacking")
	assert_eq(ActivityUtils.get_label("Craft"), "Crafting")
	assert_eq(ActivityUtils.get_label("EatBread"), "Eating")
	assert_eq(ActivityUtils.get_label("EatFruit"), "Eating")
	assert_eq(ActivityUtils.get_label("Sleep"), "Sleeping")
	assert_eq(ActivityUtils.get_label("Furnish"), "Furnishing")
	assert_eq(ActivityUtils.get_label("Haul"), "Hauling")
	assert_eq(ActivityUtils.get_label("Cook"), "Cooking")
	assert_eq(ActivityUtils.get_label("Harvest"), "Harvesting")
	assert_eq(ActivityUtils.get_label("AcquireItem"), "Fetching")
	assert_eq(ActivityUtils.get_label("Moping"), "Moping")
	assert_eq(ActivityUtils.get_label("AttackMove"), "Attack Moving")
	assert_eq(ActivityUtils.get_label("Equip"), "Equipping")
	assert_eq(ActivityUtils.get_label("Chatting"), "Chatting")
	assert_eq(ActivityUtils.get_label("Dine"), "Dining")
	assert_eq(ActivityUtils.get_label("Graze"), "Grazing")
	assert_eq(ActivityUtils.get_label("Tame"), "Taming")


func test_empty_string_returns_idle() -> void:
	assert_eq(ActivityUtils.get_label(""), "Idle")


func test_unknown_kind_returns_default() -> void:
	assert_eq(ActivityUtils.get_label("SomeNewTask"), "Idle")


func test_unknown_kind_with_custom_default() -> void:
	assert_eq(ActivityUtils.get_label("SomeNewTask", "SomeNewTask"), "SomeNewTask")


func test_labels_dict_has_all_entries() -> void:
	# Ensure the table has the expected number of entries (catches accidental
	# deletions).  Update this count when new task kinds are added.
	assert_eq(ActivityUtils.LABELS.size(), 20)
