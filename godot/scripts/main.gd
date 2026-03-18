## Main scene controller for Elven Canopy.
##
## Orchestrates startup and the per-frame sim loop. Everything in the game
## connects through this script.
##
## Startup sequence (_ready):
## 1. Check GameSession.load_save_path — if set, load a saved game instead
##    of starting fresh.
## 2. For new games: read the simulation seed from GameSession autoload (set
##    by the new-game menu) and initialize SimBridge. Initial creatures are
##    spawned from GameConfig by the session during StartGame processing.
## 3. For loaded games: read the save file, call bridge.load_game_json(),
##    and skip creature spawning (creatures are already in the loaded state).
## 4. Common path: set up renderers, toolbar, placement controller,
##    construction controller, selection controller, creature info panel,
##    menu button, and pause menu.
##
## Per-frame (_process): calls bridge.frame_update(delta) which handles
## tick pacing (via LocalRelay in SP, network polling in MP) and returns a
## fractional render_tick for smooth creature interpolation between nav
## nodes. GDScript distributes this render_tick to renderers and the
## selection controller.
##
## ESC precedence chain (reverse tree order — later children fire first):
## 1. placement_controller — cancel active placement
## 2. construction_controller — if placing: exit placing sub-mode;
##    if active: exit construction mode
## 3. selection_controller — deselect creature
## 4. tree_info_panel — close tree info (if visible, on CanvasLayer layer 1)
## 5. help_panel — close keybind help (if visible, on CanvasLayer layer 2)
## 6. units_panel — close units roster (if visible, on CanvasLayer layer 2)
## 7. structure_list_panel — close structure list (if visible, on CanvasLayer layer 2)
## 8. task_panel — close task list (if visible, on CanvasLayer layer 2)
## 9. pause_menu — open/close (on CanvasLayer layer 2, added first)
##
## See also: orbital_camera.gd for camera controls, sim_bridge.rs and
## elfcyclopedia_server.rs (Rust) for the simulation interface and the embedded
## localhost elfcyclopedia HTTP server (started at launch, URL shown via a small
## book button next to the Menu button), tree_renderer.gd / elf_renderer.gd /
## capybara_renderer.gd / blueprint_renderer.gd / ladder_renderer.gd /
## furniture_renderer.gd / ground_pile_renderer.gd / projectile_renderer.gd for rendering,
## action_toolbar.gd for the toolbar UI, placement_controller.gd for
## click-to-place logic, construction_controller.gd for construction mode
## and platform placement, selection_controller.gd for click-to-select,
## tooltip_controller.gd for hover tooltips,
## minimap.gd for the bottom-right zoomable top-down minimap,
## notification_display.gd for toast-style notifications,
## status_bar.gd for the persistent bottom-left status bar,
## keybind_help.gd for the keyboard shortcuts help overlay,
## creature_info_panel.gd for the creature info panel,
## group_info_panel.gd for the multi-creature selection panel,
## selection_highlight.gd for faction-colored selection rings,
## structure_info_panel.gd for the structure info panel,
## ground_pile_info_panel.gd for the ground pile info panel,
## tree_info_panel.gd for the tree stats panel, task_panel.gd for
## the task list overlay, structure_list_panel.gd for the structure list
## overlay, units_panel.gd for the creature roster overlay,
## game_session.gd for the autoload that carries the seed/load
## path from the menu, pause_menu.gd for the ESC pause overlay.

extends Node3D

## Emitted at the end of _setup_common() when the game is fully ready to play.
## Tests poll for this (or for SimBridge.is_initialized()) to know when the
## scene is interactive.  Also useful for any system that needs to defer work
## until all renderers, controllers, and panels exist.
signal setup_complete

## Y offsets per species for world-space sprite positions. Must match the
## values used by elf_renderer.gd, capybara_renderer.gd, and creature_renderer.gd.
const SPECIES_Y_OFFSETS = {
	"Elf": 0.48,
	"Capybara": 0.32,
	"Boar": 0.38,
	"Deer": 0.46,
	"Elephant": 0.8,
	"Goblin": 0.36,
	"Monkey": 0.44,
	"Orc": 0.48,
	"Squirrel": 0.28,
	"Troll": 0.8,
}

## The simulation seed. Deterministic: same seed = same game.
## Overridden by GameSession.sim_seed when launched through the menu flow.
## The @export default (42) is a fallback for direct scene launches (F6 in editor).
@export var sim_seed: int = 42

var _selector: Node3D
var _panel: PanelContainer
var _group_panel: PanelContainer
var _structure_info_panel: PanelContainer
var _pile_info_panel: PanelContainer
var _tree_info_panel: PanelContainer
var _task_panel: ColorRect
var _structure_panel: ColorRect
var _units_panel: ColorRect
var _military_panel: PanelContainer
var _help_panel: ColorRect
var _camera_pivot: Node3D
var _construction_controller: Node
var _placement_controller: Node3D
## Renderers for new species (Boar, Deer, Monkey, Squirrel). Receive
## render_tick each frame for smooth creature interpolation.
var _extra_renderers: Array = []
var _tree_renderer: Node3D
var _bp_renderer: Node3D
var _bldg_renderer: Node3D
var _ladder_renderer: Node3D
var _furniture_renderer: Node3D
var _pile_renderer: Node3D
var _projectile_renderer: Node3D
var _selection_highlight: Node3D
var _tooltip_controller: Node
var _notification_display: VBoxContainer
var _status_bar: PanelContainer
var _minimap: PanelContainer
var _construction_music: Node
var _view_toolbar: MarginContainer
var _roofs_hidden: bool = false
var _height_cutoff_active: bool = false
var _last_cutoff_y: int = -1
var _roof_btn: Control
var _height_btn: Control
## Highest notification ID seen so far, for polling new notifications.
## Initialized from the sim after load so historical notifications aren't
## replayed as toasts.
var _last_notification_id: int = 0
var _pause_menu: ColorRect
var _lobby_overlay: ColorRect
var _elfcyclopedia_url_label: RichTextLabel


