## Pre-game lobby overlay for multiplayer.
##
## Full-screen semi-transparent overlay shown in main.tscn while waiting for
## the host to start the game. Displays session info, a player list (populated
## from multiplayer events), and host-only "Start Game" / "Disconnect" buttons.
##
## Polls bridge.poll_network() each frame in _process(). When
## bridge.is_game_started() becomes true, hides itself and emits `game_started`
## so main.gd can proceed with renderer setup.
##
## Lives on CanvasLayer 3 inside main.tscn to overlay everything else.
##
## See also: main.gd (creates and wires this overlay), sim_bridge.rs
## (host_game/join_game/start_multiplayer_game/poll_network).

extends ColorRect

signal game_started

var _bridge: SimBridge
var _status_label: Label
var _player_list_label: Label
var _start_btn: Button
var _players: Array = []  # Array of {id: int, name: String}


func _ready() -> void:
	process_mode = Node.PROCESS_MODE_ALWAYS
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	color = Color(0.12, 0.14, 0.10, 0.9)

	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 16)
	center.add_child(vbox)

	# Header.
	var header := Label.new()
	header.text = "Multiplayer Lobby"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 32)
	vbox.add_child(header)

	# Status label.
	_status_label = Label.new()
	_status_label.text = "Connecting..."
	_status_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	vbox.add_child(_status_label)

	# Player list.
	var players_header := Label.new()
	players_header.text = "Players:"
	vbox.add_child(players_header)

	_player_list_label = Label.new()
	_player_list_label.text = "(waiting)"
	vbox.add_child(_player_list_label)

	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 16)
	vbox.add_child(spacer)

	# Start Game button (host only, hidden for joiners).
	_start_btn = Button.new()
	_start_btn.text = "Start Game"
	_start_btn.custom_minimum_size = Vector2(200, 50)
	_start_btn.pressed.connect(_on_start_pressed)
	vbox.add_child(_start_btn)

	# Disconnect button.
	var disconnect_btn := Button.new()
	disconnect_btn.text = "Disconnect"
	disconnect_btn.custom_minimum_size = Vector2(200, 50)
	disconnect_btn.pressed.connect(_on_disconnect_pressed)
	vbox.add_child(disconnect_btn)


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_start_btn.visible = bridge.is_host()
	if bridge.is_host():
		_status_label.text = "Hosting — waiting for players..."
	else:
		_status_label.text = "Connected — waiting for host to start..."


func _process(_delta: float) -> void:
	if _bridge == null:
		return

	# Poll network for lobby events.
	_bridge.poll_network()

	# Process events (player join/leave).
	var events := _bridge.poll_mp_events()
	for event_json in events:
		var parsed = JSON.parse_string(event_json)
		if parsed == null:
			continue
		var event_type: String = parsed.get("type", "")
		if event_type == "player_joined":
			_players.append({"id": parsed["id"], "name": parsed["name"]})
			_refresh_player_list()
		elif event_type == "player_left":
			var remove_id: int = parsed["id"]
			_players = _players.filter(func(p): return p["id"] != remove_id)
			_refresh_player_list()
		elif event_type == "game_start":
			visible = false
			set_process(false)
			game_started.emit()
			return

	# Check if game started (in case we missed the event).
	if _bridge.is_game_started():
		visible = false
		set_process(false)
		game_started.emit()


func _refresh_player_list() -> void:
	if _players.is_empty():
		_player_list_label.text = "(no other players)"
	else:
		var lines := PackedStringArray()
		for p in _players:
			lines.append("  %s (id %d)" % [p["name"], p["id"]])
		_player_list_label.text = "\n".join(lines)


func _on_start_pressed() -> void:
	if _bridge == null:
		return
	# Build config JSON from GameSession's tree profile.
	var config_json: String
	if not GameSession.tree_profile.is_empty():
		config_json = JSON.stringify(GameSession.tree_profile)
	else:
		config_json = "{}"
	_bridge.start_multiplayer_game(GameSession.sim_seed, config_json)


func _on_disconnect_pressed() -> void:
	if _bridge:
		_bridge.disconnect_multiplayer()
	GameSession.multiplayer_mode = ""
	get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
