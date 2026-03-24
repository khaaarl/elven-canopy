## TCP server autoload for remote game control ("Puppet").
##
## Activated by the PUPPET_SERVER=<port> environment variable.  When set,
## listens on that TCP port and processes JSON-over-TCP RPC requests in
## _process(), giving handlers full scene tree access on the main thread.
## When unset, the autoload is completely inert (no server, no overhead).
##
## Wire format: 4-byte big-endian length prefix + UTF-8 JSON payload
## (matches the relay protocol pattern from elven_canopy_protocol).
## Max message size: 1 MB.
##
## Request:  {"method": "game-state", "args": ["optional", "params"]}
## Response: {"ok": true, "result": ...} or {"error": "description"}
##
## Orphan guard: if no RPC is received for PUPPET_TIMEOUT_SECS (default
## 600), the game shuts itself down to prevent abandoned headless processes.
##
## See also: puppet_helpers.gd (shared UI interaction helpers),
## test_harness_integration.gd (GUT tests using the same helpers).
extends Node

const PuppetHelpersScript := preload("res://scripts/puppet_helpers.gd")
const MAX_MESSAGE_SIZE := 1_048_576  # 1 MB
## Autoload names to skip when searching for the main scene.
## KEEP IN SYNC with the [autoload] section of project.godot.
const AUTOLOAD_NAMES := ["GameConfig", "GameSession", "FocusGuard", "PuppetServer"]
const DEFAULT_TIMEOUT_SECS := 600

var _server: TCPServer
var _peers: Array[StreamPeerTCP] = []
## Per-peer receive buffer (raw bytes accumulated between _process calls).
var _buffers: Array[PackedByteArray] = []
var _helpers  # PuppetHelpers instance (untyped to avoid class load-order issues)
var _last_rpc_time: float
var _timeout_secs: float
var _active := false
## Method table: method_name -> [min_args, callable].  Built on first dispatch.
var _method_table: Dictionary
## Named key lookup table.  Built on first use.
var _named_keys: Dictionary


func _ready() -> void:
	var port_str := OS.get_environment("PUPPET_SERVER")
	if port_str.is_empty():
		set_process(false)
		return

	var port := port_str.to_int()
	if port <= 0 or port > 65535:
		push_error("PuppetServer: invalid port '%s'" % port_str)
		set_process(false)
		return

	_server = TCPServer.new()
	var err := _server.listen(port, "127.0.0.1")
	if err != OK:
		push_error("PuppetServer: failed to listen on port %d (error %d)" % [port, err])
		set_process(false)
		return

	var timeout_str := OS.get_environment("PUPPET_TIMEOUT_SECS")
	if timeout_str.is_empty():
		_timeout_secs = DEFAULT_TIMEOUT_SECS
	else:
		_timeout_secs = timeout_str.to_float()

	_last_rpc_time = Time.get_unix_time_from_system()
	_active = true
	print("PuppetServer: listening on port %d (timeout %ds)" % [port, int(_timeout_secs)])


func _notification(what: int) -> void:
	if what == NOTIFICATION_PREDELETE:
		_shutdown()


## Clean up TCP resources to prevent segfaults during engine shutdown.
func _shutdown() -> void:
	for peer in _peers:
		peer.disconnect_from_host()
	_peers.clear()
	_buffers.clear()
	if _server:
		_server.stop()
		_server = null
	_active = false


func _process(_delta: float) -> void:
	if not _active:
		return

	# Orphan guard: shut down if no RPCs for too long.
	if _timeout_secs > 0:
		var elapsed := Time.get_unix_time_from_system() - _last_rpc_time
		if elapsed > _timeout_secs:
			print("PuppetServer: idle timeout (%.0fs), shutting down" % elapsed)
			get_tree().quit()
			return

	# Accept new connections.
	while _server.is_connection_available():
		var peer := _server.take_connection()
		peer.set_no_delay(true)
		_peers.append(peer)
		_buffers.append(PackedByteArray())

	# Process existing connections.
	var i := 0
	while i < _peers.size():
		var peer := _peers[i]
		peer.poll()
		var status := peer.get_status()
		if status == StreamPeerTCP.STATUS_NONE or status == StreamPeerTCP.STATUS_ERROR:
			_peers.remove_at(i)
			_buffers.remove_at(i)
			continue
		if status == StreamPeerTCP.STATUS_CONNECTED:
			_read_and_dispatch(i)
		i += 1


