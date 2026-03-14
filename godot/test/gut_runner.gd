## Headless GUT test runner entry point.
##
## This script extends SceneTree (required by Godot's --script flag) and
## launches GUT programmatically. It reads .gutconfig.json for test dirs
## and settings, runs all tests, then exits with a nonzero code on failure.
##
## Usage:
##   godot --path godot --headless --script res://test/gut_runner.gd
##
## See also: .gutconfig.json for test configuration,
## godot/test/test_*.gd for test files.
extends SceneTree

var _gut: Node
var _started := false


func _initialize() -> void:
	_gut = load("res://addons/gut/gut.gd").new()
	root.add_child(_gut)

	var config = load("res://addons/gut/gut_config.gd").new()
	config.load_options("res://.gutconfig.json")
	config.apply_options(_gut)

	_gut.end_run.connect(_on_end_run)


func _process(_delta: float) -> bool:
	# Wait one frame for the tree to be fully ready, then start tests.
	if not _started and not _gut.is_running():
		_gut.test_scripts()
		_started = true
	return false


func _on_end_run() -> void:
	var fail_count: int = _gut.get_fail_count()
	if fail_count > 0:
		quit(1)
	else:
		quit(0)
