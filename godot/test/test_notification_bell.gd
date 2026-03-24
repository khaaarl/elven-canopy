## Unit tests for notification_bell.gd.
##
## Tests the unread count state management and signal emission. The
## procedural drawing is not tested (requires visual inspection), but the
## badge count logic and button interaction are covered.
##
## See also: notification_bell.gd for the implementation.
extends GutTest

const NotificationBell = preload("res://scripts/notification_bell.gd")

var _bell: Button


func before_each() -> void:
	_bell = Button.new()
	_bell.set_script(NotificationBell)
	add_child_autofree(_bell)


# -- Unread count state -----------------------------------------------------


func test_initial_unread_count_is_zero() -> void:
	assert_eq(_bell.get_unread_count(), 0)


func test_set_unread_count() -> void:
	_bell.set_unread_count(5)
	assert_eq(_bell.get_unread_count(), 5)


func test_set_unread_count_to_zero_clears() -> void:
	_bell.set_unread_count(3)
	_bell.set_unread_count(0)
	assert_eq(_bell.get_unread_count(), 0)


func test_set_same_count_is_idempotent() -> void:
	_bell.set_unread_count(7)
	_bell.set_unread_count(7)
	assert_eq(_bell.get_unread_count(), 7)


# -- Signal emission --------------------------------------------------------


func test_bell_pressed_signal_emitted() -> void:
	watch_signals(_bell)
	_bell.emit_signal("pressed")
	assert_signal_emitted(_bell, "bell_pressed")