## Read available bytes, buffer them, and dispatch complete messages.
func _read_and_dispatch(idx: int) -> void:
	var peer := _peers[idx]
	var available := peer.get_available_bytes()
	if available <= 0:
		return

	var data := peer.get_data(available)
	# get_data returns [error, PackedByteArray].
	if data[0] != OK:
		return
	_buffers[idx].append_array(data[1])

	# Try to extract complete messages from the buffer.
	while true:
		var buf := _buffers[idx]
		if buf.size() < 4:
			break
		# Read 4-byte big-endian length prefix.
		var msg_len := (buf[0] << 24) | (buf[1] << 16) | (buf[2] << 8) | buf[3]
		if msg_len > MAX_MESSAGE_SIZE:
			push_warning("PuppetServer: message too large (%d bytes), dropping peer" % msg_len)
			peer.disconnect_from_host()
			_peers.remove_at(idx)
			_buffers.remove_at(idx)
			return
		if buf.size() < 4 + msg_len:
			break  # Incomplete message, wait for more data.
		# Extract the JSON payload.
		var json_bytes := buf.slice(4, 4 + msg_len)
		_buffers[idx] = buf.slice(4 + msg_len)
		var json_str := json_bytes.get_string_from_utf8()
		_handle_request(peer, json_str)


## Parse a JSON request and dispatch to the appropriate handler.
func _handle_request(peer: StreamPeerTCP, json_str: String) -> void:
	_last_rpc_time = Time.get_unix_time_from_system()

	var parsed: Variant = JSON.parse_string(json_str)
	if parsed == null or not parsed is Dictionary:
		_send_error(peer, "invalid JSON")
		return

	var method: String = parsed.get("method", "")
	var args: Array = parsed.get("args", [])

	if method.is_empty():
		_send_error(peer, "missing 'method' field")
		return

	var result = _dispatch(method, args)
	if result is Dictionary and result.has("_error"):
		_send_error(peer, result["_error"])
	else:
		_send_ok(peer, result)


func _build_method_table() -> void:
	_method_table = {
		# Observe
		"game-state": [0, func(a: Array): return _rpc_game_state()],
		"list-panels": [0, func(a: Array): return _rpc_list_panels()],
		"is-panel-visible": [1, func(a: Array): return _rpc_is_panel_visible(a[0])],
		"read-panel-text": [1, func(a: Array): return _rpc_read_panel_text(a[0])],
		"find-text": [2, func(a: Array): return _rpc_find_text(a[0], a[1])],
		"collect-text": [1, func(a: Array): return _rpc_collect_text(a[0])],
		"tree-info": [0, func(a: Array): return _rpc_tree_info()],
		"list-structures": [0, func(a: Array): return _rpc_list_structures()],
		# Act
		"click-at-world-pos": [1, func(a: Array): return _rpc_click_at_world_pos(a[0])],
		"press-key": [1, func(a: Array): return _rpc_press_key(a[0])],
		"press-button": [1, func(a: Array): return _rpc_press_button(a[0])],
		"press-button-near": [2, func(a: Array): return _rpc_press_button_near(a[0], a[1])],
		"step-ticks": [1, func(a: Array): return _rpc_step_ticks(a[0])],
		"set-sim-speed": [1, func(a: Array): return _rpc_set_sim_speed(a[0])],
		"move-camera-to": [1, func(a: Array): return _rpc_move_camera_to(a[0])],
		"quit": [0, func(a: Array): return _rpc_quit()],
		"ping": [0, func(a: Array): return "pong"],
	}


## Dispatch an RPC method call.  Returns the result value, or a dict with
## "_error" key on failure.
func _dispatch(method: String, args: Array) -> Variant:
	# Lazily initialize helpers when the main scene is available.
	var main_scene := _get_main_scene()
	if main_scene and (_helpers == null or _helpers.scene_root != main_scene):
		_helpers = PuppetHelpersScript.new(main_scene)

	if _method_table.is_empty():
		_build_method_table()

	if not _method_table.has(method):
		return {"_error": "unknown method '%s'" % method}

	var entry: Array = _method_table[method]
	var min_args: int = entry[0]
	if args.size() < min_args:
		return {"_error": "'%s' requires %d arg(s), got %d" % [method, min_args, args.size()]}

	var handler: Callable = entry[1]
	return handler.call(args)


# ---------------------------------------------------------------------------
# RPC handlers: Observe
# ---------------------------------------------------------------------------


