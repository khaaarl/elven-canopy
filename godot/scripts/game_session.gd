## Autoload singleton that holds game session parameters across scene transitions.
##
## Registered as an autoload in project.godot so it persists when scenes change.
## The new-game screen (new_game_menu.gd) writes the seed here; the game scene
## (main.gd) reads it in _ready().
##
## For loading saves, main_menu.gd sets `load_save_path` before transitioning
## to main.tscn. main.gd checks this field to decide whether to start a new
## game or load from a save file.

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
