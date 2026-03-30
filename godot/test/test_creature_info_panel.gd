## Unit tests for creature_info_panel.gd tab switching, stats, and skills display.
##
## Tests the tab state machine, stat label formatting, and skill label formatting
## without a SimBridge. The panel is instantiated as a real node so its _ready()
## runs, then we exercise the public methods with mock dictionaries.
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
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_SKILLS].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_INVENTORY].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_THOUGHTS].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_PATH].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_SOCIAL].visible)


func test_switch_to_skills_tab() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_SKILLS)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_SKILLS)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_SKILLS].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_INVENTORY].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_THOUGHTS].visible)


func test_switch_to_inventory_tab() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_INVENTORY)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_INVENTORY)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_SKILLS].visible)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_INVENTORY].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_THOUGHTS].visible)


func test_switch_to_thoughts_tab() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_THOUGHTS)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_THOUGHTS)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_SKILLS].visible)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_INVENTORY].visible)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_THOUGHTS].visible)


func test_active_tab_button_is_disabled() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_INVENTORY)
	assert_false(_panel._tab_buttons[CreatureInfoPanel.TAB_STATUS].disabled)
	assert_false(_panel._tab_buttons[CreatureInfoPanel.TAB_SKILLS].disabled)
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


# -- Skills display ----------------------------------------------------------


func test_update_skills_populates_all_labels() -> void:
	var info := {
		"skill_striking": 10,
		"skill_archery": 25,
		"skill_evasion": 0,
		"skill_ranging": 5,
		"skill_herbalism": 100,
		"skill_beastcraft": 0,
		"skill_cuisine": 42,
		"skill_tailoring": 0,
		"skill_woodcraft": 7,
		"skill_alchemy": 0,
		"skill_singing": 150,
		"skill_channeling": 88,
		"skill_literature": 0,
		"skill_art": 3,
		"skill_influence": 0,
		"skill_culture": 12,
		"skill_counsel": 0,
	}
	_panel._update_skills(info)
	assert_eq(_panel._skill_labels["skill_striking"].text, "10")
	assert_eq(_panel._skill_labels["skill_archery"].text, "25")
	assert_eq(_panel._skill_labels["skill_evasion"].text, "0")
	assert_eq(_panel._skill_labels["skill_ranging"].text, "5")
	assert_eq(_panel._skill_labels["skill_herbalism"].text, "100")
	assert_eq(_panel._skill_labels["skill_beastcraft"].text, "0")
	assert_eq(_panel._skill_labels["skill_cuisine"].text, "42")
	assert_eq(_panel._skill_labels["skill_tailoring"].text, "0")
	assert_eq(_panel._skill_labels["skill_woodcraft"].text, "7")
	assert_eq(_panel._skill_labels["skill_alchemy"].text, "0")
	assert_eq(_panel._skill_labels["skill_singing"].text, "150")
	assert_eq(_panel._skill_labels["skill_channeling"].text, "88")
	assert_eq(_panel._skill_labels["skill_literature"].text, "0")
	assert_eq(_panel._skill_labels["skill_art"].text, "3")
	assert_eq(_panel._skill_labels["skill_influence"].text, "0")
	assert_eq(_panel._skill_labels["skill_culture"].text, "12")
	assert_eq(_panel._skill_labels["skill_counsel"].text, "0")


func test_update_skills_missing_keys_default_to_zero() -> void:
	_panel._update_skills({})
	for key in _panel._skill_labels:
		assert_eq(_panel._skill_labels[key].text, "0", "skill %s should default to 0" % key)


func test_skill_labels_has_all_17_skills() -> void:
	assert_eq(_panel._skill_labels.size(), 17)


# -- Path tab ----------------------------------------------------------------


func test_switch_to_path_tab() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_PATH)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_PATH)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_PATH].visible)


func test_path_label_visible_for_elf() -> void:
	var info := _minimal_info()
	info["path_id"] = "Warrior"
	info["path_name"] = "Way of the Warrior"
	_panel.show_creature("elf-1", info)
	assert_true(_panel._path_label.visible)
	assert_eq(_panel._path_label.text, "Way of the Warrior")


func test_path_label_hidden_for_non_elf() -> void:
	var info := _minimal_info()
	info["species"] = "Boar"
	info["path_id"] = ""
	info["path_name"] = ""
	_panel.show_creature("boar-1", info)
	assert_false(_panel._path_label.visible)


func test_path_tab_hidden_for_non_elf() -> void:
	var info := _minimal_info()
	info["species"] = "Boar"
	info["path_id"] = ""
	info["path_name"] = ""
	_panel.show_creature("boar-1", info)
	assert_false(_panel._tab_buttons[CreatureInfoPanel.TAB_PATH].visible)


func test_path_tab_auto_switches_to_status_for_non_elf() -> void:
	# Show an elf and switch to Path tab.
	_panel.show_creature("elf-1", _minimal_info())
	_panel._switch_tab(CreatureInfoPanel.TAB_PATH)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_PATH)
	# Now switch to a non-elf — should auto-switch to Status.
	var info := _minimal_info()
	info["species"] = "Boar"
	info["path_id"] = ""
	info["path_name"] = ""
	_panel.show_creature("boar-1", info)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_STATUS)


