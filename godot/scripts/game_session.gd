## Autoload singleton that holds game session parameters across scene transitions.
##
## Registered as an autoload in project.godot so it persists when scenes change.
## The new-game screen (new_game_menu.gd) writes the seed here; the game scene
## (main.gd) reads it in _ready().
##
## Future world-generation settings (world size, difficulty, biome, etc.) will
## be added as additional fields here.

extends Node

## The simulation seed for the current game. Written by new_game_menu.gd,
## read by main.gd. A value of -1 means "not yet set" — main.gd falls back
## to its @export default in that case.
var sim_seed: int = -1

## Tree generation profile. Written by new_game_menu.gd when the player
## configures tree shape parameters. An empty dictionary means "use default"
## (Fantasy Mega). When non-empty, main.gd serializes this to JSON and passes
## it to SimBridge.init_sim_with_tree_profile_json().
var tree_profile: Dictionary = {}
