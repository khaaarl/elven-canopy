## Unit tests for focus_guard.gd autoload behavior.
##
## Verifies that FocusGuard strips keyboard focus from every BaseButton
## added to the scene tree, while leaving non-button Controls unaffected.
##
## See also: focus_guard.gd for the implementation.
extends GutTest

const FocusGuardScript = preload("res://scripts/focus_guard.gd")

var _guard: Node


func before_each() -> void:
	_guard = FocusGuardScript.new()
	add_child_autofree(_guard)


func test_button_gets_focus_none() -> void:
	var btn := Button.new()
	add_child_autofree(btn)
	assert_eq(btn.focus_mode, Control.FOCUS_NONE)


func test_check_button_gets_focus_none() -> void:
	var btn := CheckButton.new()
	add_child_autofree(btn)
	assert_eq(btn.focus_mode, Control.FOCUS_NONE)


func test_non_button_control_keeps_default_focus() -> void:
	var label := Label.new()
	add_child_autofree(label)
	# Label default is FOCUS_NONE — should be unchanged by FocusGuard.
	assert_eq(label.focus_mode, Control.FOCUS_NONE)
	var line_edit := LineEdit.new()
	add_child_autofree(line_edit)
	# LineEdit default is FOCUS_ALL — should be unchanged by FocusGuard.
	assert_eq(line_edit.focus_mode, Control.FOCUS_ALL)
