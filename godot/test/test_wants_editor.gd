## Unit tests for wants_editor.gd slot conflict detection.
##
## Tests the equip slot conflict logic used by military equipment wants.
## The editor is instantiated as a real node so its _ready() runs, then
## we exercise the conflict detection paths.
##
## See also: wants_editor.gd for the implementation,
## military_panel.gd for how enforce_unique_equip_slots is enabled.
extends GutTest

const WantsEditor = preload("res://scripts/wants_editor.gd")

var _editor: VBoxContainer


func before_each() -> void:
	_editor = VBoxContainer.new()
	_editor.set_script(WantsEditor)
	_editor.enforce_unique_equip_slots = true
	_editor.default_add_quantity = 1
	add_child_autofree(_editor)
	# Provide picker data with equip_slot info.
	var item_kinds: Array = [
		{"kind": "Helmet", "label": "Helmet", "equip_slot": "Head"},
		{"kind": "Hat", "label": "Hat", "equip_slot": "Head"},
		{"kind": "Breastplate", "label": "Breastplate", "equip_slot": "Torso"},
		{"kind": "Bow", "label": "Bow"},
	]
	var mat_options: Dictionary = {}
	_editor.set_picker_data(item_kinds, mat_options)


# -- Equip slot map built from picker data -----------------------------------


func test_equip_slot_map_populated() -> void:
	assert_eq(_editor._get_equip_slot("Helmet"), "Head")
	assert_eq(_editor._get_equip_slot("Hat"), "Head")
	assert_eq(_editor._get_equip_slot("Breastplate"), "Torso")


func test_equip_slot_map_empty_for_non_wearable() -> void:
	assert_eq(_editor._get_equip_slot("Bow"), "")


# -- Slot conflict detection -------------------------------------------------


func test_no_conflict_when_empty() -> void:
	assert_eq(_editor._find_slot_conflict("Helmet"), "")


func test_no_conflict_different_slots() -> void:
	# Add a Helmet want, then check conflict for Breastplate (different slot).
	_editor.update_wants([{"kind": "Helmet", "material_filter": '"Any"', "target_quantity": 1}])
	assert_eq(_editor._find_slot_conflict("Breastplate"), "")


func test_conflict_same_slot() -> void:
	# Add a Hat want, then check conflict for Helmet (both Head).
	_editor.update_wants([{"kind": "Hat", "material_filter": '"Any"', "target_quantity": 1}])
	assert_eq(_editor._find_slot_conflict("Helmet"), "Hat")


func test_no_conflict_for_non_wearable() -> void:
	# Non-wearable items never conflict.
	_editor.update_wants([{"kind": "Helmet", "material_filter": '"Any"', "target_quantity": 1}])
	assert_eq(_editor._find_slot_conflict("Bow"), "")


func test_no_conflict_same_item_kind() -> void:
	# Adding more of the same item kind (quantity bump) is not a conflict.
	_editor.update_wants([{"kind": "Helmet", "material_filter": '"Any"', "target_quantity": 1}])
	assert_eq(_editor._find_slot_conflict("Helmet"), "")


# -- Emit behavior with conflicts -------------------------------------------


func test_emit_blocked_by_slot_conflict() -> void:
	_editor.update_wants([{"kind": "Hat", "material_filter": '"Any"', "target_quantity": 1}])
	watch_signals(_editor)
	# Try to add Helmet (conflicts with Hat on Head slot).
	_editor._emit_wants_with_added("Helmet", '"Any"', 1)
	assert_signal_not_emitted(_editor, "wants_changed")
	assert_true(_editor._error_label.visible, "Error label should be visible")
	assert_true(
		_editor._error_label.text.contains("Head"), "Error should mention the conflicting slot"
	)


func test_emit_succeeds_without_conflict() -> void:
	_editor.update_wants([{"kind": "Helmet", "material_filter": '"Any"', "target_quantity": 1}])
	watch_signals(_editor)
	# Add Breastplate (different slot — no conflict).
	_editor._emit_wants_with_added("Breastplate", '"Any"', 1)
	assert_signal_emitted(_editor, "wants_changed")


func test_emit_not_enforced_when_disabled() -> void:
	_editor.enforce_unique_equip_slots = false
	_editor.update_wants([{"kind": "Hat", "material_filter": '"Any"', "target_quantity": 1}])
	watch_signals(_editor)
	# Helmet + Hat conflict, but enforcement is off (logistics mode).
	_editor._emit_wants_with_added("Helmet", '"Any"', 1)
	assert_signal_emitted(_editor, "wants_changed")
