## Unit tests for settings_panel.gd — the settings overlay panel.
##
## Tests verify that the panel reads from GameConfig on open, writes
## back on save, and discards changes on cancel. Uses a temporary
## config path to avoid side effects on the real config file.
##
## See also: settings_panel.gd, game_config.gd, test_game_config.gd.

extends GutTest

const SettingsPanelScript := preload("res://scripts/settings_panel.gd")
const GameConfigScript := preload("res://scripts/game_config.gd")

const TEST_CONFIG_PATH := "user://_test_settings_panel_config.json"

var _config: Node
var _panel: ColorRect


func before_each() -> void:
	if FileAccess.file_exists(TEST_CONFIG_PATH):
		DirAccess.remove_absolute(TEST_CONFIG_PATH)
	_config = Node.new()
	_config.set_script(GameConfigScript)
	_config.config_path = TEST_CONFIG_PATH
	_config.load_config()

	_panel = ColorRect.new()
	_panel.set_script(SettingsPanelScript)
	add_child(_panel)
	_panel.open(_config)


func after_each() -> void:
	# Panel may already be freed by save_and_close / cancel_and_close.
	if is_instance_valid(_panel):
		_panel.queue_free()
	_panel = null
	if _config:
		_config.queue_free()
		_config = null
	if FileAccess.file_exists(TEST_CONFIG_PATH):
		DirAccess.remove_absolute(TEST_CONFIG_PATH)


## Panel populates controls with current GameConfig values on open.
func test_open_populates_from_config() -> void:
	_config.set_setting("player_name", "Legolas")
	_config.set_setting("start_paused_on_load", true)
	_panel.open(_config)

	assert_eq(_panel.get_player_name_text(), "Legolas")
	assert_true(_panel.get_start_paused_checked())


## Save writes edited values back to GameConfig.
func test_save_writes_to_config() -> void:
	_panel.set_player_name_text("Gimli")
	_panel.set_start_paused_checked(true)
	_panel.save_and_close()

	assert_eq(_config.get_setting("player_name"), "Gimli")
	assert_eq(_config.get_setting("start_paused_on_load"), true)


## Cancel discards edits — GameConfig retains original values.
func test_cancel_discards_changes() -> void:
	_config.set_setting("player_name", "Aragorn")
	_panel.open(_config)

	_panel.set_player_name_text("Boromir")
	_panel.set_start_paused_checked(true)
	_panel.cancel_and_close()

	assert_eq(_config.get_setting("player_name"), "Aragorn")
	assert_eq(_config.get_setting("start_paused_on_load"), false)


## Save with empty player name does not overwrite a non-empty name.
func test_save_rejects_empty_player_name() -> void:
	_config.set_setting("player_name", "Frodo")
	_panel.open(_config)

	_panel.set_player_name_text("   ")
	_panel.save_and_close()

	assert_eq(_config.get_setting("player_name"), "Frodo")


## Save strips whitespace from player name.
func test_save_strips_player_name_whitespace() -> void:
	_panel.set_player_name_text("  Elrond  ")
	_panel.save_and_close()

	assert_eq(_config.get_setting("player_name"), "Elrond")


## Panel emits closed signal on save.
func test_save_emits_closed_signal() -> void:
	var result := [false]
	_panel.closed.connect(func() -> void: result[0] = true)
	_panel.save_and_close()
	assert_true(result[0])


## Panel emits closed signal on cancel.
func test_cancel_emits_closed_signal() -> void:
	var result := [false]
	_panel.closed.connect(func() -> void: result[0] = true)
	_panel.cancel_and_close()
	assert_true(result[0])


## Save with empty name still persists the start_paused_on_load change.
func test_save_empty_name_still_writes_paused() -> void:
	_config.set_setting("player_name", "Frodo")
	_panel.open(_config)

	_panel.set_player_name_text("")
	_panel.set_start_paused_checked(true)
	_panel.save_and_close()

	assert_eq(_config.get_setting("player_name"), "Frodo")
	assert_eq(_config.get_setting("start_paused_on_load"), true)


## Save button is disabled when name is empty on open.
func test_save_button_disabled_when_name_empty() -> void:
	# Default config has empty player_name.
	assert_true(_panel._save_btn.disabled)


