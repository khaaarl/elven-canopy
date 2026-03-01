## Renders beds as small BoxMesh items inside buildings using MultiMesh.
##
## Each bed is a small box (0.8 x 0.15 x 0.4 units, warm brown) placed at
## each bed voxel position, offset to sit on the floor. Uses a single
## MultiMeshInstance3D for all beds across all structures, rebuilt each time
## refresh() is called.
##
## Gets data from bridge.get_bed_positions() which returns flat (x,y,z)
## triples of placed beds. Called from main.gd _process() alongside other
## renderer refreshes.
##
## See also: building_renderer.gd for the MultiMesh pattern,
## sim_bridge.rs for get_bed_positions(),
## building.rs for bed position computation,
## main.gd which creates this node and calls refresh().

extends Node3D

var _bridge: SimBridge
var _instance: MultiMeshInstance3D


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	refresh()


func refresh() -> void:
	# Remove previous instance.
	if _instance:
		_instance.queue_free()
		_instance = null

	var data := _bridge.get_bed_positions()
	var bed_count := data.size() / 3
	if bed_count == 0:
		return

	# Warm brown wood color for beds.
	var mat := StandardMaterial3D.new()
	mat.albedo_color = Color(0.55, 0.35, 0.18)

	var box := BoxMesh.new()
	box.size = Vector3(0.8, 0.15, 0.4)
	box.material = mat

	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = box
	mm.instance_count = bed_count

	for i in bed_count:
		var idx := i * 3
		var x := float(data[idx])
		var y := float(data[idx + 1])
		var z := float(data[idx + 2])
		# Position: center of voxel (+0.5), bed sits on the floor (+0.075 for half height).
		var pos := Vector3(x + 0.5, y + 0.075, z + 0.5)
		mm.set_instance_transform(i, Transform3D(Basis.IDENTITY, pos))

	_instance = MultiMeshInstance3D.new()
	_instance.multimesh = mm
	_instance.name = "Beds"
	add_child(_instance)
