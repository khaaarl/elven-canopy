## SSAO controller — applies screen-space ambient occlusion to the scene.
##
## Reads ssao_enabled from GameConfig each frame and applies it to the
## WorldEnvironment's Environment resource. This polling approach matches
## how fog_controller.gd handles fog — no signals needed.
##
## Uses Godot's built-in SSAO (Environment.ssao_enabled), which is an
## adaptive horizon-based AO implementation. Supported in Forward+ and
## Compatibility renderers, not Mobile.
##
## SSAO parameters (stored in GameConfig):
##   ssao_enabled: bool = false   — master toggle (experimental, off by default)
##
## See also: settings_panel.gd (UI for SSAO toggle), game_config.gd
## (persistence), fog_controller.gd (similar pattern), main.gd (wires
## this controller to WorldEnvironment).

extends Node

## The Environment resource to modify. Set via setup().
var _environment: Environment

## Cached value to avoid redundant Environment writes.
var _cached_enabled: bool = false


## Initialize with the scene's Environment resource. Applies current
## GameConfig value immediately.
func setup(environment: Environment) -> void:
	_environment = environment
	_apply(GameConfig.get_setting("ssao_enabled"))


func _process(_delta: float) -> void:
	if _environment == null:
		return
	var enabled: bool = GameConfig.get_setting("ssao_enabled")
	if enabled != _cached_enabled:
		_apply(enabled)


## Apply SSAO state to the Environment resource.
func _apply(enabled: bool) -> void:
	_environment.ssao_enabled = enabled
	if enabled:
		# EXPERIMENTAL — cranked up for visibility testing. Do not merge these
		# values to main. Default ssao_light_affect is 0.0 (ambient only), which
		# is nearly invisible with our low ambient_light_energy. Setting it to
		# 1.0 makes SSAO darken direct light too. High intensity + radius make
		# the effect unmissable so we can evaluate whether SSAO is worth keeping.
		_environment.ssao_radius = 0.5  # default 1.0 — small to catch chamfer-scale crevices
		_environment.ssao_intensity = 5.0  # default 2.0
		_environment.ssao_light_affect = 1.0  # default 0.0
	_cached_enabled = enabled
