## Autoload that prevents buttons from gaining keyboard focus.
##
## Godot's default behavior lets buttons receive focus, which causes
## Enter (ui_accept) to activate them unexpectedly during gameplay.
## This autoload listens for every node added to the scene tree and sets
## focus_mode to FOCUS_NONE on any BaseButton (Button, CheckButton, etc.).
##
## Registered as an autoload in project.godot.

extends Node


func _ready() -> void:
	get_tree().node_added.connect(_on_node_added)


func _on_node_added(node: Node) -> void:
	if node is BaseButton:
		node.focus_mode = Control.FOCUS_NONE
