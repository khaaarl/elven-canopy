## Construction music controller — plays procedurally generated choral music
## during construction.
##
## Each frame, polls the sim bridge for newly completed music compositions
## (generated on background threads). When a composition is ready, creates
## an AudioStreamGenerator player and feeds the PCM data into it.
##
## When a build completes or is cancelled while music is still playing, the
## audio fades out over FADE_DURATION_SECS rather than cutting abruptly.
##
## Multiple constructions can play simultaneously (cacophonous overlap is
## acceptable for now).
##
## Wired up by main.gd, which creates this node and calls
## poll_compositions() each frame.

extends Node

## Sample rate must match the Rust synth module.
const SAMPLE_RATE := 44100

## Duration of the fade-out ramp when a build finishes before the music.
const FADE_DURATION_SECS := 1.5

## Reference to the SimBridge node, set by main.gd.
var bridge: Node = null

## Active audio players, keyed by composition_id (int).
## Each value is an AudioStreamPlayer that is currently playing.
var _active_players: Dictionary = {}

## PCM data for active compositions, keyed by composition_id.
## Stored so we can continue feeding the generator buffer.
var _pcm_data: Dictionary = {}

## Playback cursors (sample index), keyed by composition_id.
var _pcm_cursors: Dictionary = {}

## Compositions currently fading out, keyed by composition_id.
## Value is the remaining fade samples (counts down to 0).
var _fading: Dictionary = {}


func poll_compositions() -> void:
	if bridge == null:
		return

	# Tell the bridge to check for new Pending compositions and start
	# background generation threads.
	bridge.poll_composition_starts()

	# Check for newly completed compositions.
	var ready: PackedInt64Array = bridge.poll_ready_compositions()
	for comp_id: int in ready:
		_start_playback(comp_id)

	# Check if any playing compositions should fade out (build finished).
	if _active_players.size() > 0:
		var active_ids := PackedInt64Array()
		for comp_id: int in _active_players:
			if not _fading.has(comp_id):
				active_ids.push_back(comp_id)
		if active_ids.size() > 0:
			var done: PackedInt64Array = bridge.poll_finished_compositions(active_ids)
			for comp_id: int in done:
				_begin_fade_out(comp_id)


func _start_playback(comp_id: int) -> void:
	if _active_players.has(comp_id):
		return

	var pcm: PackedFloat32Array = bridge.get_composition_pcm(comp_id)
	if pcm.is_empty():
		return

	# Create an AudioStreamGenerator and player.
	var stream := AudioStreamGenerator.new()
	stream.mix_rate = SAMPLE_RATE
	stream.buffer_length = 0.5

	var player := AudioStreamPlayer.new()
	player.stream = stream
	player.volume_db = -6.0
	add_child(player)
	player.play()

	_active_players[comp_id] = player
	_pcm_data[comp_id] = pcm
	_pcm_cursors[comp_id] = 0


func _begin_fade_out(comp_id: int) -> void:
	if not _active_players.has(comp_id):
		return
	if _fading.has(comp_id):
		return
	_fading[comp_id] = int(SAMPLE_RATE * FADE_DURATION_SECS)


func _process(_delta: float) -> void:
	# Feed PCM data into all active generators.
	var finished: Array[int] = []
	for comp_id: int in _active_players:
		var player: AudioStreamPlayer = _active_players[comp_id]
		var playback: AudioStreamGeneratorPlayback = player.get_stream_playback()
		var pcm: PackedFloat32Array = _pcm_data[comp_id]
		var cursor: int = _pcm_cursors[comp_id]

		var is_fading: bool = _fading.has(comp_id)
		var fade_remaining: int = _fading.get(comp_id, 0)
		var fade_total: int = int(SAMPLE_RATE * FADE_DURATION_SECS)

		# Fill as many frames as the buffer can accept.
		var frames_available := playback.get_frames_available()
		var frames_to_push := mini(frames_available, pcm.size() - cursor)

		if is_fading:
			frames_to_push = mini(frames_to_push, fade_remaining)

		for i in range(frames_to_push):
			var sample: float = pcm[cursor + i]
			# Apply fade-out envelope if fading.
			if is_fading:
				var gain: float = float(fade_remaining - i) / float(fade_total)
				sample *= gain
			# AudioStreamGenerator expects stereo Vector2 frames.
			playback.push_frame(Vector2(sample, sample))

		cursor += frames_to_push
		_pcm_cursors[comp_id] = cursor

		if is_fading:
			fade_remaining -= frames_to_push
			_fading[comp_id] = fade_remaining
			if fade_remaining <= 0:
				finished.append(comp_id)
		elif cursor >= pcm.size():
			finished.append(comp_id)

	for comp_id: int in finished:
		stop_composition(comp_id)


## Stop playback for a specific composition and clean up resources.
func stop_composition(comp_id: int) -> void:
	if _active_players.has(comp_id):
		var player: AudioStreamPlayer = _active_players[comp_id]
		player.stop()
		player.queue_free()
		_active_players.erase(comp_id)
	_pcm_data.erase(comp_id)
	_pcm_cursors.erase(comp_id)
	_fading.erase(comp_id)
	if bridge != null:
		bridge.drop_composition(comp_id)


## Stop all active compositions (e.g., on game exit or load).
func stop_all() -> void:
	var ids: Array = _active_players.keys()
	for comp_id: int in ids:
		stop_composition(comp_id)
