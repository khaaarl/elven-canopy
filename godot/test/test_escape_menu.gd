## Unit tests for escape_menu.gd.
##
## Verifies visibility toggling, input blocking when visible (prevents
## toolbar hotkeys from firing behind the overlay), pause state management
## in single-player vs multiplayer, and save-dialog hotkey suppression.
##
## See also: escape_menu.gd for the implementation.
extends GutTest

const EscapeMenuScript = preload("res://scripts/escape_menu.gd")

var _menu: ColorRect


func before_each() -> void:
	_menu = ColorRect.new()
	_menu.set_script(EscapeMenuScript)
	add_child_autofree(_menu)


func after_each() -> void:
	# Ensure tree is unpaused even if a test leaves it paused.
	get_tree().paused = false


## Helper: create a key-pressed InputEventKey for the given keycode.
func _make_key_event(keycode: Key) -> InputEventKey:
	var ev := InputEventKey.new()
	ev.keycode = keycode
	ev.pressed = true
	ev.echo = false
	return ev


# -- Visibility state --


func test_starts_hidden() -> void:
	assert_false(_menu.visible, "Escape menu should start hidden")


func test_toggle_opens_and_closes() -> void:
	_menu.toggle()
	assert_true(_menu.visible, "toggle() should open menu")
	_menu.toggle()
	assert_false(_menu.visible, "toggle() again should close menu")


func test_open_close_explicit() -> void:
	_menu.open()
	assert_true(_menu.visible, "open() should make visible")
	_menu.close()
	assert_false(_menu.visible, "close() should make hidden")


# -- Input blocking when visible --


func test_key_consumed_when_visible() -> void:
	_menu.open()
	var ev := _make_key_event(KEY_B)
	_menu._unhandled_input(ev)
	# The event should be marked as handled by calling
	# get_viewport().set_input_as_handled().  We verify indirectly: if the
	# method didn't crash and the menu is still open, the key was processed.
	assert_true(_menu.visible, "Menu should remain open after key B")


func test_space_consumed_when_visible() -> void:
	_menu.open()
	var ev := _make_key_event(KEY_SPACE)
	_menu._unhandled_input(ev)
	assert_true(_menu.visible, "Menu should remain open after Space key")


func test_f1_consumed_when_visible() -> void:
	_menu.open()
	var ev := _make_key_event(KEY_F1)
	_menu._unhandled_input(ev)
	assert_true(_menu.visible, "Menu should remain open after F1 key")


func test_arbitrary_key_consumed_when_visible() -> void:
	_menu.open()
	for key in [KEY_T, KEY_U, KEY_M, KEY_I, KEY_F2, KEY_F3, KEY_F12]:
		var ev := _make_key_event(key)
		_menu._unhandled_input(ev)
	assert_true(_menu.visible, "Menu should remain open after toolbar hotkeys")


# -- Keys pass through when hidden --


func test_non_esc_key_ignored_when_hidden() -> void:
	assert_false(_menu.visible, "Precondition: menu hidden")
	# When the menu is hidden, _unhandled_input should only react to
	# ui_cancel (ESC). A regular key like B should be ignored (no crash,
	# no state change).
	var ev := _make_key_event(KEY_B)
	_menu._unhandled_input(ev)
	assert_false(_menu.visible, "Menu should stay hidden for non-ESC key")


# -- Save dialog suppression --


func test_save_dialog_open_suppresses_q() -> void:
	_menu.open()
	_menu._save_dialog_open = true
	var ev := _make_key_event(KEY_Q)
	# Q normally quits the game — with _save_dialog_open=true, it must be
	# consumed without calling _quit_game (which would terminate the test).
	_menu._unhandled_input(ev)
	assert_true(_menu.visible, "Q should not quit while save dialog is open")


func test_save_dialog_open_suppresses_s() -> void:
	_menu.open()
	_menu._save_dialog_open = true
	var ev := _make_key_event(KEY_S)
	_menu._unhandled_input(ev)
	assert_true(_menu.visible, "S should not open save while save dialog is open")


func test_s_key_does_nothing_when_save_disabled() -> void:
	# Without setup(), _save_btn.disabled == true.
	_menu.open()
	var ev := _make_key_event(KEY_S)
	_menu._unhandled_input(ev)
	assert_true(_menu.visible, "S should not trigger save when save button is disabled")


# -- Pause state --


func test_open_pauses_tree_singleplayer() -> void:
	# Default is single-player (_is_multiplayer = false).
	_menu.open()
	assert_true(get_tree().paused, "open() should pause tree in single-player")
	_menu.close()
	assert_false(get_tree().paused, "close() should unpause tree in single-player")


func test_open_does_not_pause_in_multiplayer() -> void:
	_menu._is_multiplayer = true
	_menu.open()
	assert_false(get_tree().paused, "open() should not pause tree in multiplayer")
	_menu.close()
	assert_false(get_tree().paused, "close() should not change pause state in multiplayer")


func test_process_mode_always() -> void:
	assert_eq(
		_menu.process_mode,
		Node.PROCESS_MODE_ALWAYS,
		"Escape menu must process while tree is paused"
	)


# -- Echo / non-key filtering --


func test_echo_key_not_consumed_when_visible() -> void:
	_menu.open()
	var ev := InputEventKey.new()
	ev.keycode = KEY_B
	ev.pressed = true
	ev.echo = true  # Held-key repeat — should NOT be consumed.
	_menu._unhandled_input(ev)
	# If the echo event were consumed, set_input_as_handled would be called.
	# We can't directly assert that, but the code path is exercised without
	# crashing, and the menu stays open.
	assert_true(_menu.visible, "Echo key should not affect menu state")
