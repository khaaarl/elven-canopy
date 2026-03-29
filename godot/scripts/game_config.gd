## Autoload singleton for persistent game configuration.
##
## Reads and writes user://config.json — a flat JSON object with string keys.
## Created with defaults on first launch, loaded on startup, saved when
## settings change via set_setting().
##
## Known settings (with defaults):
##   player_name: String = ""         — persistent player display name
##   start_paused_on_load: bool = false — pause sim immediately after loading a save
##   draw_distance: int = 50          — chunk draw distance in voxels (0 = unlimited)
##   fog_enabled: bool = true         — depth-based atmospheric fog
##   fog_begin: int = 40              — distance (voxels) where fog starts
##   fog_end: int = 80                — distance (voxels) where fog is fully opaque
##   audio_volume: int = 25           — master audio volume (0–100 percent)
##   edge_scroll_mode: String = "pan" — edge scrolling ("off", "pan", "rotate")
##
## Unknown keys in the JSON file are preserved across load/save (forward
## compatibility — a newer version's settings won't be lost if an older
## version reads and re-saves the file).
##
## override_setting() sets an in-memory-only value that takes priority over
## the normal value and is never written to disk. Used by the test harness
## to inject config without side effects.
##
## See also: game_session.gd (session-scoped state), main_menu.gd (first-launch
## name prompt), game_config.gd tests in test/test_game_config.gd.

extends Node

const DEFAULT_CONFIG_PATH := "user://config.json"

## Defaults for all known settings. Missing keys are filled from here on load.
const DEFAULTS := {
	"player_name": "",
	"start_paused_on_load": false,
	"draw_distance": 50,
	"fog_enabled": true,
	"fog_begin": 40,
	"fog_end": 80,
	"audio_volume": 25,
	"edge_scroll_mode": "pan",
}

## File path for the config JSON. Tests can override this to use a temp path.
var config_path: String = DEFAULT_CONFIG_PATH

## Current settings — known keys plus any unknown keys from the file.
var _data: Dictionary = {}

## In-memory overrides that take priority over _data. Never saved to disk.
var _overrides: Dictionary = {}


func _ready() -> void:
	load_config()


## Load settings from disk. Missing keys are filled from DEFAULTS.
## If the file doesn't exist, _data is initialized from DEFAULTS alone.
func load_config() -> void:
	_data = DEFAULTS.duplicate(true)
	_overrides = {}

	if not FileAccess.file_exists(config_path):
		return

	var file := FileAccess.open(config_path, FileAccess.READ)
	if file == null:
		push_warning("GameConfig: could not open %s" % config_path)
		return

	var parsed: Variant = JSON.parse_string(file.get_as_text())
	file.close()

	if parsed is Dictionary:
		# Merge file data on top of defaults. Unknown keys are kept.
		# Null values for known keys are skipped (use the default instead).
		for key: String in parsed:
			if parsed[key] == null and DEFAULTS.has(key):
				continue
			_data[key] = parsed[key]
		# Remove stale keys from previous versions.
		_data.erase("fog_density")
	else:
		push_warning("GameConfig: %s is not a JSON object, using defaults" % config_path)


## Save current settings (excluding overrides) to disk.
func save_config() -> void:
	var file := FileAccess.open(config_path, FileAccess.WRITE)
	if file == null:
		push_error("GameConfig: could not write %s" % config_path)
		return
	file.store_string(JSON.stringify(_data, "\t"))
	file.close()


## Get a setting value. Returns the override if one exists, otherwise the
## stored value, otherwise null for unknown keys.
func get_setting(key: String) -> Variant:
	if _overrides.has(key):
		return _overrides[key]
	if _data.has(key):
		return _data[key]
	return null


## Set a setting value and save to disk. If the key has an override, the
## override still takes priority for reads, but the underlying value is updated.
func set_setting(key: String, value: Variant) -> void:
	_data[key] = value
	save_config()


## Set an in-memory override that takes priority over the normal value.
## Never written to disk. Used by the test harness to inject config values
## without side effects.
func override_setting(key: String, value: Variant) -> void:
	_overrides[key] = value
