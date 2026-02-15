## Toolbar UI for spawning creatures and placing tasks.
##
## Builds a horizontal row of buttons: "Spawn Elf [1]", "Spawn Capybara [2]",
## and "Summon [3]". Emits `spawn_requested(species_name)` for creature spawns
## and `action_requested(action_name)` for task placement.
##
## Created programmatically by main.gd and parented under a CanvasLayer so it
## renders on top of the 3D viewport.
##
## See also: placement_controller.gd which listens for these signals and
## handles the click-to-place flow, main.gd which wires toolbar to controller.

extends MarginContainer

signal spawn_requested(species_name: String)
signal action_requested(action_name: String)

var _elf_button: Button
var _capybara_button: Button
var _summon_button: Button


func _ready() -> void:
	# Anchor to top-left with some padding.
	add_theme_constant_override("margin_left", 10)
	add_theme_constant_override("margin_top", 10)

	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 8)
	add_child(hbox)

	_elf_button = Button.new()
	_elf_button.text = "Spawn Elf [1]"
	_elf_button.pressed.connect(_on_elf_pressed)
	hbox.add_child(_elf_button)

	_capybara_button = Button.new()
	_capybara_button.text = "Spawn Capybara [2]"
	_capybara_button.pressed.connect(_on_capybara_pressed)
	hbox.add_child(_capybara_button)

	_summon_button = Button.new()
	_summon_button.text = "Summon Elf [3]"
	_summon_button.pressed.connect(_on_summon_pressed)
	hbox.add_child(_summon_button)


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		var key := event as InputEventKey
		if key.keycode == KEY_1:
			_on_elf_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_2:
			_on_capybara_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_3:
			_on_summon_pressed()
			get_viewport().set_input_as_handled()


func _on_elf_pressed() -> void:
	spawn_requested.emit("Elf")


func _on_capybara_pressed() -> void:
	spawn_requested.emit("Capybara")


func _on_summon_pressed() -> void:
	action_requested.emit("Summon")
