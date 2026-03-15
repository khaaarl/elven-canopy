## Unit tests for ItemUtils (godot/scripts/item_utils.gd).
##
## Covers the condition_label() function which maps item HP ratios to
## human-readable condition strings: "(worn)" and "(damaged)". Tests
## mirror the Rust-side condition_label tests in sim/tests.rs.
extends GutTest

# -- Full HP / indestructible ------------------------------------------------


func test_full_hp_returns_empty() -> void:
	assert_eq(ItemUtils.condition_label(3, 3), "", "Full HP should have no label")


func test_indestructible_returns_empty() -> void:
	assert_eq(ItemUtils.condition_label(0, 0), "", "max_hp=0 (indestructible) should have no label")


func test_negative_max_hp_returns_empty() -> void:
	assert_eq(ItemUtils.condition_label(5, -1), "", "Negative max_hp should have no label")


# -- Worn threshold -----------------------------------------------------------


func test_worn_at_exactly_70_pct() -> void:
	assert_eq(ItemUtils.condition_label(70, 100), "(worn)")


func test_no_label_at_71_pct() -> void:
	assert_eq(ItemUtils.condition_label(71, 100), "", "71% should have no label")


# -- Damaged threshold --------------------------------------------------------


func test_damaged_at_exactly_40_pct() -> void:
	assert_eq(ItemUtils.condition_label(40, 100), "(damaged)")


func test_worn_at_41_pct() -> void:
	assert_eq(ItemUtils.condition_label(41, 100), "(worn)", "41% should be worn, not damaged")


# -- Arrow-specific HP values (max_hp=3) --------------------------------------


func test_arrow_3_of_3_no_label() -> void:
	assert_eq(ItemUtils.condition_label(3, 3), "", "Arrow at 3/3 should have no label")


func test_arrow_2_of_3_worn() -> void:
	# 2/3 = 66% which is <= 70 → worn
	assert_eq(ItemUtils.condition_label(2, 3), "(worn)")


func test_arrow_1_of_3_damaged() -> void:
	# 1/3 = 33% which is <= 40 → damaged
	assert_eq(ItemUtils.condition_label(1, 3), "(damaged)")


# -- Custom thresholds --------------------------------------------------------


func test_custom_thresholds_no_label() -> void:
	assert_eq(ItemUtils.condition_label(60, 100, 50, 20), "", "60% with worn=50 should be fine")


func test_custom_thresholds_worn() -> void:
	assert_eq(ItemUtils.condition_label(50, 100, 50, 20), "(worn)")


func test_custom_thresholds_damaged() -> void:
	assert_eq(ItemUtils.condition_label(20, 100, 50, 20), "(damaged)")


# -- Edge cases ---------------------------------------------------------------


func test_current_hp_zero_is_damaged() -> void:
	# 0/100 = 0% → damaged (item about to break but still displayed)
	assert_eq(ItemUtils.condition_label(0, 100), "(damaged)")


func test_hp_1_of_1_is_full() -> void:
	assert_eq(ItemUtils.condition_label(1, 1), "", "1/1 is 100%, no label")