## Save button enables when name text becomes non-empty.
func test_save_button_enables_on_nonempty_text() -> void:
	assert_true(_panel._save_btn.disabled)
	_panel.set_player_name_text("Gandalf")
	_panel._on_name_text_changed("Gandalf")
	assert_false(_panel._save_btn.disabled)


## Panel populates draw distance from config on open.
func test_open_populates_draw_distance() -> void:
	_config.set_setting("draw_distance", 75)
	_panel.open(_config)
	assert_eq(_panel.get_draw_distance_value(), 75)


## Save writes draw distance to config.
func test_save_writes_draw_distance() -> void:
	_panel.set_draw_distance_value(100)
	_panel.save_and_close()
	assert_eq(_config.get_setting("draw_distance"), 100)


## Cancel discards draw distance changes.
func test_cancel_discards_draw_distance() -> void:
	_config.set_setting("draw_distance", 60)
	_panel.open(_config)
	_panel.set_draw_distance_value(120)
	_panel.cancel_and_close()
	assert_eq(_config.get_setting("draw_distance"), 60)


## Draw distance defaults to 50 when not set.
func test_draw_distance_default() -> void:
	assert_eq(_panel.get_draw_distance_value(), 50)


## Invalid draw distance text falls back to current config value.
func test_draw_distance_invalid_text_keeps_config() -> void:
	_config.set_setting("draw_distance", 80)
	_panel.open(_config)
	_panel._draw_distance_input.text = "not a number"
	_panel.save_and_close()
	assert_eq(_config.get_setting("draw_distance"), 80)


## Draw distance is clamped to 0–500.
func test_draw_distance_clamped() -> void:
	_panel._draw_distance_input.text = "999"
	assert_eq(_panel.get_draw_distance_value(), 500)
	_panel._draw_distance_input.text = "-10"
	assert_eq(_panel.get_draw_distance_value(), 0)


## Panel populates fog enabled from config on open.
func test_open_populates_fog_enabled() -> void:
	_config.set_setting("fog_enabled", false)
	_panel.open(_config)
	assert_false(_panel.get_fog_enabled())


## Save writes fog enabled to config.
func test_save_writes_fog_enabled() -> void:
	_panel.set_fog_enabled(false)
	_panel.save_and_close()
	assert_eq(_config.get_setting("fog_enabled"), false)


## Cancel discards fog enabled changes.
func test_cancel_discards_fog_enabled() -> void:
	_config.set_setting("fog_enabled", true)
	_panel.open(_config)
	_panel.set_fog_enabled(false)
	_panel.cancel_and_close()
	assert_eq(_config.get_setting("fog_enabled"), true)


## Panel populates fog begin from config on open.
func test_open_populates_fog_begin() -> void:
	_config.set_setting("fog_begin", 60)
	_panel.open(_config)
	assert_eq(_panel.get_fog_begin_value(), 60)


## Save writes fog begin to config.
func test_save_writes_fog_begin() -> void:
	_panel.set_fog_begin_value(30)
	_panel.save_and_close()
	assert_eq(_config.get_setting("fog_begin"), 30)


## Cancel discards fog begin changes.
func test_cancel_discards_fog_begin() -> void:
	_config.set_setting("fog_begin", 50)
	_panel.open(_config)
	_panel.set_fog_begin_value(20)
	_panel.cancel_and_close()
	assert_eq(_config.get_setting("fog_begin"), 50)


## Fog begin defaults to 40.
func test_fog_begin_default() -> void:
	assert_eq(_panel.get_fog_begin_value(), 40)


## Invalid fog begin text falls back to current config value.
func test_fog_begin_invalid_text_keeps_config() -> void:
	_config.set_setting("fog_begin", 35)
	_panel.open(_config)
	_panel._fog_begin_input.text = "not a number"
	_panel.save_and_close()
	assert_eq(_config.get_setting("fog_begin"), 35)


## Fog begin is clamped to 0–500.
func test_fog_begin_clamped() -> void:
	_panel._fog_begin_input.text = "999"
	assert_eq(_panel.get_fog_begin_value(), 500)
	_panel._fog_begin_input.text = "-10"
	assert_eq(_panel.get_fog_begin_value(), 0)


## Panel populates fog end from config on open.
func test_open_populates_fog_end() -> void:
	_config.set_setting("fog_end", 100)
	_panel.open(_config)
	assert_eq(_panel.get_fog_end_value(), 100)