func _ready() -> void:
	var bridge: SimBridge = $SimBridge
	var is_loaded_game := false

	# --- Branch: multiplayer ---
	if GameSession.multiplayer_mode == "host":
		var ok := (
			bridge
			. host_game(
				GameSession.mp_port,
				GameSession.mp_session_name,
				GameSession.mp_password,
				GameSession.mp_max_players,
				GameSession.mp_ticks_per_turn,
				GameSession.player_name,
			)
		)
		if not ok:
			push_error("main: failed to host game, returning to menu")
			GameSession.multiplayer_mode = ""
			get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
			return
		# Read seed from GameSession for use when starting.
		if GameSession.sim_seed >= 0:
			sim_seed = GameSession.sim_seed
		_show_lobby(bridge)
		return

	if GameSession.multiplayer_mode == "join":
		var ok := (
			bridge
			. join_game(
				GameSession.mp_relay_address,
				GameSession.mp_player_name,
				GameSession.mp_password,
				GameSession.mp_session_id,
			)
		)
		if not ok:
			push_error("main: failed to join game, returning to menu")
			GameSession.multiplayer_mode = ""
			get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
			return
		_show_lobby(bridge)
		return

	# --- Branch: load a saved game or start a new one ---
	# Set player name for single-player sessions (both new and loaded games).
	if GameSession.multiplayer_mode == "" and not GameSession.player_name.is_empty():
		bridge.set_player_name(GameSession.player_name)

	if GameSession.load_save_path != "":
		is_loaded_game = _try_load_save(bridge, GameSession.load_save_path)
		GameSession.load_save_path = ""
		if not is_loaded_game:
			# Load failed — return to main menu.
			push_error("main: failed to load save, returning to main menu")
			get_tree().change_scene_to_file("res://scenes/main_menu.tscn")
			return

	if not is_loaded_game:
		# Normal new-game flow.
		if GameSession.sim_seed >= 0:
			sim_seed = GameSession.sim_seed

		if not GameSession.tree_profile.is_empty():
			var json_str := JSON.stringify(GameSession.tree_profile)
			bridge.init_sim_with_tree_profile_json(sim_seed, json_str)
		else:
			bridge.init_sim(sim_seed)
		print("Elven Canopy: sim initialized (seed=%d)" % sim_seed)

	_setup_common(bridge)


## Show the lobby overlay and wait for the game to start.
## When game_started fires, proceed with common setup.
func _show_lobby(bridge: SimBridge) -> void:
	var lobby_layer := CanvasLayer.new()
	lobby_layer.layer = 3
	add_child(lobby_layer)

	var lobby_script = load("res://scripts/lobby_overlay.gd")
	_lobby_overlay = ColorRect.new()
	_lobby_overlay.set_script(lobby_script)
	lobby_layer.add_child(_lobby_overlay)
	_lobby_overlay.setup(bridge)
	_lobby_overlay.game_started.connect(_on_mp_game_started)


## Called when the multiplayer game starts (lobby's game_started signal).
## Now the sim is initialized (initial creatures spawned via config) — set
## up renderers and UI.
func _on_mp_game_started() -> void:
	var bridge: SimBridge = $SimBridge
	_setup_common(bridge)
	print("Elven Canopy: multiplayer game ready")


