## Pure selection-modifier helpers shared by selection_controller.gd.
##
## Extracted so the click/box-select modifier logic (plain click, Shift to
## toggle, Alt to remove) can be unit-tested without Godot scene dependencies
## (SimBridge, Camera3D, etc.).
##
## See also: selection_controller.gd (uses these helpers), geometry_utils.gd
## (ray-cast math helpers).
class_name SelectionUtils


## Apply click-selection modifiers to determine the new selection state.
##
## Returns a dictionary with two keys:
##   "ids" — the new Array of selected creature IDs
##   "changed" — true if the selection actually changed
##
## Rules:
##   - Alt held: remove clicked_id from current selection (remove-only).
##   - Shift held: toggle clicked_id (add if absent, remove if present).
##   - Neither: replace selection with just the clicked creature.
static func apply_click_modifier(
	current_ids: Array, clicked_id: String, shift: bool, alt: bool
) -> Dictionary:
	if alt:
		var idx := current_ids.find(clicked_id)
		if idx >= 0:
			var result := current_ids.duplicate()
			result.remove_at(idx)
			return {"ids": result, "changed": true}
		return {"ids": current_ids, "changed": false}

	if shift:
		var idx := current_ids.find(clicked_id)
		if idx >= 0:
			var result := current_ids.duplicate()
			result.remove_at(idx)
			return {"ids": result, "changed": true}
		var result := current_ids.duplicate()
		result.append(clicked_id)
		return {"ids": result, "changed": true}

	return {"ids": [clicked_id], "changed": true}


## Apply box-selection modifiers to determine the new selection state.
##
## Returns a dictionary with two keys:
##   "ids" — the new Array of selected creature IDs
##   "changed" — true if the selection actually changed
##
## Rules:
##   - Alt held: remove all box_ids from current selection.
##   - Shift held: merge box_ids into current selection (no duplicates).
##   - Neither: replace selection with box_ids.
static func apply_box_modifier(
	current_ids: Array, box_ids: Array, shift: bool, alt: bool
) -> Dictionary:
	if alt:
		var result := current_ids.duplicate()
		for cid in box_ids:
			var idx := result.find(cid)
			if idx >= 0:
				result.remove_at(idx)
		return {"ids": result, "changed": result.size() != current_ids.size()}

	if shift:
		var result := current_ids.duplicate()
		for cid in box_ids:
			if result.find(cid) < 0:
				result.append(cid)
		return {"ids": result, "changed": result.size() != current_ids.size()}

	return {"ids": box_ids, "changed": true}
