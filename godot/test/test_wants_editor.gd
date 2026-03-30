## Unit tests for wants_editor.gd.
##
## Tests equip slot conflict detection, quantity editing emission, and
## the _last_wants dedup that prevents per-frame row rebuilds (which would
## break button click handling due to newly-created nodes having no valid
## layout rect until the next frame).
##
## The editor is instantiated as a real node so its _ready() runs, then
## we exercise the conflict detection and emission paths.
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


# -- Quantity editing emission --------------------------------------------------


func _get_emitted_wants() -> Array:
	var params = get_signal_parameters(_editor, "wants_changed")
	if params == null or params.is_empty():
		return []
	return JSON.parse_string(params[0])


func test_emit_quantity_increment() -> void:
	_editor.update_wants([{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 5}])
	watch_signals(_editor)
	_editor._emit_wants_with_quantity("Bow", '"Any"', 1)
	assert_signal_emitted(_editor, "wants_changed")
	var wants := _get_emitted_wants()
	assert_eq(wants.size(), 1)
	assert_eq(wants[0]["quantity"], 6)


func test_emit_quantity_decrement() -> void:
	_editor.update_wants([{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 5}])
	watch_signals(_editor)
	_editor._emit_wants_with_quantity("Bow", '"Any"', -1)
	assert_signal_emitted(_editor, "wants_changed")
	var wants := _get_emitted_wants()
	assert_eq(wants.size(), 1)
	assert_eq(wants[0]["quantity"], 4)


func test_emit_quantity_clamps_to_one() -> void:
	_editor.update_wants([{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 1}])
	watch_signals(_editor)
	_editor._emit_wants_with_quantity("Bow", '"Any"', -1)
	assert_signal_emitted(_editor, "wants_changed")
	var wants := _get_emitted_wants()
	assert_eq(wants.size(), 1)
	assert_eq(wants[0]["quantity"], 1)


func test_emit_set_quantity() -> void:
	_editor.update_wants([{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 5}])
	watch_signals(_editor)
	_editor._emit_wants_with_set_quantity("Bow", '"Any"', 20)
	assert_signal_emitted(_editor, "wants_changed")
	var wants := _get_emitted_wants()
	assert_eq(wants.size(), 1)
	assert_eq(wants[0]["quantity"], 20)


func test_emit_quantity_only_affects_matching_row() -> void:
	(
		_editor
		. update_wants(
			[
				{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 5},
				{"kind": "Helmet", "material_filter": '"Any"', "target_quantity": 3},
			]
		)
	)
	watch_signals(_editor)
	_editor._emit_wants_with_set_quantity("Bow", '"Any"', 99)
	assert_signal_emitted(_editor, "wants_changed")
	var wants := _get_emitted_wants()
	assert_eq(wants.size(), 2)
	# Bow was changed.
	assert_eq(wants[0]["kind"], "Bow")
	assert_eq(wants[0]["quantity"], 99)
	# Helmet was NOT changed.
	assert_eq(wants[1]["kind"], "Helmet")
	assert_eq(wants[1]["quantity"], 3)


func test_emit_quantity_preserves_other_rows() -> void:
	(
		_editor
		. update_wants(
			[
				{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 10},
				{"kind": "Helmet", "material_filter": '"Any"', "target_quantity": 2},
			]
		)
	)
	watch_signals(_editor)
	_editor._emit_wants_with_quantity("Helmet", '"Any"', 1)
	assert_signal_emitted(_editor, "wants_changed")
	var wants := _get_emitted_wants()
	assert_eq(wants.size(), 2)
	assert_eq(wants[0]["kind"], "Bow")
	assert_eq(wants[0]["quantity"], 10)
	assert_eq(wants[1]["kind"], "Helmet")
	assert_eq(wants[1]["quantity"], 3)


# -- Dedup / rebuild skip ------------------------------------------------------


func test_update_wants_skips_rebuild_when_unchanged() -> void:
	var data: Array = [{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 5}]
	_editor.update_wants(data)
	# Grab a reference to the first row.
	var first_row := _editor._wants_vbox.get_child(0)
	# Call again with identical data — should be a no-op.
	_editor.update_wants(data)
	# The same row object should still be there (not queue_free'd and replaced).
	assert_eq(_editor._wants_vbox.get_child(0), first_row)


func test_update_wants_rebuilds_when_data_changes() -> void:
	_editor.update_wants([{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 5}])
	var first_row := _editor._wants_vbox.get_child(0)
	# Update with different data.
	_editor.update_wants([{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 10}])
	# Row should have been replaced (different object).
	var new_row := _editor._wants_vbox.get_child(0)
	assert_ne(new_row, first_row)
	assert_eq(new_row.get_meta("quantity"), 10)


# -- Rebuilding guard -----------------------------------------------------------


func test_focus_exited_ignored_during_rebuild() -> void:
	_editor.update_wants([{"kind": "Bow", "material_filter": '"Any"', "target_quantity": 5}])
	watch_signals(_editor)
	# Simulate the state during a rebuild (flag stays true until rows are built).
	_editor._rebuilding = true
	# Create a dummy LineEdit to pass to the handler.
	var edit := LineEdit.new()
	edit.text = "99"
	add_child_autofree(edit)
	_editor._on_quantity_focus_exited("Bow", '"Any"', edit)
	assert_signal_not_emitted(_editor, "wants_changed")
