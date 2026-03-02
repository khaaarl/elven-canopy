## Renders ground piles (item stacks on the forest floor) using MultiMesh boxes.
##
## Each item kind gets its own MultiMeshInstance3D with a distinct box size and
## color. The data comes from bridge.get_ground_piles() which returns a VarArray
## of dicts: {x, y, z, inventory: [{kind: String, quantity: int}]}. All
## instances are rebuilt each time refresh() is called.
##
## Item kind visual properties are stored in _kind_visuals, keyed by the
## display_name string from ItemKind (e.g. "Bread").
##
## See also: furniture_renderer.gd for the MultiMesh pattern,
## sim_bridge.rs for get_ground_piles(),
## main.gd which creates this node and calls refresh().

extends Node3D

var _bridge: SimBridge
var _instances: Array = []

## Visual properties per item kind: {size: Vector3, color: Color}.
var _kind_visuals: Dictionary = {
	"Bread":
	{
		"size": Vector3(0.4, 0.2, 0.4),
		"color": Color(0.85, 0.70, 0.40),
	},
}


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	refresh()


func refresh() -> void:
	# Remove previous instances.
	for inst in _instances:
		inst.queue_free()
	_instances.clear()

	var piles := _bridge.get_ground_piles()
	if piles.size() == 0:
		return

	# Collect positions per item kind.
	var by_kind: Dictionary = {}

	for pile_idx in piles.size():
		var pile: Dictionary = piles[pile_idx]
		var x := float(pile["x"])
		var y := float(pile["y"])
		var z := float(pile["z"])
		var inventory: Array = pile["inventory"]

		for item_idx in inventory.size():
			var item: Dictionary = inventory[item_idx]
			var kind: String = item["kind"]
			if kind not in by_kind:
				by_kind[kind] = []
			by_kind[kind].append(Vector3(x, y, z))

	# Create one MultiMeshInstance3D per kind that has positions.
	for kind in by_kind:
		var positions: Array = by_kind[kind]
		if positions.is_empty():
			continue

		var visuals: Dictionary = _kind_visuals.get(kind, {})
		var sz: Vector3 = visuals.get("size", Vector3(0.3, 0.2, 0.3))
		var col: Color = visuals.get("color", Color(0.6, 0.6, 0.6))

		var mat := StandardMaterial3D.new()
		mat.albedo_color = col

		var box_mesh := BoxMesh.new()
		box_mesh.size = sz
		box_mesh.material = mat

		var mm := MultiMesh.new()
		mm.transform_format = MultiMesh.TRANSFORM_3D
		mm.mesh = box_mesh
		mm.instance_count = positions.size()

		var y_offset := sz.y / 2.0  # Sit on the floor.
		for j in positions.size():
			var p: Vector3 = positions[j]
			var pos := Vector3(p.x + 0.5, p.y + y_offset, p.z + 0.5)
			mm.set_instance_transform(j, Transform3D(Basis.IDENTITY, pos))

		var inst := MultiMeshInstance3D.new()
		inst.multimesh = mm
		inst.name = "GroundPile_%s" % kind
		add_child(inst)
		_instances.append(inst)
