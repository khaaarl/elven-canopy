## Join Game screen — connect to a relay, browse sessions, and join one.
##
## Two-phase flow:
## 1. Enter server address and player name, click "Connect".
## 2. SessionBrowser connects to the relay and lists available sessions.
##    - If exactly one session exists, auto-select it (embedded relay shortcut).
##    - Otherwise, display a session list for the player to pick from.
## 3. If the chosen session requires a password, prompt for it.
## 4. Store connection info in GameSession, transition to main.tscn.
##
## See also: session_browser.rs (SessionBrowser class), game_config.gd,
## game_session.gd, multiplayer_menu.gd, main.gd.

extends Control

var _address_input: LineEdit
var _name_input: LineEdit
var _password_input: LineEdit
var _connect_btn: Button
var _back_btn: Button
var _status_label: Label
var _session_list: ItemList
var _join_btn: Button
var _refresh_btn: Button
var _password_container: VBoxContainer
var _vbox: VBoxContainer
var _browser: SessionBrowser
## Phase: "connect" (entering address) or "browse" (viewing sessions).
var _phase: String = "connect"
## Cached session data from the last list_sessions() call.
var _sessions: Array = []


func _ready() -> void:
	# Full-rect dark background.
	var bg := ColorRect.new()
	bg.color = Color(0.12, 0.14, 0.10, 1.0)
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg)

	var center := CenterContainer.new()
	center.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(center)

	_vbox = VBoxContainer.new()
	_vbox.add_theme_constant_override("separation", 12)
	center.add_child(_vbox)

	# Header.
	var header := Label.new()
	header.text = "Join Game"
	header.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	header.add_theme_font_size_override("font_size", 36)
	_vbox.add_child(header)

	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 12)
	_vbox.add_child(spacer)

	# Address input.
	_add_label(_vbox, "Server Address (host:port):")
	_address_input = _add_line_edit(_vbox, "127.0.0.1:7878")
	_address_input.text = "127.0.0.1:7878"
	_address_input.call_deferred("grab_focus")

	# Player name input — defaults to persistent player name from config.json.
	_add_label(_vbox, "Player Name:")
	var stored_name: String = GameConfig.get_setting("player_name")
	var default_name := stored_name if not stored_name.is_empty() else "Player"
	_name_input = _add_line_edit(_vbox, default_name)
	_name_input.text = default_name

	# Status label (hidden initially).
	_status_label = Label.new()
	_status_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_status_label.add_theme_color_override("font_color", Color(0.9, 0.7, 0.3))
	_status_label.visible = false
	_vbox.add_child(_status_label)

	# Session list (hidden until connected).
	_session_list = ItemList.new()
	_session_list.custom_minimum_size = Vector2(500, 200)
	_session_list.visible = false
	_session_list.item_selected.connect(_on_session_selected)
	_vbox.add_child(_session_list)

	# Password container (hidden until a password-protected session is selected).
	_password_container = VBoxContainer.new()
	_password_container.visible = false
	_vbox.add_child(_password_container)
	_add_label(_password_container, "Session Password:")
	_password_input = _add_line_edit(_password_container, "")

	var spacer2 := Control.new()
	spacer2.custom_minimum_size = Vector2(0, 16)
	_vbox.add_child(spacer2)

	# Button row.
	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 16)
	hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	_vbox.add_child(hbox)

	_back_btn = Button.new()
	_back_btn.text = "Back"
	_back_btn.custom_minimum_size = Vector2(120, 45)
	_back_btn.pressed.connect(_on_back_pressed)
	hbox.add_child(_back_btn)

	_refresh_btn = Button.new()
	_refresh_btn.text = "Refresh"
	_refresh_btn.custom_minimum_size = Vector2(120, 45)
	_refresh_btn.pressed.connect(_on_refresh_pressed)
	_refresh_btn.visible = false
	hbox.add_child(_refresh_btn)

	_connect_btn = Button.new()
	_connect_btn.text = "Connect"
	_connect_btn.custom_minimum_size = Vector2(180, 45)
	_connect_btn.pressed.connect(_on_connect_pressed)
	hbox.add_child(_connect_btn)

	_join_btn = Button.new()
	_join_btn.text = "Join Session"
	_join_btn.custom_minimum_size = Vector2(180, 45)
	_join_btn.pressed.connect(_on_join_pressed)
	_join_btn.visible = false
	hbox.add_child(_join_btn)


func _add_label(parent: Control, text: String) -> void:
	var lbl := Label.new()
	lbl.text = text
	parent.add_child(lbl)


func _add_line_edit(parent: Control, placeholder: String) -> LineEdit:
	var edit := LineEdit.new()
	edit.placeholder_text = placeholder
	edit.custom_minimum_size = Vector2(300, 36)
	parent.add_child(edit)
	return edit


func _on_back_pressed() -> void:
	if _phase == "browse":
		# Go back to connect phase.
		_switch_to_connect_phase()
		return
	get_tree().change_scene_to_file("res://scenes/multiplayer_menu.tscn")