func test_path_tab_dropdown_syncs_with_path_id() -> void:
	var info := _minimal_info()
	info["path_id"] = "Scout"
	info["path_name"] = "Way of the Scout"
	_panel.show_creature("elf-1", info)
	var scout_idx: int = _panel._path_index_to_id.find("Scout")
	assert_eq(_panel._path_tab_option.selected, scout_idx)


# -- Tame button (F-taming) --------------------------------------------------


func test_tame_button_visible_for_wild_tameable() -> void:
	var info := _minimal_info()
	info["species"] = "Capybara"
	info["is_tameable"] = true
	info["is_wild"] = true
	info["vital_status"] = "Alive"
	info["tame_designated"] = false
	_panel.show_creature("capy-1", info)
	assert_true(_panel._tame_button.visible)
	assert_string_contains(_panel._tame_button.text, "Tame")


func test_tame_button_hidden_for_untameable() -> void:
	var info := _minimal_info()
	info["species"] = "Goblin"
	info["is_tameable"] = false
	info["is_wild"] = true
	info["vital_status"] = "Alive"
	_panel.show_creature("goblin-1", info)
	assert_false(_panel._tame_button.visible)


func test_tame_button_hidden_for_tamed_creature() -> void:
	var info := _minimal_info()
	info["species"] = "Capybara"
	info["is_tameable"] = true
	info["is_wild"] = false  # already tamed (has civ_id)
	info["vital_status"] = "Alive"
	_panel.show_creature("capy-1", info)
	assert_false(_panel._tame_button.visible)


func test_tame_button_shows_checkmark_when_designated() -> void:
	var info := _minimal_info()
	info["species"] = "Capybara"
	info["is_tameable"] = true
	info["is_wild"] = true
	info["vital_status"] = "Alive"
	info["tame_designated"] = true
	_panel.show_creature("capy-1", info)
	assert_true(_panel._tame_button.visible)
	assert_string_contains(_panel._tame_button.text, "\u2713")


func test_tame_button_shows_cross_when_not_designated() -> void:
	var info := _minimal_info()
	info["species"] = "Capybara"
	info["is_tameable"] = true
	info["is_wild"] = true
	info["vital_status"] = "Alive"
	info["tame_designated"] = false
	_panel.show_creature("capy-1", info)
	assert_true(_panel._tame_button.visible)
	assert_string_contains(_panel._tame_button.text, "\u2717")


func test_tame_button_hidden_for_dead_creature() -> void:
	var info := _minimal_info()
	info["species"] = "Capybara"
	info["is_tameable"] = true
	info["is_wild"] = true
	info["vital_status"] = "Dead"
	_panel.show_creature("capy-1", info)
	assert_false(_panel._tame_button.visible)


# -- Social tab (F-social-opinions) ------------------------------------------


func test_switch_to_social_tab() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_SOCIAL)
	assert_eq(_panel._active_tab, CreatureInfoPanel.TAB_SOCIAL)
	assert_false(_panel._tab_contents[CreatureInfoPanel.TAB_STATUS].visible)
	assert_true(_panel._tab_contents[CreatureInfoPanel.TAB_SOCIAL].visible)


func test_social_tab_shows_opinions() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_SOCIAL)
	var info := _minimal_info()
	info["social_opinions"] = [
		{"target_name": "Aelindra", "kind": "Friendliness", "intensity": 20},
		{"target_name": "Thalion", "kind": "Respect", "intensity": 3},
	]
	_panel._update_social(info)
	assert_eq(_panel._social_container.get_child_count(), 2)


func test_social_tab_skips_rebuild_when_not_active() -> void:
	# Stay on Status tab (not Social).
	var info := _minimal_info()
	info["social_opinions"] = [
		{"target_name": "Aelindra", "kind": "Friendliness", "intensity": 20},
	]
	_panel._update_social(info)
	assert_eq(_panel._social_container.get_child_count(), 0)


func test_social_tab_respect_label_format() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_SOCIAL)
	var info := _minimal_info()
	info["social_opinions"] = [
		{"target_name": "Thalion", "kind": "Respect", "intensity": 5},
	]
	_panel._update_social(info)
	assert_eq(_panel._social_container.get_child_count(), 1)
	assert_eq(_panel._social_container.get_child(0).text, "Thalion: Respect 5")


func test_social_tab_empty_state() -> void:
	_panel._switch_tab(CreatureInfoPanel.TAB_SOCIAL)
	var info := _minimal_info()
	info["social_opinions"] = []
	_panel._update_social(info)
	assert_eq(_panel._social_container.get_child_count(), 1)
	assert_eq(_panel._social_container.get_child(0).text, "No opinions yet.")


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
		"path_id": "Outcast",
		"path_name": "Way of the Outcast",
	}
