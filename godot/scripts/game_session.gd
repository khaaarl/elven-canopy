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
## See also: new_game_menu.gd, host_game_menu.gd, join_game_menu.gd, main.gd.

extends Node

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
var mp_player_name: String = ""
