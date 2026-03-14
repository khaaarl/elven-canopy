## Unit tests for the roof-click-select logic in selection_controller.gd.
##
## The actual roof filtering predicate lives in GeometryUtils.is_shielded_by_roof()
## so it can be tested without scene dependencies (SimBridge, Camera3D).
##
## See also: selection_controller.gd, tooltip_controller.gd (both use the
## same predicate), geometry_utils.gd for the implementation.
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
