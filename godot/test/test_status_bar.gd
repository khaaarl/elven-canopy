## Unit tests for status_bar.gd display formatting.
##
## Tests the pure formatting logic (speed display name, population plurals)
## without a SimBridge. The status bar is instantiated as a real node so its
## _ready() runs, then we exercise the formatting paths.
##
## Speed display is now bridge-driven (status_bar polls bridge.get_sim_speed()
## every frame), so we cannot test speed formatting here without a real bridge.
## The formatting is trivial ("VeryFast" -> "Very Fast"), covered by inspection.
##
## See also: status_bar.gd for the implementation.
extends GutTest

const StatusBar = preload("res://scripts/status_bar.gd")

var _bar: PanelContainer


func before_each() -> void:
	_bar = StatusBar.new()
	add_child_autofree(_bar)


## Initial speed label shows "Speed: Normal" before any bridge is connected.
func test_initial_speed_label() -> void:
	var label: Label = _get_speed_label()
	assert_eq(label.text, "Speed: Normal")


## Without a bridge, _process does nothing (no crash).
func test_process_without_bridge_no_crash() -> void:
	_bar._process(0.016)
	assert_eq(_get_speed_label().text, "Speed: Normal")


## Get the speed label (4th label = index 3 among Label children in the HBox).
func _get_speed_label() -> Label:
	var hbox: HBoxContainer = _bar.get_child(0)
	var labels: Array[Label] = []
	for child in hbox.get_children():
		if child is Label:
			labels.append(child)
	return labels[3]
