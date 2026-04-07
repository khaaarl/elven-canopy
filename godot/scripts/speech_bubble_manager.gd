## Manages speech bubbles above creatures in the world view.
##
## Each frame, polls the SimBridge for recent "speakable" thoughts (via
## get_recent_thoughts), generates placeholder speech text, and spawns or
## updates Label3D-based speech bubbles at creature positions.
##
## Uses a pool of SpeechBubble instances — expired bubbles are hidden and
## reused. One bubble per creature at a time; new speech replaces existing.
##
## TODO(F-llm-convo-ui): TEMPORARY — the entire thought-polling and
## text-generation pipeline in this file is a placeholder shim. When LLM
## dialogue is wired in via F-llm-convo-ui, replace the thought-polling
## loop in _poll_new_thoughts() and the _thought_to_speech_text() function with
## the real LLM dialogue event source. Search for "F-llm-convo-ui" to
## find all temporary code.
##
## See also: speech_bubble.gd (individual bubble scene),
## creature_renderer.gd (creature positions and Y offsets),
## sim_bridge.rs get_recent_thoughts() (TEMPORARY bridge method).

extends Node3D

const SpeechBubble = preload("res://scripts/speech_bubble.gd")
const CreatureRenderer = preload("res://scripts/creature_renderer.gd")

## Vertical gap above the HP/MP bar area where the bubble sits.
const BUBBLE_Y_GAP := 0.18
## Debug phrases for the "Speech Test" debug button.
const _DEBUG_PHRASES: PackedStringArray = [
	"The stars are lovely tonight",
	"I wonder what's for dinner",
	"Good to see you!",
	"These branches are sturdy",
	"Hmph.",
	"What a beautiful tree",
	"My feet are tired",
	"Have you seen the sunset?",
	"Lovely evening!",
	"I heard something in the forest",
	"That was fun!",
	"The wind smells sweet today",
]

var _bridge: SimBridge
## Last tick we polled for thoughts. Advances each frame.
var _last_polled_tick: int = 0
## Active bubbles keyed by creature_id string. Values are SpeechBubble nodes.
var _active_bubbles: Dictionary = {}
## Pool of inactive (expired) SpeechBubble nodes ready for reuse.
var _pool: Array[Node3D] = []
## Render tick for creature position interpolation.
var _render_tick: float = 0.0


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	# Start polling from the current tick so we don't show old thoughts.
	_last_polled_tick = bridge.current_tick()


func set_render_tick(tick: float) -> void:
	_render_tick = tick


func _process(_delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	_poll_new_thoughts()
	_update_positions()
	_reclaim_expired()


## TODO(F-llm-convo-ui): TEMPORARY — replace this entire method with the
## real LLM dialogue event source when available.
func _poll_new_thoughts() -> void:
	var thoughts: Array = _bridge.get_recent_thoughts(_last_polled_tick)
	# Compute max tick across the batch first, so same-tick thoughts aren't
	# skipped by premature high-water-mark advancement.
	var max_tick: int = _last_polled_tick
	for thought in thoughts:
		var tick: int = thought.get("tick", 0)
		if tick >= max_tick:
			max_tick = tick + 1
	for thought in thoughts:
		var creature_id: String = thought.get("creature_id", "")
		var text: String = thought.get("text", "")

		if creature_id == "" or text == "":
			continue

		# TODO(F-llm-convo-ui): TEMPORARY — replace with LLM dialogue text.
		var speech := _thought_to_speech_text(text)
		if speech == "":
			continue

		_show_bubble(creature_id, speech)
	_last_polled_tick = max_tick


## TODO(F-llm-convo-ui): TEMPORARY — this entire function is a placeholder.
## Maps thought description strings to short spoken-aloud placeholder text.
## Remove when LLM dialogue provides real speech text.
static func _thought_to_speech_text(thought_desc: String) -> String:
	# Match on the start of the description to handle the name suffix.
	if thought_desc.begins_with("Had a pleasant chat with"):
		return "Good to see you!"
	if thought_desc.begins_with("Had an awkward exchange with"):
		return "Hmph."
	if thought_desc.begins_with("Enjoyed dinner with"):
		return "Great dinner!"
	if thought_desc.begins_with("Awkward dinner with"):
		return "..."
	if thought_desc.begins_with("Enjoyed dancing with"):
		return "What a dance!"
	if thought_desc.begins_with("Awkward dance with"):
		return "That was... something."
	if thought_desc == "Danced with a friend":
		return "That was fun!"
	if thought_desc == "Enjoyed a dinner party":
		return "Lovely evening!"
	return ""


func _show_bubble(creature_id: String, text: String) -> void:
	# If creature already has an active bubble, reuse it with new text.
	if _active_bubbles.has(creature_id):
		var bubble: Node3D = _active_bubbles[creature_id]
		bubble.show_speech(text)
		return

	# Get a bubble from the pool or create a new one.
	var bubble: Node3D
	if _pool.size() > 0:
		bubble = _pool.pop_back()
	else:
		bubble = Node3D.new()
		bubble.set_script(SpeechBubble)
		add_child(bubble)

	bubble.show_speech(text)
	_active_bubbles[creature_id] = bubble


## Update bubble positions to track their creatures.
func _update_positions() -> void:
	if _active_bubbles.is_empty():
		return

	var data: Dictionary = _bridge.get_creature_render_data(_render_tick)
	var ids: PackedStringArray = data.get("creature_ids", PackedStringArray())
	var species_arr: PackedStringArray = data.get("species", PackedStringArray())
	var positions: PackedVector3Array = data.get("positions", PackedVector3Array())

	# Build a lookup for creature positions this frame.
	var pos_lookup: Dictionary = {}
	var species_lookup: Dictionary = {}
	for i in ids.size():
		pos_lookup[ids[i]] = positions[i]
		if i < species_arr.size():
			species_lookup[ids[i]] = species_arr[i]

	# Position each active bubble above its creature.
	for creature_id: String in _active_bubbles:
		var bubble: Node3D = _active_bubbles[creature_id]
		if not pos_lookup.has(creature_id):
			# Creature not visible / dead — hide bubble.
			bubble.visible = false
			continue
		var pos: Vector3 = pos_lookup[creature_id]
		var sp: String = species_lookup.get(creature_id, "")
		var y_off: float = CreatureRenderer.SPECIES_Y_OFFSETS.get(
			sp, CreatureRenderer.DEFAULT_Y_OFFSET
		)
		bubble.visible = true
		bubble.global_position = Vector3(
			pos.x + 0.5, pos.y + y_off * 2.0 + BUBBLE_Y_GAP, pos.z + 0.5
		)


## Reclaim expired bubbles back into the pool.
func _reclaim_expired() -> void:
	var to_remove: PackedStringArray = PackedStringArray()
	for creature_id: String in _active_bubbles:
		var bubble: Node3D = _active_bubbles[creature_id]
		if bubble.is_expired():
			to_remove.append(creature_id)
			_pool.append(bubble)
	for creature_id in to_remove:
		_active_bubbles.erase(creature_id)


## Debug: show a speech bubble on every visible creature with random text.
func debug_show_all(bridge: SimBridge) -> void:
	var data: Dictionary = bridge.get_creature_render_data(_render_tick)
	var ids: PackedStringArray = data.get("creature_ids", PackedStringArray())
	for i in ids.size():
		var cid: String = ids[i]
		var phrase: String = _DEBUG_PHRASES[i % _DEBUG_PHRASES.size()]
		_show_bubble(cid, phrase)