## Set up renderers, toolbar, controllers, and menus. Called for both
## single-player (immediately in _ready) and multiplayer (after game_started).
func _setup_common(bridge: SimBridge) -> void:
	# Set up tree renderer (refreshed every frame for carve updates).
	_tree_renderer = $TreeRenderer
	_tree_renderer.setup(bridge, $CameraPivot/Camera3D)

	# Set up elf renderer.
	var elf_renderer = $ElfRenderer
	elf_renderer.setup(bridge)

	# Set up capybara renderer (sim-driven).
	var capybara_renderer = $CapybaraRenderer
	capybara_renderer.setup(bridge)

	# Set up generic renderers for new species.
	var renderer_script = load("res://scripts/creature_renderer.gd")
	for entry in [
		["Boar", 0.38],
		["Deer", 0.46],
		["Elephant", 0.8],
		["Goblin", 0.36],
		["Monkey", 0.44],
		["Orc", 0.48],
		["Squirrel", 0.28],
		["Troll", 0.8],
	]:
		var r := Node3D.new()
		r.set_script(renderer_script)
		add_child(r)
		r.setup(bridge, entry[0], entry[1])
		_extra_renderers.append(r)

	# Set up selection highlight renderer (rings at selected creatures' feet).
	var highlight_script = load("res://scripts/selection_highlight.gd")
	_selection_highlight = Node3D.new()
	_selection_highlight.set_script(highlight_script)
	add_child(_selection_highlight)
	_selection_highlight.setup(bridge)

	# Set up mana-wasted VFX (floating blue swirls).
	var mana_vfx_script = load("res://scripts/mana_vfx.gd")
	var mana_vfx := Node3D.new()
	mana_vfx.set_script(mana_vfx_script)
	add_child(mana_vfx)
	mana_vfx.setup(bridge)

	# Set up action toolbar UI (rendered on top of 3D via CanvasLayer).
	var canvas_layer := CanvasLayer.new()
	add_child(canvas_layer)

	var toolbar_script = load("res://scripts/action_toolbar.gd")
	var toolbar := MarginContainer.new()
	toolbar.name = "ActionToolbar"
	toolbar.set_script(toolbar_script)
	canvas_layer.add_child(toolbar)

	# Set up notification display (toast-style, bottom-right corner).
	var notif_script = load("res://scripts/notification_display.gd")
	_notification_display = VBoxContainer.new()
	_notification_display.set_script(notif_script)
	canvas_layer.add_child(_notification_display)

	# Set up status bar (bottom-left, at-a-glance stats).
	var status_bar_script = load("res://scripts/status_bar.gd")
	_status_bar = PanelContainer.new()
	_status_bar.name = "StatusBar"
	_status_bar.set_script(status_bar_script)
	_status_bar.bridge = bridge
	canvas_layer.add_child(_status_bar)

	# Set up minimap (bottom-right, zoomable top-down view).
	var minimap_script = load("res://scripts/minimap.gd")
	_minimap = PanelContainer.new()
	_minimap.name = "Minimap"
	_minimap.set_script(minimap_script)
	canvas_layer.add_child(_minimap)

	# Set up construction music controller.
	var music_script = load("res://scripts/construction_music.gd")
	_construction_music = Node.new()
	_construction_music.set_script(music_script)
	_construction_music.bridge = bridge
	add_child(_construction_music)

	# Set up placement controller.
	var controller_script = load("res://scripts/placement_controller.gd")
	_placement_controller = Node3D.new()
	_placement_controller.set_script(controller_script)
	add_child(_placement_controller)
	_placement_controller.setup(bridge, $CameraPivot/Camera3D)
	_placement_controller.connect_toolbar(toolbar)

	# Set up height grid renderer.
	var grid_script = load("res://scripts/height_grid_renderer.gd")
	var height_grid := Node3D.new()
	height_grid.set_script(grid_script)
	add_child(height_grid)
	height_grid.setup(bridge, $CameraPivot)
	height_grid.visible = false

	# Set up construction controller.
	var construction_script = load("res://scripts/construction_controller.gd")
	_construction_controller = Node.new()
	_construction_controller.name = "ConstructionController"
	_construction_controller.set_script(construction_script)
	add_child(_construction_controller)
	_construction_controller.setup(bridge, $CameraPivot)
	_construction_controller.set_height_grid_renderer(height_grid)
	_construction_controller.connect_toolbar(toolbar)
	canvas_layer.add_child(_construction_controller.get_panel())

	# Entering construction mode: deselect, cancel placement, hide panels.
	_construction_controller.construction_mode_entered.connect(
		func():
			if _placement_controller.is_placing():
				_placement_controller.cancel_placement()
			if _selector:
				_selector.deselect()
			if _panel:
				_panel.hide_panel()
			if _group_panel and _group_panel.visible:
				_group_panel.hide_panel()
			if _structure_info_panel and _structure_info_panel.visible:
				_structure_info_panel.hide_panel()
			if _pile_info_panel and _pile_info_panel.visible:
				_pile_info_panel.hide_panel()
			if _tree_info_panel and _tree_info_panel.visible:
				_tree_info_panel.hide_panel()
			if _camera_pivot:
				_camera_pivot.stop_follow()
	)

	# Set up blueprint renderer.
	var bp_renderer_script = load("res://scripts/blueprint_renderer.gd")
	_bp_renderer = Node3D.new()
	_bp_renderer.set_script(bp_renderer_script)
	add_child(_bp_renderer)
	_bp_renderer.setup(bridge)
	_construction_controller.blueprint_placed.connect(_bp_renderer.refresh)

	# Set up building renderer.
	var bldg_renderer_script = load("res://scripts/building_renderer.gd")
	_bldg_renderer = Node3D.new()
	_bldg_renderer.set_script(bldg_renderer_script)
	_bldg_renderer.name = "BuildingRenderer"
	add_child(_bldg_renderer)
	_bldg_renderer.setup(bridge)
	_construction_controller.blueprint_placed.connect(_bldg_renderer.refresh)

	# Set up ladder renderer.
	var ladder_renderer_script = load("res://scripts/ladder_renderer.gd")
	_ladder_renderer = Node3D.new()
	_ladder_renderer.set_script(ladder_renderer_script)
	_ladder_renderer.name = "LadderRenderer"
	add_child(_ladder_renderer)
	_ladder_renderer.setup(bridge)
	_construction_controller.blueprint_placed.connect(_ladder_renderer.refresh)

	# Set up furniture renderer.
	var furniture_renderer_script = load("res://scripts/furniture_renderer.gd")
	_furniture_renderer = Node3D.new()
	_furniture_renderer.set_script(furniture_renderer_script)
	_furniture_renderer.name = "FurnitureRenderer"
	add_child(_furniture_renderer)
	_furniture_renderer.setup(bridge)

	# Set up ground pile renderer.
	var pile_renderer_script = load("res://scripts/ground_pile_renderer.gd")
	_pile_renderer = Node3D.new()
	_pile_renderer.set_script(pile_renderer_script)
	_pile_renderer.name = "GroundPileRenderer"
	add_child(_pile_renderer)
	_pile_renderer.setup(bridge)

	# Set up projectile renderer.
	var proj_renderer_script = load("res://scripts/projectile_renderer.gd")
	_projectile_renderer = Node3D.new()
	_projectile_renderer.set_script(proj_renderer_script)
	_projectile_renderer.name = "ProjectileRenderer"
	add_child(_projectile_renderer)
	_projectile_renderer.setup(bridge)

	# Set up view toolbar (right-edge toggles for roof/height visibility).
	_view_toolbar = MarginContainer.new()
	_view_toolbar.set_script(load("res://scripts/view_toolbar.gd"))
	canvas_layer.add_child(_view_toolbar)

	_roof_btn = _view_toolbar.add_toggle("Toggle roof visibility", ViewToggleIcons.draw_roof_icon)
	_roof_btn.toggled.connect(_on_roof_toggle)

	_height_btn = _view_toolbar.add_toggle("Toggle height cutoff", ViewToggleIcons.draw_height_icon)
	_height_btn.toggled.connect(_on_height_toggle)

	# Info panel layer (layer 3) — creature and structure info panels render
	# on top of the units panel (layer 2) so clicking a unit row shows the
	# detail panel above the roster overlay.
	var info_panel_layer := CanvasLayer.new()
	info_panel_layer.layer = 3
	add_child(info_panel_layer)

	# Set up creature info panel (single creature).
	var panel_script = load("res://scripts/creature_info_panel.gd")
	_panel = PanelContainer.new()
	_panel.name = "CreatureInfoPanel"
	_panel.set_script(panel_script)
	info_panel_layer.add_child(_panel)

	# Set up group info panel (multi-creature selection).
	var group_panel_script = load("res://scripts/group_info_panel.gd")
	_group_panel = PanelContainer.new()
	_group_panel.name = "GroupInfoPanel"
	_group_panel.set_script(group_panel_script)
	info_panel_layer.add_child(_group_panel)
	_group_panel.setup(bridge)

	# Set up structure info panel.
	var struct_panel_script = load("res://scripts/structure_info_panel.gd")
	_structure_info_panel = PanelContainer.new()
	_structure_info_panel.name = "StructureInfoPanel"
	_structure_info_panel.set_script(struct_panel_script)
	info_panel_layer.add_child(_structure_info_panel)

	# Set up ground pile info panel.
	var pile_panel_script = load("res://scripts/ground_pile_info_panel.gd")
	_pile_info_panel = PanelContainer.new()
	_pile_info_panel.name = "GroundPileInfoPanel"
	_pile_info_panel.set_script(pile_panel_script)
	info_panel_layer.add_child(_pile_info_panel)

	# Set up tooltip controller (hover tooltips on CanvasLayer 4 so they
	# render above info panels on layer 3).
	var tooltip_layer := CanvasLayer.new()
	tooltip_layer.layer = 4
	add_child(tooltip_layer)

	var tooltip_script = load("res://scripts/tooltip_controller.gd")
	_tooltip_controller = Node.new()
	_tooltip_controller.set_script(tooltip_script)
	add_child(_tooltip_controller)
	_tooltip_controller.setup(bridge, $CameraPivot/Camera3D, tooltip_layer)

	# Set up selection controller.
	var selector_script = load("res://scripts/selection_controller.gd")
	_selector = Node3D.new()
	_selector.name = "SelectionController"
	_selector.set_script(selector_script)
	add_child(_selector)
	_selector.setup(bridge, $CameraPivot/Camera3D)
	_selector.set_placement_controller(_placement_controller)
	_selector.set_construction_controller(_construction_controller)
	_tooltip_controller.set_placement_controller(_placement_controller)
	_tooltip_controller.set_construction_controller(_construction_controller)

	# Wire creature selection -> creature info panel.
	_camera_pivot = $CameraPivot

	# Finish minimap setup now that selector and camera pivot are available.
	if _minimap:
		_minimap.setup(bridge, _camera_pivot, _selector)
		_minimap.camera_jump_requested.connect(
			func(world_pos: Vector3): _look_at_position(world_pos)
		)

	# Home key: center camera on the home tree.
	_camera_pivot.home_requested.connect(_center_on_home_tree)

	# Selection group double-tap: center camera on group centroid.
	_selector.group_center_requested.connect(func(pos: Vector3): _look_at_position(pos))

	_selector.creatures_selected.connect(
		func(ids: Array):
			# Mutual exclusion: hide tree info, structure info, pile, and military panels.
			if _tree_info_panel and _tree_info_panel.visible:
				_tree_info_panel.hide_panel()
			if _structure_info_panel and _structure_info_panel.visible:
				_structure_info_panel.hide_panel()
			if _pile_info_panel and _pile_info_panel.visible:
				_pile_info_panel.hide_panel()
			if _military_panel and _military_panel.visible:
				_military_panel.toggle()
			if ids.size() == 1:
				# Single selection — show detailed creature info panel.
				if _group_panel and _group_panel.visible:
					_group_panel.hide_panel()
				var tick := float(bridge.current_tick())
				var info := bridge.get_creature_info_by_id(ids[0], tick)
				if not info.is_empty():
					_panel.show_creature(ids[0], info)
			elif ids.size() > 1:
				# Multi selection — show group overview panel.
				if _panel and _panel.visible:
					_panel.hide_panel()
				_group_panel.show_group(ids)
	)
	_selector.creature_deselected.connect(
		func():
			_panel.hide_panel()
			if _group_panel:
				_group_panel.hide_panel()
			_camera_pivot.stop_follow()
	)

	# Wire structure selection -> structure info panel.
	_selector.structure_selected.connect(
		func(structure_id: int):
			# Mutual exclusion: hide creature, tree info, and pile panels.
			if _panel and _panel.visible:
				_panel.hide_panel()
			if _tree_info_panel and _tree_info_panel.visible:
				_tree_info_panel.hide_panel()
			if _pile_info_panel and _pile_info_panel.visible:
				_pile_info_panel.hide_panel()
			if _camera_pivot:
				_camera_pivot.stop_follow()
			var info := bridge.get_structure_info(structure_id)
			if not info.is_empty():
				_structure_info_panel.show_structure(info)
	)
	_selector.structure_deselected.connect(func(): _structure_info_panel.hide_panel())

	# Wire pile selection -> pile info panel.
	_selector.pile_selected.connect(
		func(x: int, y: int, z: int):
			# Mutual exclusion: hide creature, structure, and tree info panels.
			if _panel and _panel.visible:
				_panel.hide_panel()
			if _structure_info_panel and _structure_info_panel.visible:
				_structure_info_panel.hide_panel()
			if _tree_info_panel and _tree_info_panel.visible:
				_tree_info_panel.hide_panel()
			if _camera_pivot:
				_camera_pivot.stop_follow()
			var info := bridge.get_ground_pile_info(x, y, z)
			if not info.is_empty():
				_pile_info_panel.show_pile(info)
	)
	_selector.pile_deselected.connect(func(): _pile_info_panel.hide_panel())

	# Wire pile info panel close -> deselect.
	_pile_info_panel.panel_closed.connect(
		func():
			if _selector.get_selected_pile_pos() != Vector3i(-1, -1, -1):
				_selector.deselect()
	)

	# Wire structure info panel signals.
	_structure_info_panel.zoom_requested.connect(
		func(x: float, y: float, z: float): _look_at_position(Vector3(x + 0.5, y, z + 0.5))
	)
	_structure_info_panel.panel_closed.connect(
		func():
			if _selector.get_selected_structure_id() >= 0:
				_selector.deselect()
	)
	_structure_info_panel.rename_requested.connect(
		func(structure_id: int, new_name: String): bridge.rename_structure(structure_id, new_name)
	)
	_structure_info_panel.furnish_requested.connect(
		func(structure_id: int, furnishing_type: String, species_id: int):
			bridge.furnish_structure(structure_id, furnishing_type, species_id)
	)
	_structure_info_panel.assign_elf_requested.connect(
		func(structure_id: int, creature_id_str: String):
			bridge.assign_home(creature_id_str, structure_id)
	)
	_structure_info_panel.unassign_elf_requested.connect(
		func(_structure_id: int, creature_id_str: String): bridge.assign_home(creature_id_str, -1)
	)
	_structure_info_panel.logistics_priority_changed.connect(
		func(sid: int, p: int): bridge.set_logistics_priority(sid, p)
	)
	_structure_info_panel.logistics_wants_changed.connect(
		func(sid: int, json: String): bridge.set_logistics_wants(sid, json)
	)
	_structure_info_panel.crafting_enabled_changed.connect(
		func(sid: int, enabled: bool): bridge.set_crafting_enabled(sid, enabled)
	)
	_structure_info_panel.add_recipe_requested.connect(
		func(sid: int, recipe_variant: int, material_json: String):
			bridge.add_active_recipe(sid, recipe_variant, material_json)
	)
	_structure_info_panel.remove_recipe_requested.connect(
		func(ar_id: int): bridge.remove_active_recipe(ar_id)
	)
	_structure_info_panel.recipe_output_target_changed.connect(
		func(target_id: int, qty: int): bridge.set_recipe_output_target(target_id, qty)
	)
	_structure_info_panel.recipe_auto_logistics_changed.connect(
		func(ar_id: int, auto: bool, spare: int):
			bridge.set_recipe_auto_logistics(ar_id, auto, spare)
	)
	_structure_info_panel.recipe_enabled_changed.connect(
		func(ar_id: int, enabled: bool): bridge.set_recipe_enabled(ar_id, enabled)
	)
	_structure_info_panel.recipe_move_up_requested.connect(
		func(ar_id: int): bridge.move_active_recipe_up(ar_id)
	)
	_structure_info_panel.recipe_move_down_requested.connect(
		func(ar_id: int): bridge.move_active_recipe_down(ar_id)
	)
	_structure_info_panel.set_cultivable_fruits(bridge.get_cultivable_fruit_species())
	# Cache logistics item kinds and material options for the two-step picker.
	var item_kinds: Array = bridge.get_logistics_item_kinds()
	_structure_info_panel.set_logistics_item_kinds(item_kinds)
	var mat_options: Dictionary = {}
	for entry in item_kinds:
		var kind: String = entry.get("kind", "")
		mat_options[kind] = bridge.get_logistics_material_options(kind)
	_structure_info_panel.set_logistics_material_options(mat_options)

	# Menu button.
	var menu_btn := Button.new()
	menu_btn.text = "Menu"
	menu_btn.custom_minimum_size = Vector2(80, 40)
	menu_btn.set_anchors_preset(Control.PRESET_TOP_RIGHT)
	menu_btn.position = Vector2(-90, 10)
	canvas_layer.add_child(menu_btn)

	# Book button toggles a clickable elfcyclopedia URL. Server was started at
	# Godot launch by game_session.gd; the URL is read from GameSession.
	var enc_url := GameSession.elfcyclopedia_url
	if not enc_url.is_empty():
		var book_btn := Button.new()
		book_btn.text = "B"
		book_btn.tooltip_text = "Elfcyclopedia"
		book_btn.custom_minimum_size = Vector2(28, 28)
		book_btn.focus_mode = Control.FOCUS_NONE
		book_btn.add_theme_font_size_override("font_size", 16)
		book_btn.set_anchors_preset(Control.PRESET_TOP_RIGHT)
		book_btn.position = Vector2(-120, 10)
		canvas_layer.add_child(book_btn)

		_elfcyclopedia_url_label = RichTextLabel.new()
		_elfcyclopedia_url_label.bbcode_enabled = true
		_elfcyclopedia_url_label.text = "[url]%s[/url]" % enc_url
		_elfcyclopedia_url_label.fit_content = true
		_elfcyclopedia_url_label.scroll_active = false
		_elfcyclopedia_url_label.custom_minimum_size = Vector2(200, 24)
		_elfcyclopedia_url_label.add_theme_font_size_override("normal_font_size", 13)
		_elfcyclopedia_url_label.set_anchors_preset(Control.PRESET_TOP_RIGHT)
		_elfcyclopedia_url_label.position = Vector2(-320, 12)
		_elfcyclopedia_url_label.visible = false
		_elfcyclopedia_url_label.meta_clicked.connect(func(_meta: Variant): OS.shell_open(enc_url))
		canvas_layer.add_child(_elfcyclopedia_url_label)

		book_btn.pressed.connect(_toggle_elfcyclopedia_url)

	# Pause menu overlay.
	var pause_layer := CanvasLayer.new()
	pause_layer.layer = 2
	add_child(pause_layer)

	var pause_script = load("res://scripts/pause_menu.gd")
	_pause_menu = ColorRect.new()
	_pause_menu.name = "PauseMenu"
	_pause_menu.set_script(pause_script)
	pause_layer.add_child(_pause_menu)
	_pause_menu.setup(bridge)
	menu_btn.pressed.connect(_pause_menu.toggle)

	# Task panel overlay.
	var task_panel_layer := CanvasLayer.new()
	task_panel_layer.layer = 2
	add_child(task_panel_layer)

	var task_panel_script = load("res://scripts/task_panel.gd")
	_task_panel = ColorRect.new()
	_task_panel.name = "TaskPanel"
	_task_panel.set_script(task_panel_script)
	task_panel_layer.add_child(_task_panel)

	# Structure list panel overlay (on CanvasLayer 2, added after task panel
	# so its ESC handler fires first in reverse tree order).
	var structure_panel_layer := CanvasLayer.new()
	structure_panel_layer.layer = 2
	add_child(structure_panel_layer)

	var structure_panel_script = load("res://scripts/structure_list_panel.gd")
	_structure_panel = ColorRect.new()
	_structure_panel.name = "StructureListPanel"
	_structure_panel.set_script(structure_panel_script)
	structure_panel_layer.add_child(_structure_panel)

	# Units panel overlay (on CanvasLayer 2, added after structure panel
	# so its ESC handler fires first in reverse tree order).
	var units_panel_layer := CanvasLayer.new()
	units_panel_layer.layer = 2
	add_child(units_panel_layer)

	var units_panel_script = load("res://scripts/units_panel.gd")
	_units_panel = ColorRect.new()
	_units_panel.name = "UnitsPanel"
	_units_panel.set_script(units_panel_script)
	units_panel_layer.add_child(_units_panel)

	# Help panel (keybind overlay, same layer as other overlays).
	var help_panel_layer := CanvasLayer.new()
	help_panel_layer.layer = 2
	add_child(help_panel_layer)

	var help_panel_script = load("res://scripts/keybind_help.gd")
	_help_panel = ColorRect.new()
	_help_panel.name = "HelpPanel"
	_help_panel.set_script(help_panel_script)
	help_panel_layer.add_child(_help_panel)

	# Tree info panel (on its own CanvasLayer, added after units panel
	# so its ESC handler fires first in reverse tree order).
	var tree_panel_layer := CanvasLayer.new()
	tree_panel_layer.layer = 1
	add_child(tree_panel_layer)

	var tree_panel_script = load("res://scripts/tree_info_panel.gd")
	_tree_info_panel = PanelContainer.new()
	_tree_info_panel.name = "TreeInfoPanel"
	_tree_info_panel.set_script(tree_panel_script)
	tree_panel_layer.add_child(_tree_info_panel)

	# Wire structure panel zoom -> move camera + select structure.
	_structure_panel.zoom_to_structure.connect(
		func(structure_id: int, x: float, y: float, z: float):
			_structure_panel.hide_panel()
			_look_at_position(Vector3(x + 0.5, y, z + 0.5))
			_selector.select_structure(structure_id)
	)

	# Wire units panel creature click -> select creature (panel stays open).
	_units_panel.creature_clicked.connect(
		func(creature_id: String): _selector.select_creature_by_id(creature_id)
	)

	# Military groups panel (right-side, same CanvasLayer as creature info panels).
	var military_panel_layer := CanvasLayer.new()
	military_panel_layer.layer = 3
	add_child(military_panel_layer)
	var military_panel_script = load("res://scripts/military_panel.gd")
	_military_panel = PanelContainer.new()
	_military_panel.name = "MilitaryPanel"
	_military_panel.set_script(military_panel_script)
	military_panel_layer.add_child(_military_panel)
	_military_panel.setup(bridge)

	# Wire military panel close -> no-op (just hides).
	_military_panel.panel_closed.connect(func(): pass)

	# Wire creature info panel military group click -> open military panel.
	_panel.military_group_clicked.connect(
		func(group_id: int):
			_panel.hide_panel()
			_military_panel.show_group_detail(group_id)
	)

	# Wire speed controls.
	toolbar.speed_changed.connect(
		func(speed_name: String):
			bridge.set_sim_speed(speed_name)
			if _status_bar:
				_status_bar.set_speed(speed_name)
	)

	# Wire toolbar actions -> panel toggles.
	toolbar.action_requested.connect(
		func(action: String):
			if action == "Tasks":
				_task_panel.toggle()
			elif action == "Structures":
				_structure_panel.toggle()
			elif action == "Units":
				_units_panel.toggle()
			elif action == "Military":
				# Close creature info if open (shares screen space).
				if _panel and _panel.visible:
					_panel.hide_panel()
				_military_panel.toggle()
			elif action == "TreeInfo":
				# Mutual exclusion: opening tree info deselects creature/structure/pile.
				if not _tree_info_panel.visible:
					if _selector:
						_selector.deselect()
					if _panel and _panel.visible:
						_panel.hide_panel()
					if _structure_info_panel and _structure_info_panel.visible:
						_structure_info_panel.hide_panel()
					if _pile_info_panel and _pile_info_panel.visible:
						_pile_info_panel.hide_panel()
					if _camera_pivot:
						_camera_pivot.stop_follow()
				_tree_info_panel.toggle()
			elif action == "Help":
				_help_panel.toggle()
			elif action == "TestNotification":
				bridge.send_debug_notification(
					"Test notification at tick %d" % bridge.current_tick()
				)
			elif action == "TriggerRaid":
				bridge.trigger_raid()
	)

	_task_panel.zoom_to_creature.connect(
		func(creature_id: String):
			_task_panel.hide_panel()
			_selector.select_creature_by_id(creature_id)
			var tick := float(bridge.current_tick())
			var pos = _get_creature_world_pos_by_id(bridge, tick, creature_id)
			if pos != null:
				_camera_pivot.start_follow(pos)
	)

	_task_panel.zoom_to_location.connect(
		func(x: float, y: float, z: float):
			_task_panel.hide_panel()
			_look_at_position(Vector3(x + 0.5, y, z + 0.5))
	)

	# Wire group panel signals.
	_group_panel.creature_clicked.connect(
		func(creature_id: String): _selector.select_creature_by_id(creature_id)
	)
	_group_panel.panel_closed.connect(
		func():
			_selector.deselect()
			_camera_pivot.stop_follow()
	)

	_panel.follow_requested.connect(
		func():
			var cid: String = _selector.get_selected_creature_id()
			if cid != "":
				var tick := float(bridge.current_tick())
				var pos = _get_creature_world_pos_by_id(bridge, tick, cid)
				if pos != null:
					_camera_pivot.start_follow(pos)
	)
	_panel.unfollow_requested.connect(func(): _camera_pivot.stop_follow())
	_panel.zoom_to_task_location.connect(
		func(x: float, y: float, z: float): _look_at_position(Vector3(x + 0.5, y, z + 0.5))
	)
	_panel.panel_closed.connect(
		func():
			_selector.deselect()
			_camera_pivot.stop_follow()
	)

	# Fix ESC precedence. _unhandled_input fires in reverse tree order (last
	# child first). Move the three input controllers to the end so they get
	# ESC before panels and the pause menu. Order after move:
	#   ... → pause → tasks → structures → units → selector → construct → place
	# Reverse (input order): place → construct → selector → units → panels → pause
	move_child(_selector, -1)
	move_child(_construction_controller, -1)
	move_child(_placement_controller, -1)

	# Initialize notification cursor to the highest existing ID so that
	# historical notifications (from a loaded save) aren't replayed as toasts.
	_last_notification_id = bridge.get_max_notification_id()

	# Hydrate selection groups from sim (restores groups after loading a save).
	_selector.hydrate_selection_groups()

	setup_complete.emit()


