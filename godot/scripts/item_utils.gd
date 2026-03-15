## Utility functions for item display formatting.
##
## Provides condition_label() for computing durability condition strings
## ("(worn)" / "(damaged)") from HP values. The canonical display name is
## built in Rust (sim/mod.rs item_display_name), but this GDScript mirror
## is useful for any UI-side formatting that needs the same logic, and is
## independently unit-tested (test/test_item_utils.gd).
class_name ItemUtils


## Return the condition label for an item based on its HP ratio.
## Returns "" if the item is at full health, indestructible (max_hp <= 0),
## or above the worn threshold.
static func condition_label(
	current_hp: int, max_hp: int, worn_pct: int = 70, damaged_pct: int = 40
) -> String:
	if max_hp <= 0 or current_hp >= max_hp:
		return ""
	var ratio: int = current_hp * 100 / max_hp
	if ratio <= damaged_pct:
		return "(damaged)"
	if ratio <= worn_pct:
		return "(worn)"
	return ""
