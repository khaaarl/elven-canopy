## Distance fog controller — applies depth-based atmospheric fog to the scene.
##
## Reads fog parameters from GameConfig each frame and applies them to the
## WorldEnvironment's Environment resource. This polling approach matches
## how tree_renderer.gd handles draw_distance — no signals needed.
##
## Uses Godot's FOG_MODE_DEPTH (not the default FOG_MODE_EXPONENTIAL).
## In depth mode, fog_depth_begin and fog_depth_end define the camera-
## distance range over which fog ramps from transparent to opaque, and
## fog_density controls the max opacity at fog_depth_end (0.0–1.0).
## fog_depth_curve shapes the ramp (1.0 = linear, >1 = late ramp).
##
## Fog parameters (all stored in GameConfig):
##   fog_enabled: bool = true   — master toggle
##   fog_begin: int = 40        — distance (voxels) where fog starts
##   fog_end: int = 80          — distance (voxels) where fog reaches max opacity
##
## Voxels are 2 meters on a side, so the config values are converted to
## Godot world units (* 2) before applying to the Environment.
##
## The fog color is matched to the scene's background color for a seamless
## blend at distance. Godot's built-in Environment fog handles all 3D
## geometry uniformly (meshes and billboard sprites).
##
## See also: settings_panel.gd (UI for fog settings), game_config.gd
## (persistence), main.gd (wires this controller to WorldEnvironment).

extends Node

## Background color — fog fades to this color at distance.
const FOG_COLOR := Color(0.55, 0.7, 0.85, 1.0)

## Meters per voxel — voxel coords to Godot world units.
const VOXEL_SIZE := 2.0

## The Environment resource to modify. Set via setup().
var _environment: Environment

## Cached values to avoid redundant Environment writes.
var _cached_enabled: bool = false
var _cached_begin: int = 0
var _cached_end: int = 0


## Initialize with the scene's Environment resource. Applies current
## GameConfig values immediately.
func setup(environment: Environment) -> void:
	_environment = environment
	_apply(
		GameConfig.get_setting("fog_enabled"),
		GameConfig.get_setting("fog_begin"),
		GameConfig.get_setting("fog_end"),
	)


func _process(_delta: float) -> void:
	if _environment == null:
		return
	var enabled: bool = GameConfig.get_setting("fog_enabled")
	var fog_begin: int = GameConfig.get_setting("fog_begin")
	var fog_end: int = GameConfig.get_setting("fog_end")
	if enabled != _cached_enabled or fog_begin != _cached_begin or fog_end != _cached_end:
		_apply(enabled, fog_begin, fog_end)


## Apply fog parameters to the Environment resource.
func _apply(enabled: bool, fog_begin: int, fog_end: int) -> void:
	_environment.fog_enabled = enabled
	if enabled:
		_environment.fog_mode = Environment.FOG_MODE_DEPTH
		_environment.fog_light_color = FOG_COLOR
		# In depth mode, density is the max opacity at fog_depth_end (0–1).
		# 1.0 = fully opaque at end distance.
		_environment.fog_density = 1.0
		_environment.fog_depth_begin = fog_begin * VOXEL_SIZE
		_environment.fog_depth_end = fog_end * VOXEL_SIZE
		_environment.fog_depth_curve = 1.0

	_cached_enabled = enabled
	_cached_begin = fog_begin
	_cached_end = fog_end