## Try to load a save file. Returns true on success.
func _try_load_save(bridge: SimBridge, save_path: String) -> bool:
	var file := FileAccess.open(save_path, FileAccess.READ)
	if file == null:
		push_error("main: cannot open save file: %s" % save_path)
		return false
	var json := file.get_as_text()
	file.close()
	if json.is_empty():
		push_error("main: save file is empty: %s" % save_path)
		return false
	# Stop any playing construction music before loading new state.
	if _construction_music:
		_construction_music.stop_all()
	var ok := bridge.load_game_json(json)
	if ok:
		print("Elven Canopy: loaded save from %s (tick=%d)" % [save_path, bridge.current_tick()])
	return ok


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		$SimBridge.shutdown()


func _process(delta: float) -> void:
	var bridge: SimBridge = $SimBridge
	if not bridge.is_initialized():
		return

	var render_tick := bridge.frame_update(delta)

	if bridge.is_multiplayer():
		var events := bridge.poll_mp_events()
		for event_json in events:
			print("MP event: %s" % event_json)

	# Distribute render_tick to all consumers that read creature positions.
	$ElfRenderer.set_render_tick(render_tick)
	$CapybaraRenderer.set_render_tick(render_tick)
	for r in _extra_renderers:
		r.set_render_tick(render_tick)
	if _projectile_renderer:
		_projectile_renderer.set_render_tick(render_tick)
	_selector.set_render_tick(render_tick)
	if _minimap:
		_minimap.set_render_tick(render_tick)
		_minimap.set_selected_ids(_selector.get_selected_creature_ids())
	if _tooltip_controller:
		_tooltip_controller.set_render_tick(render_tick)
		# Suppress tooltip when any overlay panel is open.
		var any_overlay := (
			(_pause_menu and _pause_menu.visible)
			or (_task_panel and _task_panel.visible)
			or (_structure_panel and _structure_panel.visible)
			or (_units_panel and _units_panel.visible)
			or (_help_panel and _help_panel.visible)
		)
		_tooltip_controller.set_suppressed(any_overlay)

	# Refresh tree renderer so carved voxels disappear and new fruit appears.
	if _tree_renderer:
		_tree_renderer.refresh()

	# Refresh blueprint/construction renderer so materialized voxels appear
	# as solid wood and ghost cubes disappear as voxels are placed.
	if _bp_renderer:
		_bp_renderer.refresh()
	if _bldg_renderer:
		_bldg_renderer.refresh()
	if _ladder_renderer:
		_ladder_renderer.refresh()
	if _furniture_renderer:
		_furniture_renderer.refresh()
	if _pile_renderer:
		_pile_renderer.refresh()

	# Update follow target each frame so the camera tracks creature movement.
	if _camera_pivot and _camera_pivot.is_following():
		var cid: String = _selector.get_selected_creature_id()
		if cid != "":
			var pos = _get_creature_world_pos_by_id(bridge, render_tick, cid)
			if pos != null:
				_camera_pivot.update_follow_target(pos)
			else:
				_camera_pivot.stop_follow()
				_panel.set_follow_state(false)
		else:
			_camera_pivot.stop_follow()
			_panel.set_follow_state(false)

	# Detect if camera broke follow via movement keys.
	if _panel and _panel.visible and not _camera_pivot.is_following():
		_panel.set_follow_state(false)

	# Refresh task panel while visible.
	if _task_panel and _task_panel.visible:
		var tasks := bridge.get_active_tasks()
		_task_panel.update_tasks(tasks)

	# Refresh structure list panel while visible.
	if _structure_panel and _structure_panel.visible:
		var structures := bridge.get_structures()
		_structure_panel.update_structures(structures)

	# Refresh units panel while visible.
	if _units_panel and _units_panel.visible:
		var creatures := bridge.get_all_creatures_summary()
		_units_panel.update_creatures(creatures)

	# Refresh tree info panel while visible.
	if _tree_info_panel and _tree_info_panel.visible:
		var tree_data := bridge.get_home_tree_info()
		_tree_info_panel.update_info(tree_data)

	# Refresh creature info panel while a creature is selected.
	var selected_cid: String = _selector.get_selected_creature_id()
	if _panel and _panel.visible and selected_cid != "":
		var info := bridge.get_creature_info_by_id(selected_cid, render_tick)
		if info.is_empty():
			_selector.deselect()
		else:
			_panel.update_info(info)

	# Refresh group info panel while multiple creatures are selected.
	# Prune dead creatures from the selection first.
	if _group_panel and _group_panel.visible:
		_group_panel.set_render_tick(render_tick)
		var group_ids: Array = _selector.get_selected_creature_ids()
		for cid in group_ids.duplicate():
			var info := bridge.get_creature_info_by_id(cid, render_tick)
			if info.is_empty():
				_selector.remove_creature_id(cid)
		_group_panel.update_group(_selector.get_selected_creature_ids())

	# Update selection highlight rings under selected creatures.
	if _selection_highlight:
		_selection_highlight.set_render_tick(render_tick)
		_selection_highlight.update_highlights(_selector.get_selected_creature_ids())

	# Refresh structure info panel while a structure is selected.
	if (
		_structure_info_panel
		and _structure_info_panel.visible
		and _selector.get_selected_structure_id() >= 0
	):
		var sinfo := bridge.get_structure_info(_selector.get_selected_structure_id())
		if not sinfo.is_empty():
			_structure_info_panel.update_info(sinfo)
			if _structure_info_panel.is_elf_picker_visible():
				_structure_info_panel.set_elf_list(bridge.get_all_elves())
			if _structure_info_panel.is_crafting_details_visible():
				_structure_info_panel.set_building_recipes(
					bridge.get_available_recipes(_selector.get_selected_structure_id())
				)
		else:
			# Structure was demolished — deselect and hide panel.
			_selector.deselect()

	# Refresh pile info panel while a pile is selected.
	if (
		_pile_info_panel
		and _pile_info_panel.visible
		and _selector.get_selected_pile_pos() != Vector3i(-1, -1, -1)
	):
		var pp: Vector3i = _selector.get_selected_pile_pos()
		var pinfo := bridge.get_ground_pile_info(pp.x, pp.y, pp.z)
		if not pinfo.is_empty():
			_pile_info_panel.update_info(pinfo)
		else:
			# Pile was removed — deselect and hide panel.
			_selector.deselect()

	# Update height cutoff when camera focus Y changes.
	if _height_cutoff_active:
		var new_y: int = _camera_pivot.get_focus_voxel().y + 1
		if new_y != _last_cutoff_y:
			_last_cutoff_y = new_y
			bridge.set_mesh_y_cutoff(new_y)

	# Poll for new notifications and push them to the toast display.
	if _notification_display:
		var new_notifs := bridge.get_notifications_after(_last_notification_id)
		for notif in new_notifs:
			var nid: int = notif.get("id", 0)
			var msg: String = notif.get("message", "")
			if nid > _last_notification_id:
				_last_notification_id = nid
			if not msg.is_empty():
				_notification_display.push_notification(msg)

	# Poll for construction music composition completions.
	if _construction_music:
		_construction_music.poll_compositions()