func _rpc_game_state() -> Variant:
	var bridge := _get_bridge()
	if not bridge:
		return {"_error": "bridge not available"}
	var result := {}
	result["tick"] = bridge.current_tick()
	result["elf_count"] = bridge.elf_count()
	result["speed"] = bridge.get_sim_speed()
	var tree_info = bridge.get_home_tree_info()
	if tree_info and tree_info.has("mana_stored"):
		result["mana"] = tree_info["mana_stored"]
	if _helpers:
		var panels: Array[Dictionary] = _helpers.list_panels()
		var visible_names: Array[String] = []
		for p in panels:
			if p["visible"]:
				visible_names.append(p["name"])
		result["visible_panels"] = visible_names
	return result


func _rpc_list_panels() -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	return _helpers.list_panels()


func _rpc_is_panel_visible(panel_name: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	return _helpers.is_panel_visible(panel_name)


func _rpc_read_panel_text(node_name: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	return _helpers.read_panel_text(node_name)


func _rpc_find_text(panel_name: String, substring: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	var panel: Node = _helpers.scene_root.find_child(panel_name, true, false)
	if not panel:
		return {"_error": "panel '%s' not found" % panel_name}
	return _helpers.find_text_in_descendants(panel, substring)


func _rpc_collect_text(panel_name: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	var panel: Node = _helpers.scene_root.find_child(panel_name, true, false)
	if not panel:
		return {"_error": "panel '%s' not found" % panel_name}
	var texts: Array[Dictionary] = _helpers.collect_text(panel)
	return _variant_to_json_safe(texts)


func _rpc_tree_info() -> Variant:
	var bridge := _get_bridge()
	if not bridge:
		return {"_error": "bridge not available"}
	return _variant_to_json_safe(bridge.get_home_tree_info())


func _rpc_list_structures() -> Variant:
	var bridge := _get_bridge()
	if not bridge:
		return {"_error": "bridge not available"}
	return _variant_to_json_safe(bridge.get_structures())


# ---------------------------------------------------------------------------
# RPC handlers: Act
# ---------------------------------------------------------------------------


func _rpc_click_at_world_pos(pos_str: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	if not _helpers.has_camera():
		return {"_error": "no camera in current scene (are you on the main menu?)"}
	var parts := pos_str.split(",")
	if parts.size() != 3:
		return {"_error": "expected x,y,z format, got '%s'" % pos_str}
	var pos := Vector3(parts[0].to_float(), parts[1].to_float(), parts[2].to_float())
	_helpers.move_camera_to(pos)
	_helpers.click_at_world_pos(pos)
	return true


func _rpc_press_key(key_name: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	var keycode := _key_name_to_code(key_name)
	if keycode == KEY_NONE:
		return {"_error": "unknown key '%s'" % key_name}
	_helpers.press_key(keycode)
	return true


func _rpc_press_button(button_text: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	var btn: Button = _helpers.find_button(_helpers.scene_root, button_text)
	if not btn:
		return {"_error": "no button matching '%s' found" % button_text}
	_helpers.press_button(btn)
	return true


func _rpc_press_button_near(label_text: String, button_text: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	var btn: Button = _helpers.find_button_near_label(_helpers.scene_root, label_text, button_text)
	if not btn:
		return {"_error": "no button '%s' near label '%s' found" % [button_text, label_text]}
	_helpers.press_button(btn)
	return true


func _rpc_step_ticks(count: Variant) -> Variant:
	var bridge := _get_bridge()
	if not bridge:
		return {"_error": "bridge not available"}
	var n: int = int(count)
	if n <= 0:
		return {"_error": "tick count must be positive"}
	var speed := bridge.get_sim_speed()
	if speed != "Paused":
		return {"_error": "sim must be paused to step (current speed: %s)" % speed}
	bridge.step_exactly(n)
	return {"ticks_stepped": n, "current_tick": bridge.current_tick()}


func _rpc_set_sim_speed(speed_name: String) -> Variant:
	var bridge := _get_bridge()
	if not bridge:
		return {"_error": "bridge not available"}
	var valid := ["Paused", "Normal", "Fast", "VeryFast"]
	if speed_name not in valid:
		return {"_error": "invalid speed '%s', must be one of %s" % [speed_name, str(valid)]}
	bridge.set_sim_speed(speed_name)
	return true


func _rpc_move_camera_to(pos_str: String) -> Variant:
	if not _helpers:
		return {"_error": "scene not loaded"}
	if not _helpers.has_camera():
		return {"_error": "no camera in current scene (are you on the main menu?)"}
	var parts := pos_str.split(",")
	if parts.size() != 3:
		return {"_error": "expected x,y,z format, got '%s'" % pos_str}
	var pos := Vector3(parts[0].to_float(), parts[1].to_float(), parts[2].to_float())
	_helpers.move_camera_to(pos)
	return true


func _rpc_quit() -> Variant:
	# Respond before quitting so the client gets the ack.
	# Use call_deferred so the response is sent first.
	get_tree().quit.call_deferred()
	return true


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


## Get the current scene node (first non-autoload child of root).
## Works for both the main game scene and the main menu.
func _get_main_scene() -> Node:
	var root := get_tree().root
	for child in root.get_children():
		if child.name not in AUTOLOAD_NAMES:
			return child
	return null


## Get the SimBridge from the main scene.
func _get_bridge() -> SimBridge:
	var main := _get_main_scene()
	if not main:
		return null
	return main.get_node_or_null("SimBridge") as SimBridge


## Send a success response.
func _send_ok(peer: StreamPeerTCP, result: Variant) -> void:
	_send_response(peer, {"ok": true, "result": result})


## Send an error response.
func _send_error(peer: StreamPeerTCP, message: String) -> void:
	_send_response(peer, {"error": message})


## Encode and send a JSON response with length prefix.
func _send_response(peer: StreamPeerTCP, data: Dictionary) -> void:
	var json_str := JSON.stringify(data)
	var json_bytes := json_str.to_utf8_buffer()
	var len_bytes := PackedByteArray()
	len_bytes.resize(4)
	var msg_len := json_bytes.size()
	len_bytes[0] = (msg_len >> 24) & 0xFF
	len_bytes[1] = (msg_len >> 16) & 0xFF
	len_bytes[2] = (msg_len >> 8) & 0xFF
	len_bytes[3] = msg_len & 0xFF
	peer.put_data(len_bytes)
	peer.put_data(json_bytes)


## Convert a Godot Variant (VarDictionary/VarArray) to JSON-safe types.
## GDExtension bridge methods return Variant types that JSON.stringify may
## not handle directly.
func _variant_to_json_safe(value: Variant) -> Variant:
	if value is Dictionary:
		var result := {}
		for key in value.keys():
			result[str(key)] = _variant_to_json_safe(value[key])
		return result
	if value is Array:
		var result := []
		for item in value:
			result.append(_variant_to_json_safe(item))
		return result
	if value is Vector3:
		return {"x": value.x, "y": value.y, "z": value.z}
	if value is Vector3i:
		return {"x": value.x, "y": value.y, "z": value.z}
	return value


func _build_named_keys() -> void:
	_named_keys = {
		"ESCAPE": KEY_ESCAPE,
		"ESC": KEY_ESCAPE,
		"ENTER": KEY_ENTER,
		"RETURN": KEY_ENTER,
		"SPACE": KEY_SPACE,
		"TAB": KEY_TAB,
		"BACKSPACE": KEY_BACKSPACE,
		"DELETE": KEY_DELETE,
		"DEL": KEY_DELETE,
		"HOME": KEY_HOME,
		"END": KEY_END,
		"UP": KEY_UP,
		"DOWN": KEY_DOWN,
		"LEFT": KEY_LEFT,
		"RIGHT": KEY_RIGHT,
		"F1": KEY_F1,
		"F2": KEY_F2,
		"F3": KEY_F3,
		"F4": KEY_F4,
		"F5": KEY_F5,
		"F6": KEY_F6,
		"F7": KEY_F7,
		"F8": KEY_F8,
		"F9": KEY_F9,
		"F10": KEY_F10,
		"F11": KEY_F11,
		"F12": KEY_F12,
	}


## Map a key name string to a Godot keycode.
## Supports single letters (A–Z), digits (0–9), and common named keys.
func _key_name_to_code(key_name: String) -> int:
	var upper := key_name.to_upper()
	# Single letters.
	if upper.length() == 1 and upper[0] >= "A"[0] and upper[0] <= "Z"[0]:
		return KEY_A + (upper.unicode_at(0) - "A".unicode_at(0))
	# Number keys.
	if upper.length() == 1 and upper[0] >= "0"[0] and upper[0] <= "9"[0]:
		return KEY_0 + (upper.unicode_at(0) - "0".unicode_at(0))
	# Named keys.
	if _named_keys.is_empty():
		_build_named_keys()
	return _named_keys.get(upper, KEY_NONE)
