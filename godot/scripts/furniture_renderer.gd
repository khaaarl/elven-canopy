## Renders furniture items inside buildings using MultiMesh instances.
##
## Each furniture kind (Bed, Bench, Counter, Shelf, Table, Workbench) gets its
## own MultiMeshInstance3D with a distinct box size and color. The data comes
## from bridge.get_furniture_positions() which returns flat (x, y, z, kind)
## quads. All instances are rebuilt each time refresh() is called.
##
## Furniture kind discriminants (from FurnitureKind in types.rs):
##   0 = Bed, 1 = Bench, 2 = Counter, 3 = Shelf, 4 = Table, 5 = Workbench
##
## See also: building_renderer.gd for the MultiMesh pattern,
## sim_bridge.rs for get_furniture_positions(),
## building.rs for furniture position computation,
## main.gd which creates this node and calls refresh().

extends Node3D

## Per-kind mesh sizes (Vector3) and colors (Color), indexed by kind int.
## 0=Bed, 1=Bench, 2=Counter, 3=Shelf, 4=Table, 5=Workbench
const KIND_COUNT := 6

var _bridge: SimBridge
var _instances: Array = []
var _sizes: Array = [
	Vector3(0.8, 0.15, 0.4),  # Bed
	Vector3(0.8, 0.20, 0.3),  # Bench
	Vector3(0.9, 0.40, 0.5),  # Counter
	Vector3(0.3, 0.60, 0.8),  # Shelf
	Vector3(0.7, 0.30, 0.7),  # Table
	Vector3(0.8, 0.35, 0.5),  # Workbench
]
var _colors: Array = [
	Color(0.55, 0.35, 0.18),  # Bed: warm brown (#8C5A2F approx)
	Color(0.29, 0.19, 0.13),  # Bench: dark (#4A3020)
	Color(0.66, 0.56, 0.50),  # Counter: stone (#A89080)
	Color(0.72, 0.56, 0.38),  # Shelf: pale (#B89060)
	Color(0.36, 0.23, 0.10),  # Table: dark oak (#5C3A1A)
	Color(0.48, 0.23, 0.16),  # Workbench: reddish (#7A3B2A)
]


func setup(bridge: SimBridge) -> void:
	_bridge = bridge
	refresh()


func refresh() -> void:
	# Remove previous instances.
	for inst in _instances:
		inst.queue_free()
	_instances.clear()

	var data := _bridge.get_furniture_positions()
	var item_count := data.size() / 4
	if item_count == 0:
		return

	# Group positions by kind.
	var by_kind: Array = []
	for _k in KIND_COUNT:
		by_kind.append([])

	for i in item_count:
		var base := i * 4
		var x := float(data[base])
		var y := float(data[base + 1])
		var z := float(data[base + 2])
		var kind := data[base + 3]
		if kind >= 0 and kind < KIND_COUNT:
			by_kind[kind].append(Vector3(x, y, z))

	# Create one MultiMeshInstance3D per kind that has items.
	for kind in KIND_COUNT:
		var positions: Array = by_kind[kind]
		if positions.is_empty():
			continue

		var mat := StandardMaterial3D.new()
		mat.albedo_color = _colors[kind]

		var sz: Vector3 = _sizes[kind]
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
		inst.name = "Furniture_%d" % kind
		add_child(inst)
		_instances.append(inst)
