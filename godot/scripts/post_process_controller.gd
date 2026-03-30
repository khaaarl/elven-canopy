## Post-process controller — combined edge outline and depth fog.
##
## Manages a single full-screen spatial shader that applies edge detection
## (Sobel on depth/normal buffers) followed by depth-based fog. This ordering
## ensures edges are drawn on the raw scene colors and then fade into fog at
## distance, rather than darkening already-fogged pixels.
##
## Replaces the separate fog_controller.gd — Godot's built-in Environment fog
## is disabled, and fog is applied by our shader instead. This gives us full
## control over the compositing order (edges first, then fog).
##
## Reads edge_outline, fog_enabled, fog_begin, and fog_end from GameConfig
## each frame (polling pattern). The shader is always visible when either
## effect is enabled; it hides only when both are off.
##
## Uses a spatial shader (not canvas_item) because depth and normal-roughness
## textures are only accessible from spatial shaders in Godot 4.
##
## See also: post_process.gdshader (the shader), settings_panel.gd (UI toggles),
## game_config.gd (persistence).

extends Node

## Meters per voxel — voxel coords to Godot world units.
const VOXEL_SIZE := 2.0

## The MeshInstance3D that carries the fullscreen shader quad.
var _mesh_instance: MeshInstance3D

## The ShaderMaterial — cached for setting uniforms.
var _material: ShaderMaterial

## The Environment resource — we disable its built-in fog.
var _environment: Environment

## Cached values to avoid redundant uniform writes.
var _cached_edge_enabled: bool = false
var _cached_fog_enabled: bool = false
var _cached_fog_begin: int = 0
var _cached_fog_end: int = 0


## Create the overlay quad and add it as a child of the given camera.
## Also disable built-in Environment fog (we handle it in the shader).
## Call this once from main.gd.
func setup(camera: Camera3D, environment: Environment) -> void:
	_environment = environment
	# Disable Godot's built-in fog — our shader handles it instead.
	_environment.fog_enabled = false

	_mesh_instance = MeshInstance3D.new()

	var quad := QuadMesh.new()
	quad.size = Vector2(2.0, 2.0)
	_mesh_instance.mesh = quad

	var shader := load("res://shaders/post_process.gdshader") as Shader
	_material = ShaderMaterial.new()
	_material.shader = shader
	_material.render_priority = 1
	_mesh_instance.material_override = _material

	# Prevent frustum-culling the quad (its AABB is tiny at the camera
	# origin, but the vertex shader projects it fullscreen).
	_mesh_instance.extra_cull_margin = 16384.0

	camera.add_child(_mesh_instance)

	# Apply initial state.
	_apply(
		GameConfig.get_setting("edge_outline"),
		GameConfig.get_setting("fog_enabled"),
		GameConfig.get_setting("fog_begin"),
		GameConfig.get_setting("fog_end"),
	)


func _process(_delta: float) -> void:
	if _mesh_instance == null:
		return
	var edge_enabled: bool = GameConfig.get_setting("edge_outline")
	var fog_enabled: bool = GameConfig.get_setting("fog_enabled")
	var fog_begin: int = GameConfig.get_setting("fog_begin")
	var fog_end: int = GameConfig.get_setting("fog_end")
	if (
		edge_enabled != _cached_edge_enabled
		or fog_enabled != _cached_fog_enabled
		or fog_begin != _cached_fog_begin
		or fog_end != _cached_fog_end
	):
		_apply(edge_enabled, fog_enabled, fog_begin, fog_end)


## Update shader uniforms and visibility.
func _apply(edge_enabled: bool, fog_enabled: bool, fog_begin: int, fog_end: int) -> void:
	# Hide the mesh entirely when both effects are off (zero GPU cost).
	_mesh_instance.visible = edge_enabled or fog_enabled

	_material.set_shader_parameter("edge_enabled", 1.0 if edge_enabled else 0.0)
	_material.set_shader_parameter("fog_enabled", 1.0 if fog_enabled else 0.0)
	_material.set_shader_parameter("fog_begin", fog_begin * VOXEL_SIZE)
	_material.set_shader_parameter("fog_end", fog_end * VOXEL_SIZE)

	_cached_edge_enabled = edge_enabled
	_cached_fog_enabled = fog_enabled
	_cached_fog_begin = fog_begin
	_cached_fog_end = fog_end
