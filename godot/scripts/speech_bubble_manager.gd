## Manages speech bubbles above creatures in the world view.
##
## Each frame, polls the SimBridge for recent LLM-generated dialogue (via
## get_recent_dialogue), and spawns or updates Label3D-based speech bubbles
## at creature positions.
##
## Uses a pool of SpeechBubble instances — expired bubbles are hidden and
## reused. One bubble per creature at a time; new speech replaces existing.
##
## See also: speech_bubble.gd (individual bubble scene),
## creature_renderer.gd (creature positions and Y offsets),
## sim_bridge.rs get_recent_dialogue() (bridge method).

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
## Last tick we polled for dialogue. Advances each frame.
var _last_polled_tick: int = 0
## Active bubbles keyed by creature_id string. Values are SpeechBubble nodes.
var _active_bubbles: Dictionary = {}
## Pool of inactive (expired) SpeechBubble nodes ready for reuse.
var _pool: Array[Node3D] = []
## Render tick for creature position interpolation.
var _render_tick: float = 0.0


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	# Start polling from the current tick so we don't show old dialogue.
	_last_polled_tick = bridge.current_tick()


func set_render_tick(tick: float) -> void:
	_render_tick = tick


func _process(_delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	_poll_new_dialogue()
	_update_positions()
	_reclaim_expired()


## Poll the SimBridge for recent LLM dialogue and spawn speech bubbles.
func _poll_new_dialogue() -> void:
	var messages: Array = _bridge.get_recent_dialogue(_last_polled_tick)
	# Compute max tick across the batch first, so same-tick messages aren't
	# skipped by premature high-water-mark advancement.
	var max_tick: int = _last_polled_tick
	for msg in messages:
		var tick: int = msg.get("tick", 0)
		if tick >= max_tick:
			max_tick = tick + 1
	for msg in messages:
		var creature_id: String = msg.get("creature_id", "")
		var text: String = msg.get("text", "")

		if creature_id == "" or text == "":
			continue

		_show_bubble(creature_id, text)
	_last_polled_tick = max_tick


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
		var y_off: float = CreatureSprites.get_y_offset(sp)
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
