## Toolbar UI for spawning creatures.
##
## Builds a horizontal row of "Spawn Elf [1]" and "Spawn Capybara [2]" buttons.
## Emits `spawn_requested(species_name)` when a button is clicked or its
## keyboard shortcut is pressed.
##
## Created programmatically by main.gd and parented under a CanvasLayer so it
## renders on top of the 3D viewport.
##
## See also: placement_controller.gd which listens for `spawn_requested` and
## handles the click-to-place flow, main.gd which wires toolbar to controller.

extends MarginContainer

signal spawn_requested(species_name: String)

var _elf_button: Button
var _capybara_button: Button


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


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		var key := event as InputEventKey
		if key.keycode == KEY_1:
			_on_elf_pressed()
			get_viewport().set_input_as_handled()
		elif key.keycode == KEY_2:
			_on_capybara_pressed()
			get_viewport().set_input_as_handled()


func _on_elf_pressed() -> void:
	spawn_requested.emit("Elf")


func _on_capybara_pressed() -> void:
	spawn_requested.emit("Capybara")