func _switch_to_connect_phase() -> void:
	_phase = "connect"
	_address_input.editable = true
	_name_input.editable = true
	_session_list.visible = false
	_connect_btn.visible = true
	_join_btn.visible = false
	_refresh_btn.visible = false
	_password_container.visible = false
	_status_label.visible = false
	_sessions = []
	if _browser:
		_browser.disconnect_relay()
		_browser = null


func _on_connect_pressed() -> void:
	var address := _address_input.text.strip_edges()
	if address.is_empty():
		_show_status("Enter a server address.")
		return

	var player_name := _name_input.text.strip_edges()
	if player_name.is_empty():
		_show_status("Enter a player name.")
		return

	_show_status("Connecting...")
	_connect_btn.disabled = true

	# Use call_deferred so the status label renders before blocking connect.
	call_deferred("_do_connect", address)


func _do_connect(address: String) -> void:
	_browser = SessionBrowser.new()
	if not _browser.connect_to_relay(address):
		_show_status("Failed to connect to %s" % address)
		_connect_btn.disabled = false
		_browser = null
		return

	_fetch_and_show_sessions()


func _fetch_and_show_sessions() -> void:
	var sessions := _browser.list_sessions()
	_sessions = sessions
	_connect_btn.disabled = false

	if sessions.is_empty():
		_show_status("No sessions available on this server.")
		_session_list.visible = false
		_join_btn.visible = false
		_refresh_btn.visible = true
		_connect_btn.visible = false
		_phase = "browse"
		return

	# If exactly one session, auto-select it (embedded relay shortcut).
	if sessions.size() == 1:
		var s: Dictionary = sessions[0]
		_select_session(s)
		return

	# Show session list.
	_switch_to_browse_phase(sessions)


func _switch_to_browse_phase(sessions: Array) -> void:
	_phase = "browse"
	_address_input.editable = false
	_name_input.editable = false
	_connect_btn.visible = false
	_join_btn.visible = true
	_join_btn.disabled = true
	_refresh_btn.visible = true
	_session_list.visible = true
	_status_label.visible = false
	_password_container.visible = false

	_populate_session_list(sessions)


func _populate_session_list(sessions: Array) -> void:
	_session_list.clear()
	for s: Dictionary in sessions:
		var name_str: String = s.get("name", "???")
		var players: int = s.get("player_count", 0)
		var max_p: int = s.get("max_players", 0)
		var has_pw: bool = s.get("has_password", false)
		var started: bool = s.get("game_started", false)
		var status_str := "In Lobby"
		if started:
			status_str = "In Game"
		var pw_str := ""
		if has_pw:
			pw_str = " [Password]"
		var label := (
			"%s  —  %d/%d players  —  %s%s" % [name_str, players, max_p, status_str, pw_str]
		)
		_session_list.add_item(label)


func _on_session_selected(index: int) -> void:
	if index < 0 or index >= _sessions.size():
		return
	_join_btn.disabled = false
	var s: Dictionary = _sessions[index]
	var has_pw: bool = s.get("has_password", false)
	_password_container.visible = has_pw
	if not has_pw:
		_password_input.text = ""


func _on_refresh_pressed() -> void:
	if _browser and _browser.is_relay_connected():
		_show_status("Refreshing...")
		call_deferred("_do_refresh")
	else:
		_switch_to_connect_phase()


func _do_refresh() -> void:
	_fetch_and_show_sessions()


func _on_join_pressed() -> void:
	var selected := _session_list.get_selected_items()
	if selected.is_empty():
		_show_status("Select a session to join.")
		return
	var index: int = selected[0]
	if index >= _sessions.size():
		return
	var s: Dictionary = _sessions[index]
	_select_session(s)


func _select_session(s: Dictionary) -> void:
	var has_pw: bool = s.get("has_password", false)
	var session_id: int = s.get("session_id", 0)

	# If password required and not yet entered, show the password field.
	if has_pw and _password_input.text.strip_edges().is_empty():
		_password_container.visible = true
		_phase = "browse"
		_address_input.editable = false
		_name_input.editable = false
		_connect_btn.visible = false
		_join_btn.visible = true
		_join_btn.disabled = false
		_refresh_btn.visible = true
		_session_list.visible = false
		_show_status("Enter the session password to join '%s'." % s.get("name", ""))
		_password_input.call_deferred("grab_focus")
		# Store pending session for the join button.
		_sessions = [s]
		return

	# We have everything — store in GameSession and transition.
	GameSession.multiplayer_mode = "join"
	GameSession.mp_relay_address = _address_input.text.strip_edges()
	GameSession.mp_player_name = _name_input.text.strip_edges()
	GameSession.mp_password = _password_input.text
	GameSession.mp_session_id = session_id

	# Clean up browser connection before transitioning.
	if _browser:
		_browser.disconnect_relay()
		_browser = null

	get_tree().change_scene_to_file("res://scenes/main.tscn")


func _show_status(text: String) -> void:
	_status_label.text = text
	_status_label.visible = true
