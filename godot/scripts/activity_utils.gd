## Canonical mapping from sim task_kind strings to human-readable activity labels.
##
## Three UI files (units_panel.gd, group_info_panel.gd, tooltip_controller.gd)
## all need to display a friendly name for a creature's current task. This class
## provides the single canonical table so new task kinds only need to be added
## in one place.
class_name ActivityUtils

## Map from task_kind string to display label.  The empty-string key covers
## the case where a creature has no task (idle).
const LABELS = {
	"": "Idle",
	"GoTo": "Walking",
	"Build": "Building",
	"EatBread": "Eating",
	"EatFruit": "Eating",
	"Sleep": "Sleeping",
	"Furnish": "Furnishing",
	"Haul": "Hauling",
	"Cook": "Cooking",
	"Harvest": "Harvesting",
	"AcquireItem": "Fetching",
	"Moping": "Moping",
	"Craft": "Crafting",
	"AttackMove": "Attack Moving",
	"Attack": "Attacking",
	"Equip": "Equipping",
	"Chatting": "Chatting",
	"Dine": "Dining",
	"Graze": "Grazing",
	"Tame": "Taming",
}


## Return the human-readable label for a task_kind string.
## Falls back to p_default if the kind is not in the table.
static func get_label(task_kind: String, p_default: String = "Idle") -> String:
	return LABELS.get(task_kind, p_default)
