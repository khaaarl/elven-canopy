## Unit tests for game_config.gd — the GameConfig autoload singleton.
##
## Tests cover: default values, save/load roundtrip, override_setting
## (in-memory only), unknown key preservation, and missing key backfill.
##
## These tests instantiate GameConfig directly (not as an autoload) and
## use a temporary file path to avoid interfering with any real config.
##
## See also: game_config.gd.

extends GutTest

const GameConfigScript := preload("res://scripts/game_config.gd")

const TEST_CONFIG_PATH := "user://_test_game_config.json"

var _config: Node


func before_each() -> void:
	# Remove any leftover test config file.
	if FileAccess.file_exists(TEST_CONFIG_PATH):
		DirAccess.remove_absolute(TEST_CONFIG_PATH)
	_config = Node.new()
	_config.set_script(GameConfigScript)
	_config.config_path = TEST_CONFIG_PATH
	_config.load_config()


func after_each() -> void:
	if _config:
		_config.queue_free()
		_config = null
	if FileAccess.file_exists(TEST_CONFIG_PATH):
		DirAccess.remove_absolute(TEST_CONFIG_PATH)


## When no config file exists, all settings should return their defaults.
func test_defaults_when_no_file() -> void:
	assert_eq(_config.get_setting("player_name"), "")
	assert_eq(_config.get_setting("start_paused_on_load"), false)
	assert_eq(_config.get_setting("draw_distance"), 50)
	assert_eq(_config.get_setting("fog_enabled"), true)
	assert_eq(_config.get_setting("fog_begin"), 40)
	assert_eq(_config.get_setting("fog_end"), 80)
	assert_eq(_config.get_setting("audio_volume"), 25)


## Settings can be changed and read back.
func test_set_and_get() -> void:
	_config.set_setting("player_name", "Legolas")
	assert_eq(_config.get_setting("player_name"), "Legolas")

	_config.set_setting("start_paused_on_load", true)
	assert_eq(_config.get_setting("start_paused_on_load"), true)


## After set + reload, values persist (set_setting auto-saves).
func test_save_load_roundtrip() -> void:
	_config.set_setting("player_name", "Thranduil")
	_config.set_setting("start_paused_on_load", true)

	# Create a fresh instance and load from the same path.
	var config2 := Node.new()
	config2.set_script(GameConfigScript)
	config2.config_path = TEST_CONFIG_PATH
	config2.load_config()

	assert_eq(config2.get_setting("player_name"), "Thranduil")
	assert_eq(config2.get_setting("start_paused_on_load"), true)
	config2.free()


## override_setting changes in-memory value but does NOT persist to disk.
func test_override_setting_is_memory_only() -> void:
	_config.set_setting("player_name", "Galadriel")
	_config.save_config()

	_config.override_setting("player_name", "Overridden")
	assert_eq(_config.get_setting("player_name"), "Overridden")

	# Reload from disk — override should be gone.
	var config2 := Node.new()
	config2.set_script(GameConfigScript)
	config2.config_path = TEST_CONFIG_PATH
	config2.load_config()
	assert_eq(config2.get_setting("player_name"), "Galadriel")
	config2.free()


## override_setting value survives a set_setting call (override takes priority).
func test_override_takes_priority_over_set() -> void:
	_config.override_setting("start_paused_on_load", true)
	_config.set_setting("start_paused_on_load", false)
	assert_eq(_config.get_setting("start_paused_on_load"), true)


## Unknown keys in the JSON file are preserved across load/save.
func test_unknown_keys_preserved() -> void:
	# Write a config file with an unknown key.
	var data := {"player_name": "Elrond", "future_setting": 42}
	var file := FileAccess.open(TEST_CONFIG_PATH, FileAccess.WRITE)
	file.store_string(JSON.stringify(data))
	file.close()

	_config.load_config()
	assert_eq(_config.get_setting("player_name"), "Elrond")

	# Change a known setting and save.
	_config.set_setting("start_paused_on_load", true)
	_config.save_config()

	# Re-read raw JSON and verify unknown key survived.
	var file2 := FileAccess.open(TEST_CONFIG_PATH, FileAccess.READ)
	var raw: Dictionary = JSON.parse_string(file2.get_as_text())
	file2.close()
	assert_eq(raw.get("future_setting"), 42)
	assert_eq(raw.get("start_paused_on_load"), true)
	assert_eq(raw.get("player_name"), "Elrond")


## Missing keys in an existing file are filled from defaults.
func test_missing_keys_filled_from_defaults() -> void:
	# Write a config file with only one key.
	var data := {"player_name": "Celeborn"}
	var file := FileAccess.open(TEST_CONFIG_PATH, FileAccess.WRITE)
	file.store_string(JSON.stringify(data))
	file.close()

	_config.load_config()
	assert_eq(_config.get_setting("player_name"), "Celeborn")
	assert_eq(_config.get_setting("start_paused_on_load"), false)


