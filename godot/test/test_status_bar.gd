## Unit tests for status_bar.gd display formatting.
##
## Tests the pure formatting logic (speed display name, population plurals)
## without a SimBridge. The status bar is instantiated as a real node so its
## _ready() runs, then we exercise the formatting paths.
##
## See also: status_bar.gd for the implementation.
extends GutTest

const StatusBar = preload("res://scripts/status_bar.gd")

var _bar: PanelContainer


func before_each() -> void:
	_bar = StatusBar.new()
	add_child_autofree(_bar)


# -- Speed display ---------------------------------------------------------


func test_speed_normal() -> void:
	_bar.set_speed("Normal")
	var label: Label = _get_speed_label()
	assert_eq(label.text, "Speed: Normal")


func test_speed_very_fast_is_split() -> void:
	# "VeryFast" should display as "Very Fast" with a space.
	_bar.set_speed("VeryFast")
	var label: Label = _get_speed_label()
	assert_eq(label.text, "Speed: Very Fast")


func test_speed_paused() -> void:
	_bar.set_speed("Paused")
	var label: Label = _get_speed_label()
	assert_eq(label.text, "Speed: Paused")


func test_speed_fast() -> void:
	_bar.set_speed("Fast")
	var label: Label = _get_speed_label()
	assert_eq(label.text, "Speed: Fast")


## Get the speed label (4th label = index 3 among Label children in the HBox).
func _get_speed_label() -> Label:
	var hbox: HBoxContainer = _bar.get_child(0)
	var labels: Array[Label] = []
	for child in hbox.get_children():
		if child is Label:
			labels.append(child)
	return labels[3]