## Save writes fog end to config.
func test_save_writes_fog_end() -> void:
	_panel.set_fog_end_value(90)
	_panel.save_and_close()
	assert_eq(_config.get_setting("fog_end"), 90)


## Cancel discards fog end changes.
func test_cancel_discards_fog_end() -> void:
	_config.set_setting("fog_end", 70)
	_panel.open(_config)
	_panel.set_fog_end_value(120)
	_panel.cancel_and_close()
	assert_eq(_config.get_setting("fog_end"), 70)


## Fog end defaults to 80.
func test_fog_end_default() -> void:
	assert_eq(_panel.get_fog_end_value(), 80)


## Invalid fog end text falls back to current config value.
func test_fog_end_invalid_text_keeps_config() -> void:
	_config.set_setting("fog_end", 75)
	_panel.open(_config)
	_panel._fog_end_input.text = "not a number"
	_panel.save_and_close()
	assert_eq(_config.get_setting("fog_end"), 75)


## Fog end is clamped to 0–500.
func test_fog_end_clamped() -> void:
	_panel._fog_end_input.text = "999"
	assert_eq(_panel.get_fog_end_value(), 500)
	_panel._fog_end_input.text = "-10"
	assert_eq(_panel.get_fog_end_value(), 0)


## Panel populates audio volume from config on open.
func test_open_populates_audio_volume() -> void:
	_config.set_setting("audio_volume", 60)
	_panel.open(_config)
	assert_eq(_panel.get_audio_volume_value(), 60)


## Save writes audio volume to config.
func test_save_writes_audio_volume() -> void:
	_panel.set_audio_volume_value(80)
	_panel.save_and_close()
	assert_eq(_config.get_setting("audio_volume"), 80)


## Cancel discards audio volume changes.
func test_cancel_discards_audio_volume() -> void:
	_config.set_setting("audio_volume", 40)
	_panel.open(_config)
	_panel.set_audio_volume_value(90)
	_panel.cancel_and_close()
	assert_eq(_config.get_setting("audio_volume"), 40)


## Audio volume defaults to 25.
func test_audio_volume_default() -> void:
	assert_eq(_panel.get_audio_volume_value(), 25)


## Panel populates edge scroll mode from config on open.
func test_open_populates_edge_scroll_mode() -> void:
	_config.set_setting("edge_scroll_mode", "rotate")
	_panel.open(_config)
	assert_eq(_panel.get_edge_scroll_mode(), "rotate")


## Save writes edge scroll mode to config.
func test_save_writes_edge_scroll_mode() -> void:
	_panel.set_edge_scroll_mode("rotate")
	_panel.save_and_close()
	assert_eq(_config.get_setting("edge_scroll_mode"), "rotate")


## Cancel discards edge scroll mode changes.
func test_cancel_discards_edge_scroll_mode() -> void:
	_config.set_setting("edge_scroll_mode", "pan")
	_panel.open(_config)
	_panel.set_edge_scroll_mode("rotate")
	_panel.cancel_and_close()
	assert_eq(_config.get_setting("edge_scroll_mode"), "pan")


## Edge scroll mode defaults to "pan".
func test_edge_scroll_mode_default() -> void:
	assert_eq(_panel.get_edge_scroll_mode(), "pan")


## Opening with an invalid edge scroll mode in config falls back to "off".
func test_open_invalid_edge_scroll_mode_falls_back_to_off() -> void:
	_config.set_setting("edge_scroll_mode", "turbo")
	_panel.open(_config)
	assert_eq(_panel.get_edge_scroll_mode(), "off")


## Edge scroll mode cycles through off -> pan -> rotate -> off.
func test_edge_scroll_mode_cycles() -> void:
	_panel.set_edge_scroll_mode("off")
	assert_eq(_panel.get_edge_scroll_mode(), "off")
	_panel._cycle_edge_scroll()
	assert_eq(_panel.get_edge_scroll_mode(), "pan")
	_panel._cycle_edge_scroll()
	assert_eq(_panel.get_edge_scroll_mode(), "rotate")
	_panel._cycle_edge_scroll()
	assert_eq(_panel.get_edge_scroll_mode(), "off")


## Panel runs with PROCESS_MODE_ALWAYS (required for escape menu paused tree).
func test_process_mode_always() -> void:
	assert_eq(
		_panel.process_mode,
		Node.PROCESS_MODE_ALWAYS,
		"Settings panel must process while tree is paused"
	)
