## Renders in-flight projectiles as thin elongated cylinders oriented along
## their velocity vector.
##
## Each frame, reads projectile positions and velocities from SimBridge and
## places a CylinderMesh at each one, rotated to point along the flight
## direction. The cylinder naturally tips downward as gravity curves the
## trajectory.
##
## Uses a pool pattern: MeshInstance3D nodes are created on demand (never
## destroyed), and excess nodes are hidden when the projectile count drops.
## All projectiles share the same mesh and material.
##
## Projectile positions are interpolated between sim ticks using the velocity
## vector: render_pos = sim_pos + velocity * fractional_tick_offset. This
## interpolation is done on the Rust side in get_projectile_positions().
##
## See also: sim_bridge.rs for get_projectile_positions() and
## get_projectile_velocities(), projectile.rs for SubVoxelCoord conversion,
## main.gd which creates this node and calls setup() and set_render_tick().

extends Node3D

const ARROW_RADIUS := 0.03
const ARROW_LENGTH := 0.6

var _bridge: SimBridge
var _mesh: CylinderMesh
var _material: StandardMaterial3D
var _instances: Array[MeshInstance3D] = []
var _render_tick: float = 0.0


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	_material = StandardMaterial3D.new()
	_material.albedo_color = Color(0.45, 0.30, 0.15)  # Brown wood
	_mesh = CylinderMesh.new()
	_mesh.top_radius = ARROW_RADIUS
	_mesh.bottom_radius = ARROW_RADIUS
	_mesh.height = ARROW_LENGTH
	_mesh.radial_segments = 4
	_mesh.rings = 0
	_mesh.material = _material


## Set the fractional render tick for smooth position interpolation.
## Called by main.gd each frame after stepping the sim.
func set_render_tick(tick: float) -> void:
	_render_tick = tick


func _process(_delta: float) -> void:
	if _bridge == null or not _bridge.is_initialized():
		return

	var positions := _bridge.get_projectile_positions(_render_tick)
	var velocities := _bridge.get_projectile_velocities()
	var count := positions.size()

	# Grow pool if needed.
	while _instances.size() < count:
		var inst := MeshInstance3D.new()
		inst.mesh = _mesh
		add_child(inst)
		_instances.append(inst)

	# Update transforms and hide excess.
	for i in _instances.size():
		if i < count:
			_instances[i].visible = true
			var pos := positions[i]
			var vel := velocities[i] if i < velocities.size() else Vector3.FORWARD
			# Orient cylinder along velocity vector.
			# CylinderMesh is Y-aligned by default; we need to rotate it to
			# point along the velocity direction.
			if vel.length_squared() > 0.0001:
				var dir := vel.normalized()
				# Build a transform looking along the velocity direction.
				# look_at gives us -Z along dir, but cylinder is Y-aligned,
				# so we rotate the basis by 90 degrees around X.
				var target := pos + dir
				var up := Vector3.UP
				# Avoid degenerate up vector when flying straight up/down.
				if abs(dir.dot(Vector3.UP)) > 0.99:
					up = Vector3.FORWARD
				var t := Transform3D()
				t = t.looking_at(target - pos, up)
				# Rotate 90 degrees around local X so the cylinder (Y-axis)
				# aligns with the look direction (-Z axis).
				t.basis = t.basis * Basis(Vector3.RIGHT, -PI / 2.0)
				t.origin = pos
				_instances[i].transform = t
			else:
				_instances[i].global_position = pos
		else:
			_instances[i].visible = false
