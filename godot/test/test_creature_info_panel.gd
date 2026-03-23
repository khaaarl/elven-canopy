## Unit tests for creature_info_panel.gd tab switching and stats display.
##
## Tests the tab state machine and stat label formatting without a SimBridge.
## The panel is instantiated as a real node so its _ready() runs, then we
## exercise the public methods with mock dictionaries.
##
## See also: creature_info_panel.gd for the implementation.
extends GutTest

const CreatureInfoPanel = preload("res://scripts/creature_info_panel.gd")

var _panel: PanelContainer


func before_each() -> void:
	_panel = CreatureInfoPanel.new()
	add_child_autofree(_panel)


# -- Tab switching -----------------------------------------------------------


func test_initial_tab_is_status() -> void:
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_STATUS)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_INVENTORY].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_THOUGHTS].visible)


func test_switch_to_inventory_tab() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_INVENTORY)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_INVENTORY)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_INVENTORY].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_THOUGHTS].visible)


func test_switch_to_thoughts_tab() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_THOUGHTS)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_THOUGHTS)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_INVENTORY].visible)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_THOUGHTS].visible)


func test_active_tab_button_is_disabled() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_INVENTORY)
	assert_false(_panel._tab_buttons[CreatureInfoPanel.TAB_STATUS].disabled)
	assert_true(_panel._tab_buttons[CreatureInfoPanel.TAB_INVENTORY].disabled)
	assert_false(_panel._tab_buttons[CreatureInfoPanel.TAB_THOUGHTS].disabled)


func test_hide_panel_resets_tab_to_status() -> void:
	_panel.show_creature("id-1", _minimal_info())
	_panel._switch_tab(CreatureInfoPanel.TAB_THOUGHTS)
	_panel.hide_panel()
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_STATUS)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)


# -- Follow state ------------------------------------------------------------


func test_show_creature_emits_unfollow_when_following() -> void:
	_panel.show_creature("id-1", _minimal_info())
	_panel.set_follow_state(true)
	watch_signals(_panel)
	_panel.show_creature("id-2", _minimal_info())
	assert_signal_emitted(_panel, "unfollow_requested")
	assert_false(_panel._is_following)


func test_show_creature_no_unfollow_when_not_following() -> void:
	_panel.show_creature("id-1", _minimal_info())
	watch_signals(_panel)
	_panel.show_creature("id-2", _minimal_info())
	assert_signal_not_emitted(_panel, "unfollow_requested")


# -- Name display ------------------------------------------------------------


func test_name_with_meaning() -> void:
	var info := _minimal_info()
	info["name"] = "Vaelith"
	info["name_meaning"] = "Starlight"
	_panel.show_creature("id-1", info)
	assert_eq(_panel._name_label.text, "Name: Vaelith (Starlight)")


func test_name_without_meaning() -> void:
	var info := _minimal_info()
	info["name"] = "Vaelith"
	info["name_meaning"] = ""
	_panel.show_creature("id-1", info)
	assert_eq(_panel._name_label.text, "Name: Vaelith")


func test_empty_name_falls_back_to_species() -> void:
	var info := _minimal_info()
	info["name"] = ""
	info["species"] = "Troll"
	_panel.show_creature("id-1", info)
	assert_eq(_panel._name_label.text, "Name: Troll")


# -- Stats display -----------------------------------------------------------


func test_update_stats_populates_all_labels() -> void:
	var info := {
		"stat_dex": 5,
		"stat_agi": -3,
		"stat_str": 12,
		"stat_con": 105,
		"stat_wil": -1,
		"stat_int": 14,
		"stat_per": 6,
		"stat_cha": 0,
	}
	_panel._update_stats(info)
	assert_eq(_panel._stat_labels["stat_dex"].text, "5")
	assert_eq(_panel._stat_labels["stat_agi"].text, "-3")
	assert_eq(_panel._stat_labels["stat_str"].text, "12")
	assert_eq(_panel._stat_labels["stat_con"].text, "105")
	assert_eq(_panel._stat_labels["stat_wil"].text, "-1")
	assert_eq(_panel._stat_labels["stat_int"].text, "14")
	assert_eq(_panel._stat_labels["stat_per"].text, "6")
	assert_eq(_panel._stat_labels["stat_cha"].text, "0")


func test_update_stats_missing_keys_default_to_zero() -> void:
	_panel._update_stats({})
	for key in _panel._stat_labels:
		assert_eq(_panel._stat_labels[key].text, "0", "stat %s should default to 0" % key)


# -- Helpers -----------------------------------------------------------------


## Minimal info dictionary with required fields to avoid errors in show_creature.
func _minimal_info() -> Dictionary:
	return {
		"species": "Elf",
		"name": "Test",
		"name_meaning": "",
		"hp": 100,
		"hp_max": 100,
		"mp": 0,
		"mp_max": 0,
		"x": 0,
		"y": 0,
		"z": 0,
		"has_task": false,
		"food": 100,
		"food_max": 100,
		"rest": 100,
		"rest_max": 100,
		"mood_tier": "Neutral",
		"mood_score": 0,
		"thoughts": [],
		"inventory": [],
		"incapacitated": false,
		"military_group_name": "",
		"military_group_id": -1,
	}