func _on_roof_toggle(is_active: bool) -> void:
	_roofs_hidden = is_active
	_bldg_renderer.set_roofs_hidden(is_active)
	_selector.set_roofs_hidden(is_active)
	_tooltip_controller.set_roofs_hidden(is_active)


func _on_height_toggle(is_active: bool) -> void:
	_height_cutoff_active = is_active
	if is_active:
		var y: int = _camera_pivot.get_focus_voxel().y + 1
		_last_cutoff_y = y
		$SimBridge.set_mesh_y_cutoff(y)
	else:
		_last_cutoff_y = -1
		$SimBridge.set_mesh_y_cutoff(-1)


## Move the camera to look at a world-space position, stopping any active
## creature follow. Use this for any "jump the camera here" action that
## isn't creature-tracking (task sites, landmarks, etc.).
func _look_at_position(pos: Vector3) -> void:
	_camera_pivot.stop_follow()
	if _panel and _panel.visible:
		_panel.set_follow_state(false)
	_camera_pivot.position = pos


## Center the camera on the home tree's position. Called when the user presses
## the Home key. Keeps current zoom and pitch — only repositions the focal point.
func _center_on_home_tree() -> void:
	var bridge: SimBridge = $SimBridge
	var tree_data := bridge.get_home_tree_info()
	var x: float = tree_data.get("position_x", 0.0)
	var y: float = tree_data.get("position_y", 0.0)
	var z: float = tree_data.get("position_z", 0.0)
	_look_at_position(Vector3(x + 0.5, y, z + 0.5))


## Get the world-space position of a creature sprite by its stable ID.
## Uses get_creature_info_by_id for the position and applies species Y offset.
func _get_creature_world_pos_by_id(
	bridge: SimBridge, render_tick: float, creature_id: String
) -> Variant:
	var info := bridge.get_creature_info_by_id(creature_id, render_tick)
	if info.is_empty():
		return null
	var species: String = info.get("species", "")
	var y_off: float = SPECIES_Y_OFFSETS.get(species, 0.4)
	var x: float = info.get("x", 0.0)
	var y: float = info.get("y", 0.0)
	var z: float = info.get("z", 0.0)
	return Vector3(x + 0.5, y + y_off, z + 0.5)


## Toggle visibility of the elfcyclopedia URL next to the book button.
func _toggle_elfcyclopedia_url() -> void:
	if _elfcyclopedia_url_label:
		_elfcyclopedia_url_label.visible = not _elfcyclopedia_url_label.visible
