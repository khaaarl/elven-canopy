## Unit tests for SelectionUtils click/box modifier logic.
##
## Tests the pure selection-modifier helpers without scene dependencies.
## Covers plain click, Shift (toggle/additive), and Alt (subtractive)
## modifiers for both single-click and box-select operations.
##
## See also: selection_utils.gd (implementation), selection_controller.gd
## (consumer).
extends GutTest

# -- Click modifier: no modifier --


func test_click_plain_replaces_selection() -> void:
	var result := SelectionUtils.apply_click_modifier(["a", "b"], "c", false, false)
	assert_eq(result["ids"], ["c"])
	assert_true(result["changed"])


func test_click_plain_replaces_even_if_already_selected() -> void:
	var result := SelectionUtils.apply_click_modifier(["a", "b"], "a", false, false)
	assert_eq(result["ids"], ["a"])
	assert_true(result["changed"])


# -- Click modifier: Shift (toggle) --


func test_click_shift_adds_new_creature() -> void:
	var result := SelectionUtils.apply_click_modifier(["a"], "b", true, false)
	assert_eq(result["ids"], ["a", "b"])
	assert_true(result["changed"])


func test_click_shift_removes_existing_creature() -> void:
	var result := SelectionUtils.apply_click_modifier(["a", "b"], "a", true, false)
	assert_eq(result["ids"], ["b"])
	assert_true(result["changed"])


func test_click_shift_on_empty_selection_adds() -> void:
	var result := SelectionUtils.apply_click_modifier([], "a", true, false)
	assert_eq(result["ids"], ["a"])
	assert_true(result["changed"])


# -- Click modifier: Alt (remove-only) --


func test_click_alt_removes_existing_creature() -> void:
	var result := SelectionUtils.apply_click_modifier(["a", "b", "c"], "b", false, true)
	assert_eq(result["ids"], ["a", "c"])
	assert_true(result["changed"])


func test_click_alt_ignores_creature_not_in_selection() -> void:
	var result := SelectionUtils.apply_click_modifier(["a", "b"], "c", false, true)
	assert_eq(result["ids"], ["a", "b"])
	assert_false(result["changed"])


func test_click_alt_removes_last_creature() -> void:
	var result := SelectionUtils.apply_click_modifier(["a"], "a", false, true)
	assert_eq(result["ids"], [])
	assert_true(result["changed"])


func test_click_alt_on_empty_selection_does_nothing() -> void:
	var result := SelectionUtils.apply_click_modifier([], "a", false, true)
	assert_eq(result["ids"], [])
	assert_false(result["changed"])


# -- Click modifier: Alt takes priority over Shift --


func test_click_alt_shift_both_held_alt_wins() -> void:
	var result := SelectionUtils.apply_click_modifier(["a", "b"], "a", true, true)
	assert_eq(result["ids"], ["b"])
	assert_true(result["changed"])


func test_click_alt_shift_both_held_no_add() -> void:
	# Alt should prevent Shift's add behavior.
	var result := SelectionUtils.apply_click_modifier(["a"], "b", true, true)
	assert_eq(result["ids"], ["a"])
	assert_false(result["changed"])


# -- Box modifier: no modifier --


func test_box_plain_replaces_selection() -> void:
	var result := SelectionUtils.apply_box_modifier(["a"], ["b", "c"], false, false)
	assert_eq(result["ids"], ["b", "c"])
	assert_true(result["changed"])


# -- Box modifier: Shift (additive) --


func test_box_shift_merges_without_duplicates() -> void:
	var result := SelectionUtils.apply_box_modifier(["a", "b"], ["b", "c"], true, false)
	assert_eq(result["ids"], ["a", "b", "c"])
	assert_true(result["changed"])


func test_box_shift_all_already_selected() -> void:
	var result := SelectionUtils.apply_box_modifier(["a", "b"], ["a", "b"], true, false)
	assert_eq(result["ids"], ["a", "b"])
	assert_false(result["changed"])


# -- Box modifier: Alt (subtractive) --


func test_box_alt_removes_matching_creatures() -> void:
	var result := SelectionUtils.apply_box_modifier(["a", "b", "c", "d"], ["b", "d"], false, true)
	assert_eq(result["ids"], ["a", "c"])
	assert_true(result["changed"])


func test_box_alt_none_in_selection() -> void:
	var result := SelectionUtils.apply_box_modifier(["a", "b"], ["c", "d"], false, true)
	assert_eq(result["ids"], ["a", "b"])
	assert_false(result["changed"])


func test_box_alt_removes_all() -> void:
	var result := SelectionUtils.apply_box_modifier(["a", "b"], ["a", "b"], false, true)
	assert_eq(result["ids"], [])
	assert_true(result["changed"])


func test_click_does_not_mutate_original_array() -> void:
	var original: Array = ["a", "b"]
	SelectionUtils.apply_click_modifier(original, "a", false, true)
	assert_eq(original, ["a", "b"], "Original array should not be mutated")


func test_box_does_not_mutate_original_array() -> void:
	var original: Array = ["a", "b", "c"]
	SelectionUtils.apply_box_modifier(original, ["b"], false, true)
	assert_eq(original, ["a", "b", "c"], "Original array should not be mutated")
