## Unit tests for selection logic helpers in geometry_utils.gd.
##
## Tests roof-click-select (GeometryUtils.is_shielded_by_roof) and
## double-click group matching (GeometryUtils.matches_double_click_group).
## Both predicates are pure functions testable without scene dependencies.
##
## See also: selection_controller.gd (click-to-select, double-click-select),
## tooltip_controller.gd, geometry_utils.gd for the implementations.
extends GutTest


func test_creature_below_roof_is_shielded() -> void:
	# Creature at Y=1, roof at Y=3: shielded (inside the building).
	assert_true(
		GeometryUtils.is_shielded_by_roof(1, true, 3),
		"Creature below roof should be shielded",
	)


func test_creature_at_roof_level_is_not_shielded() -> void:
	# Creature at Y=3, roof at Y=3: NOT shielded (standing on the roof).
	assert_false(
		GeometryUtils.is_shielded_by_roof(3, true, 3),
		"Creature at roof Y should not be shielded",
	)


func test_creature_above_roof_is_not_shielded() -> void:
	# Creature at Y=5, roof at Y=3: NOT shielded (above the building).
	assert_false(
		GeometryUtils.is_shielded_by_roof(5, true, 3),
		"Creature above roof should not be shielded",
	)


func test_no_roof_hit_never_shields() -> void:
	# When is_roof is false, no creature should be shielded regardless of Y.
	assert_false(
		GeometryUtils.is_shielded_by_roof(0, false, 3),
		"No roof hit should never shield",
	)
	assert_false(
		GeometryUtils.is_shielded_by_roof(1, false, 3),
		"No roof hit should never shield even below roof_y",
	)


# --- Double-click group matching ---


func test_dblclick_same_military_group_matches() -> void:
	# Both player-civ, same group ID → should match.
	assert_true(
		GeometryUtils.matches_double_click_group(5, true, 5, true),
		"Same military group should match",
	)


func test_dblclick_different_military_group_no_match() -> void:
	# Both player-civ, different group IDs → should not match.
	assert_false(
		GeometryUtils.matches_double_click_group(5, true, 7, true),
		"Different military groups should not match",
	)


func test_dblclick_civilians_match_each_other() -> void:
	# Both player-civ, both civilian (group_id == -1) → should match.
	assert_true(
		GeometryUtils.matches_double_click_group(-1, true, -1, true),
		"Civilians (group -1) should match each other",
	)


func test_dblclick_civilian_does_not_match_military() -> void:
	# Player-civ civilian vs player-civ military → should not match.
	assert_false(
		GeometryUtils.matches_double_click_group(-1, true, 5, true),
		"Civilian should not match military group",
	)
	assert_false(
		GeometryUtils.matches_double_click_group(5, true, -1, true),
		"Military group should not match civilian",
	)


func test_dblclick_non_player_target_no_match() -> void:
	# Target is not player-civ → no group select.
	assert_false(
		GeometryUtils.matches_double_click_group(5, true, 5, false),
		"Non-player target should not trigger group select",
	)


func test_dblclick_non_player_candidate_no_match() -> void:
	# Candidate is not player-civ → should not be included.
	assert_false(
		GeometryUtils.matches_double_click_group(5, false, 5, true),
		"Non-player candidate should not be included in group select",
	)


func test_dblclick_both_non_player_no_match() -> void:
	# Neither is player-civ → no group select even with matching group IDs.
	assert_false(
		GeometryUtils.matches_double_click_group(5, false, 5, false),
		"Two non-player creatures should not trigger group select",
	)