## Requesting an unknown setting returns null.
func test_unknown_setting_returns_null() -> void:
	assert_null(_config.get_setting("nonexistent_key"))


## save_config creates the file if it doesn't exist.
func test_save_creates_file() -> void:
	assert_false(FileAccess.file_exists(TEST_CONFIG_PATH))
	_config.set_setting("player_name", "Arwen")
	_config.save_config()
	assert_true(FileAccess.file_exists(TEST_CONFIG_PATH))


## Null values in JSON for known keys fall back to the default, not null.
func test_null_value_in_json_uses_default() -> void:
	var data := {"player_name": null, "start_paused_on_load": null}
	var file := FileAccess.open(TEST_CONFIG_PATH, FileAccess.WRITE)
	file.store_string(JSON.stringify(data))
	file.close()

	_config.load_config()
	assert_eq(_config.get_setting("player_name"), "")
	assert_eq(_config.get_setting("start_paused_on_load"), false)


## Invalid JSON (not parseable) falls back to defaults.
func test_invalid_json_falls_back_to_defaults() -> void:
	var file := FileAccess.open(TEST_CONFIG_PATH, FileAccess.WRITE)
	file.store_string("{not valid json {{{{")
	file.close()

	_config.load_config()
	assert_eq(_config.get_setting("player_name"), "")
	assert_eq(_config.get_setting("start_paused_on_load"), false)


## A JSON array (valid JSON but not an object) falls back to defaults.
func test_non_dict_json_falls_back_to_defaults() -> void:
	var file := FileAccess.open(TEST_CONFIG_PATH, FileAccess.WRITE)
	file.store_string("[1, 2, 3]")
	file.close()

	_config.load_config()
	assert_eq(_config.get_setting("player_name"), "")
	assert_eq(_config.get_setting("start_paused_on_load"), false)


## An empty file falls back to defaults.
func test_empty_file_falls_back_to_defaults() -> void:
	var file := FileAccess.open(TEST_CONFIG_PATH, FileAccess.WRITE)
	file.store_string("")
	file.close()

	_config.load_config()
	assert_eq(_config.get_setting("player_name"), "")
	assert_eq(_config.get_setting("start_paused_on_load"), false)


## load_config clears any previous overrides.
func test_load_config_clears_overrides() -> void:
	_config.override_setting("player_name", "Temporary")
	assert_eq(_config.get_setting("player_name"), "Temporary")

	_config.load_config()
	assert_eq(_config.get_setting("player_name"), "")


## set_setting auto-saves to disk without needing an explicit save_config call.
func test_set_setting_auto_saves() -> void:
	_config.set_setting("player_name", "Frodo")
	# No explicit save_config() call.

	var config2 := Node.new()
	config2.set_script(GameConfigScript)
	config2.config_path = TEST_CONFIG_PATH
	config2.load_config()
	assert_eq(config2.get_setting("player_name"), "Frodo")
	config2.free()


## Overrides are not written to disk even when save_config is called.
func test_override_not_written_to_disk() -> void:
	_config.set_setting("player_name", "OnDisk")
	_config.override_setting("player_name", "InMemory")
	_config.save_config()

	var file := FileAccess.open(TEST_CONFIG_PATH, FileAccess.READ)
	var raw: Dictionary = JSON.parse_string(file.get_as_text())
	file.close()
	assert_eq(raw.get("player_name"), "OnDisk")


## Fog settings survive a save/load roundtrip with correct types.
func test_fog_save_load_roundtrip() -> void:
	_config.set_setting("fog_enabled", false)
	_config.set_setting("fog_begin", 25)
	_config.set_setting("fog_end", 60)

	var config2 := Node.new()
	config2.set_script(GameConfigScript)
	config2.config_path = TEST_CONFIG_PATH
	config2.load_config()

	assert_eq(config2.get_setting("fog_enabled"), false)
	assert_eq(config2.get_setting("fog_begin"), 25)
	assert_eq(config2.get_setting("fog_end"), 60)
	config2.free()


## Audio volume survives a save/load roundtrip.
func test_audio_volume_save_load_roundtrip() -> void:
	_config.set_setting("audio_volume", 75)

	var config2 := Node.new()
	config2.set_script(GameConfigScript)
	config2.config_path = TEST_CONFIG_PATH
	config2.load_config()

	assert_eq(config2.get_setting("audio_volume"), 75)
	config2.free()


## Stale fog_density key from a previous version is erased on load.
func test_stale_fog_density_erased_on_load() -> void:
	var data := {"fog_density": 0.0015, "fog_enabled": true}
	var file := FileAccess.open(TEST_CONFIG_PATH, FileAccess.WRITE)
	file.store_string(JSON.stringify(data))
	file.close()

	_config.load_config()
	assert_null(_config.get_setting("fog_density"))
