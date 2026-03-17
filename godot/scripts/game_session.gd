## Autoload singleton that holds game session parameters across scene transitions.
##
## Registered as an autoload in project.godot so it persists when scenes change.
## The new-game screen (new_game_menu.gd) writes the seed here; the game scene
## (main.gd) reads it in _ready().
##
## For loading saves, main_menu.gd sets `load_save_path` before transitioning
## to main.tscn. main.gd checks this field to decide whether to start a new
## game or load from a save file.
##
## For multiplayer, the host/join menu screens set `multiplayer_mode` and
## related fields before transitioning to main.tscn. main.gd checks
## `multiplayer_mode` to decide between single-player and multiplayer startup.
##
## On startup, loads the player's persistent username from user://player.cfg.
## If no username exists (first launch), `player_name` stays empty and
## main_menu.gd shows a username prompt before enabling menu buttons.
##
## Also sets the window title — appending branch and commit info for
## debug builds (read from .build_info, written by scripts/build.sh).
##
## Also creates a temporary SimBridge to trigger the global elfcyclopedia
## HTTP server (runs on localhost, persists for the lifetime of the process).
## The URL is stored in `elfcyclopedia_url` for UI display.
##
## Also handles graceful shutdown: disables auto-quit and intercepts
## NOTIFICATION_WM_CLOSE_REQUEST (window close / Alt+F4) to call
## SimBridge.shutdown() before exiting. This tears down Rust state
## (elfcyclopedia server, relay threads, sim session) while Godot is still
## intact, avoiding segfaults from gdext cleanup ordering.
##
## See also: new_game_menu.gd, host_game_menu.gd, join_game_menu.gd, main.gd,
## elfcyclopedia_server.rs, sim_bridge.rs (shutdown method).

extends Node

const PLAYER_CONFIG_PATH := "user://player.cfg"

## Persistent player username. Loaded from user://player.cfg on startup.
## Empty string means no username has been set yet (first launch).
## Used as the player's identity in both single-player and multiplayer.
var player_name: String = ""

## The simulation seed for the current game. Written by new_game_menu.gd,
## read by main.gd. A value of -1 means "not yet set" — main.gd falls back
## to its @export default in that case.
var sim_seed: int = -1

## Tree generation profile. Written by new_game_menu.gd when the player
## configures tree shape parameters. An empty dictionary means "use Rust
## default profile". When non-empty, main.gd serializes this to JSON and
## passes it to SimBridge.init_sim_with_tree_profile_json().
var tree_profile: Dictionary = {}

## Path to a save file to load (e.g., "user://saves/my_save.json").
## When non-empty, main.gd loads this file instead of starting a new game,
## then clears the field. Set by main_menu.gd's load dialog.
var load_save_path: String = ""

## Multiplayer mode: "" (single-player), "host", or "join".
var multiplayer_mode: String = ""

## Multiplayer session config (host mode).
var mp_port: int = 7878
var mp_session_name: String = ""
var mp_password: String = ""
var mp_max_players: int = 4
var mp_ticks_per_turn: int = 50

## Multiplayer join config (join mode).
var mp_relay_address: String = ""
## Per-session join name — defaults to player_name but can be overridden
## on the join screen. Does NOT persist back to player.cfg.
var mp_player_name: String = ""
## Session ID to join. 0 for embedded relays, or the ID picked from the
## session browser for dedicated relays.
var mp_session_id: int = 0

## Elfcyclopedia server URL (set at startup, persists across scenes).
var elfcyclopedia_url: String = ""


func _ready() -> void:
	# Disable auto-quit so we can run Rust cleanup before exiting.
	# Handled centrally here (autoload) so every scene is covered.
	get_tree().set_auto_accept_quit(false)

	# Load persistent player name from user://player.cfg.
	_load_player_name()

	# Set window title. Debug builds on non-main branches get a .build_info
	# file written by scripts/build.sh containing "branch @ shorthash".
	# Release/exported builds won't have this file, so the title stays plain.
	# Deferred because Godot applies project.godot's config/name as the window
	# title after autoload _ready() runs, which would overwrite our custom title.
	_set_window_title.call_deferred()

	# Create a temporary SimBridge to trigger the global elfcyclopedia server
	# start. The server lives in a Rust static and persists after the bridge
	# is freed. This runs as soon as the autoload initializes (before the
	# main menu appears).
	var bridge := SimBridge.new()
	elfcyclopedia_url = bridge.elfcyclopedia_url()
	bridge.free()
	if not elfcyclopedia_url.is_empty():
		print("Elfcyclopedia server: %s" % elfcyclopedia_url)


func _set_window_title() -> void:
	var title := "Elven Canopy"
	if FileAccess.file_exists("res://.build_info"):
		var file := FileAccess.open("res://.build_info", FileAccess.READ)
		if file:
			var info := file.get_as_text().strip_edges()
			if not info.is_empty():
				title += "  [%s]" % info
	DisplayServer.window_set_title(title)


func _load_player_name() -> void:
	var cfg := ConfigFile.new()
	if cfg.load(PLAYER_CONFIG_PATH) == OK:
		player_name = cfg.get_value("identity", "player_name", "")


## Save the player name to user://player.cfg. Called after the first-launch
## prompt in main_menu.gd and whenever the player renames themselves.
func save_player_name() -> void:
	var cfg := ConfigFile.new()
	cfg.set_value("identity", "player_name", player_name)
	cfg.save(PLAYER_CONFIG_PATH)


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		# Shut down Rust state (elfcyclopedia server, relay threads, sim)
		# before Godot tears down the process.
		var bridge := SimBridge.new()
		bridge.shutdown()
		bridge.free()
		get_tree().quit()
