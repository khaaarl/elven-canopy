## Unit tests for notification_history_panel.gd.
##
## Tests the entry management, show/hide logic, and signal emission of the
## notification history panel. Does not require a SimBridge.
##
## See also: notification_history_panel.gd for the implementation.
extends GutTest

const HistoryPanel = preload("res://scripts/notification_history_panel.gd")

var _panel: PanelContainer


func before_each() -> void:
	_panel = PanelContainer.new()
	_panel.set_script(HistoryPanel)
	add_child_autofree(_panel)


# -- Visibility -------------------------------------------------------------


func test_starts_hidden() -> void:
	assert_false(_panel.visible)


func test_show_panel_makes_visible() -> void:
	_panel.show_panel()
	assert_true(_panel.visible)


func test_hide_panel_makes_invisible() -> void:
	_panel.show_panel()
	_panel.hide_panel()
	assert_false(_panel.visible)


func test_toggle_opens_then_closes() -> void:
	_panel.toggle()
	assert_true(_panel.visible)
	_panel.toggle()
	assert_false(_panel.visible)


# -- Signals ----------------------------------------------------------------


func test_panel_opened_signal() -> void:
	watch_signals(_panel)
	_panel.show_panel()
	assert_signal_emitted(_panel, "panel_opened")


func test_panel_closed_signal_on_hide() -> void:
	_panel.show_panel()
	watch_signals(_panel)
	_panel.hide_panel()
	assert_signal_emitted(_panel, "panel_closed")


# -- Entry management -------------------------------------------------------


func test_add_entries_creates_children() -> void:
	var entries := [
		{"id": 0, "tick": 100, "message": "Hello"},
		{"id": 1, "tick": 200, "message": "World"},
	]
	_panel.add_entries(entries)
	# The entry container should have the empty label + 2 entry panels.
	var container: VBoxContainer = _get_entry_container()
	# 1 empty label (hidden) + 2 entries = 3 children.
	assert_eq(container.get_child_count(), 3)


func test_add_entries_hides_empty_label() -> void:
	var entries := [{"id": 0, "tick": 100, "message": "Test"}]
	_panel.add_entries(entries)
	var container: VBoxContainer = _get_entry_container()
	# Empty label drifts to the end as entries are inserted at index 0.
	var empty_label: Label = container.get_child(container.get_child_count() - 1)
	assert_false(empty_label.visible)


func test_newest_entry_is_first_child() -> void:
	_panel.add_entries([{"id": 0, "tick": 100, "message": "First"}])
	_panel.add_entries([{"id": 1, "tick": 200, "message": "Second"}])
	var container: VBoxContainer = _get_entry_container()
	# Child 0 should be the newest entry ("Second"), child 1 the older ("First").
	var newest_panel: PanelContainer = container.get_child(0)
	var vbox: VBoxContainer = newest_panel.get_child(0)
	var msg_label: Label = vbox.get_child(1)
	assert_eq(msg_label.text, "Second")


func test_add_entries_with_empty_array() -> void:
	_panel.add_entries([])
	var container: VBoxContainer = _get_entry_container()
	# Should still only have the empty label, which stays visible.
	assert_eq(container.get_child_count(), 1)
	var empty_label: Label = container.get_child(0)
	assert_true(empty_label.visible)


func test_add_entries_with_missing_keys() -> void:
	# Entries missing tick and message should not crash (defaults used).
	_panel.add_entries([{"id": 5}])
	var container: VBoxContainer = _get_entry_container()
	assert_eq(container.get_child_count(), 2)  # 1 entry + empty label
	var entry_panel: PanelContainer = container.get_child(0)
	var vbox: VBoxContainer = entry_panel.get_child(0)
	var tick_label: Label = vbox.get_child(0)
	var msg_label: Label = vbox.get_child(1)
	assert_eq(tick_label.text, "Tick 0")
	assert_eq(msg_label.text, "")


func test_toggle_emits_panel_opened_signal() -> void:
	watch_signals(_panel)
	_panel.toggle()  # hidden -> visible
	assert_signal_emitted(_panel, "panel_opened")


func test_toggle_emits_panel_closed_signal() -> void:
	_panel.show_panel()
	watch_signals(_panel)
	_panel.toggle()  # visible -> hidden
	assert_signal_emitted(_panel, "panel_closed")


func test_within_batch_ordering() -> void:
	# A single batch [A, B, C] should display C at top, then B, then A.
	# Each entry is inserted at index 0, so the last-processed entry is on top.
	var batch: Array = [
		{"id": 0, "tick": 10, "message": "A"},
		{"id": 1, "tick": 20, "message": "B"},
		{"id": 2, "tick": 30, "message": "C"},
	]
	_panel.add_entries(batch)
	var container: VBoxContainer = _get_entry_container()
	# Children: C (0), B (1), A (2), empty_label (3)
	var c_label: Label = container.get_child(0).get_child(0).get_child(1)
	var b_label: Label = container.get_child(1).get_child(0).get_child(1)
	var a_label: Label = container.get_child(2).get_child(0).get_child(1)
	assert_eq(c_label.text, "C")
	assert_eq(b_label.text, "B")
	assert_eq(a_label.text, "A")


func test_empty_label_visible_when_no_entries() -> void:
	var container: VBoxContainer = _get_entry_container()
	var empty_label: Label = container.get_child(0)
	assert_true(empty_label.visible)


func test_eviction_removes_oldest_entries() -> void:
	# MAX_ENTRIES is 500. Add 505 entries and verify only 500 remain.
	var batch: Array = []
	for i in range(505):
		batch.append({"id": i, "tick": i * 10, "message": "Msg %d" % i})
	_panel.add_entries(batch)
	# Internal array should be capped at MAX_ENTRIES.
	assert_eq(_panel._entries.size(), 500)
	# UI should have MAX_ENTRIES entry panels + 1 hidden empty label.
	var container: VBoxContainer = _get_entry_container()
	assert_eq(container.get_child_count(), 501)
	# Oldest entries (0-4) should be evicted; newest (504) should be at top.
	var newest: PanelContainer = container.get_child(0)
	var msg_label: Label = newest.get_child(0).get_child(1)
	assert_eq(msg_label.text, "Msg 504")
	# Oldest surviving entry (5) should be second-to-last, before empty_label.
	var oldest: PanelContainer = container.get_child(container.get_child_count() - 2)
	var oldest_msg: Label = oldest.get_child(0).get_child(1)
	assert_eq(oldest_msg.text, "Msg 5")


# -- Helpers ----------------------------------------------------------------


## Navigate the node tree to find the entry container (VBoxContainer inside
## the ScrollContainer).
func _get_entry_container() -> VBoxContainer:
	# Structure: PanelContainer > VBoxContainer > [header, sep, ScrollContainer]
	var root_vbox: VBoxContainer = _panel.get_child(0)
	var scroll: ScrollContainer = root_vbox.get_child(2)  # header=0, sep=1, scroll=2
	return scroll.get_child(0)
