# F-placement-ui — Construction Placement UX Draft

Design draft for the construction placement UI. Different construction types
need different interaction models: platforms are placed freely in 3D space on
a height slice, while buildings snap to solid surfaces. This document describes
the goal-level UX for each mode, followed by implementation details.

### Shared rules

These apply to all placement modes:

- **Structural integrity warnings.** Every mode shows structural integrity
  warnings in the preview step, using the existing three-tier system: Ok
  (blue ghost), Warning (orange ghost, confirm allowed), Blocked (red ghost,
  confirm disabled). "Blocked" means a structural violation that prevents
  confirmation. "Warning" means the placement is structurally risky but
  allowed.
- **Confirm or cancel.** All modes use confirm via the on-screen Confirm
  button or Enter. Cancel via the Cancel button or Escape. Left-clicking the
  world does NOT confirm — only Enter and the Confirm button do.

---

## Platform Placement

Platforms are placed on a horizontal slice at the camera's current orbit height
(Y-level).

### Height-slice wireframe grid

When in platform placement mode, the game renders ghostly wireframe cube
outlines on the voxel grid at the current orbit height. Two visibility rules:

1. **Radial falloff from the orbit point** — cubes near the camera's focus
   point are most opaque, fading to fully transparent further out. This keeps
   the grid localized and uncluttered.
2. **Extra transparency over solid voxels** — wireframe cubes that overlap
   wood, trunk, branch, etc. dim significantly so they don't obscure the tree,
   but remain faintly visible.

The net effect is a gentle, context-aware height indicator that shows the
player what Y-level they're editing without visual noise.

### Interaction flow

1. **Hover** — the voxel under the mouse cursor highlights on the current
   height slice, making it clear where a click would start.
2. **Click and drag** — click sets one corner of a rectangular region; drag
   extends the highlight to the opposite corner in real time.
3. **Release** — locks in the rectangle and shows a blueprint preview.

WASD camera movement is not disabled during drag. The drag endpoint is always
determined by the mouse pointer's projection onto the world grid, not by the
camera focus position. If the player holds the mouse button, keeps the mouse
still, then WASDes the camera elsewhere, the mouse now projects to a different
world position, so releasing produces a rectangle spanning from the drag start
to wherever the mouse currently projects.

**Future:** A toggle option will allow circular/oval platform shapes in
addition to rectangular. Not yet designed.

## Building Placement

Buildings sit on solid surfaces (wood or dirt). The camera's orbit height is
irrelevant — placement raycasts from the mouse into the 3D scene to find the
first solid surface and snaps the footprint to the top of that voxel. There is
no height-slice wireframe grid.

Note: buildings can only be placed on flat terrain — all footprint voxels must
be at the same Y-level. On uneven surfaces (tree branches, roots), the drag
will freeze as soon as the mouse moves to a voxel at a different height. This
is intentionally restrictive for the first implementation.

### Interaction flow

1. **Hover** — the voxel atop the solid surface under the mouse cursor
   highlights, regardless of height level.
2. **Click and drag** — defines the building footprint rectangle on the
   surface, with the same real-time highlight as platform mode. If the mouse
   moves off even terrain (e.g. the surface height changes mid-drag), the drag
   stops extending; releasing the mouse uses the most recent valid position.
3. **Minimum size** — the footprint must be at least 3x3 horizontally. If
   smaller, an error message appears and confirmation is disabled.
4. **Height controls** — the player can adjust building height via +/- buttons
   in the construction panel (available in PREVIEW). A height of 1 shows a
   warning ("elves dislike short buildings") but is still allowed.

**Deferred:** Player-controlled door orientation is tracked as F-building-door.

## Ladder Placement

Ladders are vertical 1x1 columns. Placement uses surface raycasting to pick
a start point, then the player points at a destination surface to set the
vertical extent.

### Interaction flow

1. **Click (start point)** — raycast to find a valid anchor. Valid anchors are
   the top of a solid block (ladder starting from the ground or a platform) or
   the side of a solid block (ladder attached to a wall). Clicking on an
   invalid surface (open air, leaves, etc.) does nothing. "Solid" means
   `VoxelType::is_solid()` — wood, dirt, trunk, branch, etc.
2. **Drag (end point)** — point at a destination surface to set where the
   ladder reaches. Only the Y-coordinate of the hit surface matters — the
   ladder column's X/Z are locked to `_drag_start.x/z` regardless of where
   the endpoint ray hits horizontally. The end point must be a solid surface
   (top or side). If the mouse moves off a valid surface, the drag stops
   extending; releasing uses the most recent valid position. The wireframe
   grid is not shown during ladder placement (neither hover nor drag), since
   the mouse hits different surfaces at different heights and the grid would
   flicker between Y-levels. The ghost column itself provides sufficient
   height feedback.
3. **Release** — locks in the ladder column and shows a blueprint preview.
4. **Orientation** — defaults to "Auto", which picks the facing that maximizes
   the number of ladder voxels adjacent to a solid surface (so a ladder up a
   trunk wall automatically faces the trunk, a ladder to a platform lip faces
   the platform edge, etc.). The orientation selector in the panel cycles
   through five options: Auto, East, South, West, North. During drag, if set
   to Auto, the ghost faces the start-click surface as a visual hint; the
   final auto-orientation is computed on entering PREVIEW and recomputed if
   the player adjusts height.
5. **Material** — toggle between wood and rope via the construction panel.

## Carve Placement

Carving removes voxels from a 3D rectangular prism. It reuses the platform
mode's height-slice wireframe grid and extends the interaction into 3D by
letting the player adjust the camera height during the drag.

### Interaction flow

1. **Hover** — same as platform mode: the voxel under the cursor highlights on
   the current height slice, with the wireframe grid visible.
2. **Click and drag** — click sets one corner of the horizontal rectangle on
   the current height slice. Drag extends to the opposite corner. While holding
   the mouse button, the player can raise or lower the camera height via
   keyboard controls to sweep the selection vertically. The wireframe grid
   updates to show the current height slice as the camera moves. WASD camera
   movement is not disabled during drag — the drag endpoint is always
   determined by the mouse pointer's projection onto the world grid. If the
   player holds the mouse button, keeps the mouse still, then WASDes the
   camera elsewhere, the mouse now projects to a different world position,
   so releasing produces a rectangle spanning from the drag start to wherever
   the mouse currently projects.
3. **Release** — locks in the full 3D rectangular prism and shows a blueprint
   preview of all voxels that would be removed across every Y-level in the
   range. The player can freely orbit and zoom the camera to inspect the
   selection from any angle before confirming.
4. **Height adjustment in PREVIEW** — if the player wants to adjust the
   vertical extent after releasing, +/- height controls in the panel allow
   extending or shrinking the prism vertically (top edge only). This avoids
   requiring the player to perfectly nail the camera height during drag.

---

## Implementation Plan

This section describes the implementation changes needed to support the UX
described above. The current `construction_controller.gd` uses a camera-focus
model (ghost follows `camera.get_focus_voxel()`, dimensions set via +/- buttons).
The overhaul replaces this with mouse-driven click-drag for all modes.

### Overview of changes

**New GDScript files:**
- `height_grid_renderer.gd` — renders the wireframe height-slice grid overlay.

**Modified GDScript files:**
- `construction_controller.gd` — major rework: new state machine with drag
  phases, mouse raycasting, per-mode input handling.
- `orbital_camera.gd` — horizontal voxel snap is removed entirely. A new
  `set_vertical_snap(enabled: bool)` method controls vertical-only snap
  (snapping the camera's Y to integer voxel heights). When enabled and no
  vertical input is held, the camera's Y lerps to the nearest integer + 0.5
  (voxel center) using exponential decay, reusing the existing snap lerp
  speed. This ensures crisp Y-levels for platform/carve placement. The
  construction controller calls `set_vertical_snap(true)` when entering
  platform or carve mode, `set_vertical_snap(false)` otherwise. The existing
  `set_voxel_snap()` method is removed. `get_focus_voxel()` remains and
  continues to return a full `Vector3i` (floor of all three axes); only Y is
  snapped, but all three components are still available.
- `selection_controller.gd` — add a check for construction drag state
  (similar to the existing `placement_controller.is_placing()` check) to
  suppress click-to-select during construction HOVER/DRAGGING/PREVIEW.
  The construction controller exposes `is_placing() -> bool` (true in HOVER,
  DRAGGING, or PREVIEW states). Wired via a new
  `set_construction_controller()` method on selection_controller, called
  from `main.gd` alongside the existing `set_placement_controller()`.
- `main.gd` — creates `height_grid_renderer.gd`, passes it to the
  construction controller via `set_height_grid_renderer()`. The construction
  controller calls `grid.set_visible(true/false)` on mode transitions.

**New Rust (SimBridge) methods:**
- `raycast_solid(origin, dir) -> Dictionary` — DDA voxel raycast that returns
  the first solid voxel hit and the face that was hit (for determining "top of"
  vs "side of" a block). Returns `{hit: true, voxel: Vector3i, face: int}` or
  `{hit: false}`. Based on `raycast_structure()`'s DDA but extended to track
  the entry face (see Rust-side changes section below).
- `get_voxel_solidity_slice(y, cx, cz, radius) -> PackedByteArray` — returns
  a square grid of solid/air flags for voxels at height Y, centered on
  `(cx, cz)`, extending `radius` voxels in each direction. The returned array
  is `(2*radius+1)^2` bytes in row-major order (X varies fastest). Index
  `(x - (cx - radius)) + (z - (cz - radius)) * side_len` gives the flag for
  voxel `(x, y, z)`. Value 1 = solid, 0 = non-solid. All parameters are
  integers (voxel coordinates).
- `auto_ladder_orientation(x, y, z, height) -> int` — counts adjacent solid
  voxels for each of the 4 cardinal orientations along the ladder column and
  returns the best one.

**No changes needed:**
- Existing `validate_*_preview()` methods and `designate_*()` commands are
  reused as-is. The input model changes (mouse-driven rectangles instead of
  camera-focus + dimension buttons), but the validation and command APIs stay
  the same.
- Existing `designate_build_rect()`, `designate_building()`,
  `designate_ladder()`, `designate_carve()` commands are unchanged.
- The anchor passed to all `designate_*` and `validate_*` calls is the
  min-corner of the AABB (i.e. `(min_x, y, min_z)`), not the drag start
  point. This matches the current convention.
- Existing signals (`construction_mode_entered`, `construction_mode_exited`,
  `blueprint_placed`) are preserved. `main.gd` connects to these for panel
  visibility, selection controller integration, and renderer refresh.

### State machine rework (`construction_controller.gd`)

Replace the current two-level state (ACTIVE/PLACING) with a five-state model:

```
INACTIVE → ACTIVE → HOVER → DRAGGING → PREVIEW → (confirm → HOVER, cancel → HOVER)
```

- **INACTIVE** — construction panel hidden, no grid, no mouse handling.
- **ACTIVE** — panel visible with mode buttons (Platform/Building/Ladder/Carve).
  No placement sub-mode selected yet.
- **HOVER** — a mode is selected. Mouse position maps to a highlighted voxel.
  For platform/carve: project mouse ray onto the Y-level plane (camera focus
  height) to get (X, Z). For building/ladder: raycast to solid surface via
  `bridge.raycast_solid()`. The wireframe grid is visible for platform/carve
  modes (not building or ladder hover). Single-voxel highlight follows the
  mouse. If the mouse is over UI or the projection/raycast yields no valid
  position, no highlight is shown and left-click is ignored (no drag starts
  from an invalid position).
- **DRAGGING** — mouse button is held. `_drag_start: Vector3i` records the
  first voxel, `_drag_current: Vector3i` tracks the current endpoint, and
  `_drag_start_face: int` records the face of the surface that was clicked
  (used for ladder auto-orientation hint during drag). As the mouse moves,
  the highlight extends to cover the rectangle (or column for ladders) from
  `_drag_start` to `_drag_current`. For carve mode, camera height changes
  during drag extend the Y range via `_drag_y_start`/`_drag_y_current`.
  Validation runs continuously on the current selection and updates ghost
  color.
- **PREVIEW** — mouse released. The selection is locked. Ghost mesh shows the
  full blueprint with structural integrity coloring. Mode-specific controls
  are active: building height +/-, carve height +/-, ladder orientation cycle
  (Auto/E/S/W/N), ladder material (Wood/Rope). Confirm button enabled if
  validation tier is not Blocked. Enter or Confirm button commits; Escape or
  Cancel button returns to HOVER. Left-clicking the world does nothing in
  PREVIEW (this is a deliberate change from the current code, which confirms
  on left-click). After confirming, the player returns to HOVER in the same
  mode, ready for another placement — this supports rapid repeated placement
  without re-selecting the mode.

**Input mapping per phase:**

| Input          | ACTIVE                   | HOVER                    | DRAGGING                 | PREVIEW               |
|----------------|--------------------------|--------------------------|--------------------------|------------------------|
| Mouse move     | No effect                | Update highlight voxel   | Extend drag rectangle    | No effect             |
| Left click     | No effect                | Start drag               | —                        | No effect (world)     |
| Left release   | —                        | —                        | End drag → PREVIEW       | —                     |
| Right click    | No effect                | Exit to ACTIVE           | Cancel drag → HOVER      | Cancel → HOVER        |
| ESC            | Deactivate → INACTIVE    | Exit to ACTIVE           | Cancel drag → HOVER      | Cancel → HOVER        |
| Enter          | —                        | —                        | —                        | Confirm (if valid)    |
| PgUp/PgDn      | —                        | Change height slice      | (carve) extend Y range   | —                     |
| P/G/L/C        | Enter HOVER for mode     | Switch mode              | Cancel → switch mode     | Cancel → switch mode  |

**Mode-switch during drag or preview:** Pressing a mode-switch key (P for
platform, G for building, L for ladder, C for carve) during DRAGGING or
PREVIEW cancels the current operation and switches to the new mode's HOVER
state.

**Hover computation:** The hover position is computed in `_process()` by
polling `get_viewport().get_mouse_position()` each frame, consistent with the
existing `placement_controller.gd` pattern. The `_unhandled_input()` method
handles discrete events (mouse press/release, key presses). The hover
highlight is suppressed when `get_viewport().gui_get_hovered_control() != null`
(mouse is over UI).

### Mouse-to-voxel projection

Two projection models, selected by mode:

**Height-slice projection (platform, carve):**
- Cast a ray from the camera through the mouse position.
- Intersect the ray with the horizontal plane at `Y = camera_focus_y + 0.5`
  (the center of the voxel layer at the camera's focus height).
- The intersection point gives floating-point (X, Z); floor both to get the
  voxel coordinate. Combined with the camera's integer Y, this gives a full
  `Vector3i`.
- This is a simple plane-ray intersection — no bridge call needed, pure
  GDScript math.
- Guard against degenerate cases: if `abs(ray_dir.y) < 0.001` (ray nearly
  parallel to the plane) or `t < 0` (plane is behind the camera), treat as
  no valid hover position.
- Clamp the result to world bounds `[0, size_x)` x `[0, size_z)` to avoid
  ghost meshes extending outside the world.

**Surface raycast (building, ladder):**
- Call `bridge.raycast_solid(origin, dir)` with the mouse ray.
- The result includes the hit voxel and the face index (0–5). "Top of a solid
  block" means face=PosY (2); the placement voxel is one above the hit voxel.
  "Side of a solid block" means a horizontal face; the placement voxel is the
  air voxel adjacent to that face.
- For buildings: only accept PosY (top) hits where `is_solid()` is true. The
  placement anchor is `(hit.x, hit.y, hit.z)` — the solid voxel itself, since
  `validate_building_preview` expects the foundation row at anchor Y (interior
  starts at `y + 1`).
- For ladders: accept PosY (top) or any horizontal face. The start anchor is
  the air voxel adjacent to the hit face. The ladder column occupies that X/Z
  position from the start Y upward (or downward, whichever direction the
  endpoint defines).

### Drag rectangle computation

During DRAGGING, maintain `_drag_start: Vector3i` and `_drag_current: Vector3i`.

**Y-lock for platform and building modes:** The drag Y is locked to
`_drag_start.y` for the entire drag. If the player changes the camera height
(PgUp/PgDn) during a platform drag, the wireframe grid updates to show the
new height slice, but the drag rectangle stays at the original Y. The
height-slice projection always uses `_drag_start.y` (not the current camera Y)
for computing the intersection plane during drag. For buildings, the surface
raycast already constrains to the same Y via the "stops on invalid terrain"
rule. Carve mode is the exception — it tracks camera Y changes to extend the
vertical range.

The rectangle is the axis-aligned bounding box:

```gdscript
var min_x := mini(_drag_start.x, _drag_current.x)
var max_x := maxi(_drag_start.x, _drag_current.x)
var min_z := mini(_drag_start.z, _drag_current.z)
var max_z := maxi(_drag_start.z, _drag_current.z)
var width := mini(max_x - min_x + 1, 10)
var depth := mini(max_z - min_z + 1, 10)
```

**Per-dimension cap:** Each dimension (width, depth, height) is capped at 10
voxels during drag. If the drag extends beyond 10 in one dimension, that
dimension clamps to 10 but the other dimensions are unaffected. For example,
a 12x6 drag produces a 10x6 rectangle. The clamping is applied when computing
the rectangle from `_drag_start` and `_drag_current`, not by restricting mouse
movement.

For carve mode, also track `_drag_y_start: int` and `_drag_y_current: int`.
`_drag_y_start` is set to `floor(camera_focus_y)` when the drag begins.
`_drag_y_current` is updated continuously during the drag to
`floor(camera_focus_y)` — the integer voxel Y derived from the camera's float
position:

```gdscript
var min_y := mini(_drag_y_start, _drag_y_current)
var max_y := maxi(_drag_y_start, _drag_y_current)
var height := mini(max_y - min_y + 1, 10)
```

For ladders, the drag defines a vertical column: the X/Z stay at
`_drag_start.x/z`, and the height is derived from the Y-coordinates of the
start and end surface hits:
`mini(abs(_drag_current.y - _drag_start.y) + 1, 10)`.
The anchor Y is `min(_drag_start.y, _drag_current.y)`.

### "Stops on invalid terrain" behavior

For building and ladder modes, the drag can encounter invalid mouse positions
(open air, different surface height, leaves). The rule is:

- Track `_last_valid_drag: Vector3i` — updated only when the current mouse
  position is valid for the mode.
- On each mouse move, attempt projection. If valid, update `_drag_current`
  and `_last_valid_drag`. If invalid, `_drag_current` stays at
  `_last_valid_drag`.
- For buildings: "valid" means the raycast hit a PosY face on solid wood/dirt
  at the same Y-level as `_drag_start.y`.
- For ladders: "valid" means the raycast hit any solid surface.
- If the raycast hits nothing (open sky, out of bounds), treat it as an invalid
  position — the hover highlight and drag preview show the last valid position.
  In HOVER state with no valid position yet, no highlight is shown.

This also applies in HOVER state (not just DRAGGING): if the mouse is over
open sky or an invalid surface, no highlight is shown.

### Height-slice wireframe grid (`height_grid_renderer.gd`)

A new node (extends `Node3D`) that renders the wireframe overlay. Created by
`main.gd` and given references to the camera and bridge.

**Geometry approach:** Generate an `ImmediateMesh` each time the grid needs
updating. The grid is rebuilt only when the integer Y-level or the integer
camera focus (X, Z) changes — not every frame. Track `_last_grid_y: int`,
`_last_grid_cx: int`, `_last_grid_cz: int` for change detection. For each
voxel in a square region around the camera focus point (±15 voxels in X/Z),
emit 12 line segments forming a wireframe cube at `(x, y, z)` where `y` is
the camera's focus height.

**Per-cube opacity:** Each cube's vertex color alpha is computed as:

```
base_alpha = max(0, 1.0 - distance / max_radius)   # radial falloff
if voxel_is_solid:
    alpha = base_alpha * solid_dim_factor            # e.g. 0.15
else:
    alpha = base_alpha
```

The `voxel_is_solid` check uses `bridge.get_voxel_solidity_slice()` — fetched
when the grid is rebuilt (same change-detection as the mesh), not per-cube or
per-frame.

**Material:** A single `StandardMaterial3D` with `TRANSPARENCY_ALPHA`,
`vertex_color_use_as_albedo = true`, `no_depth_test = true`,
`shading_mode = UNSHADED`. Line color: a soft white or pale blue.

**Visibility:** The grid renderer is enabled by `construction_controller.gd`
when entering platform or carve HOVER, and disabled when entering building or
ladder HOVER, or when leaving construction mode entirely. The grid is not shown
during ladder mode (the ghost column provides height feedback instead).

**Performance:** A ±15 radius is 31x31 = 961 cubes x 12 lines = ~11.5K line
segments. Rebuilds only happen when the integer voxel position changes (not
every frame), so the cost is negligible.

### Panel click exclusion

The hover highlight is hidden when the mouse is over the construction panel.
Clicks on the panel interact with panel buttons, not the world. This is
naturally handled by `_unhandled_input` for clicks, but the hover highlight
(computed in `_process`) must check whether the mouse is over a UI control
before showing — use `get_viewport().gui_get_hovered_control() != null` to
suppress the highlight when the mouse is over any UI element.

### Ghost mesh in DRAGGING and PREVIEW phases

The existing ghost (`BoxMesh` + `StandardMaterial3D`) is reused. Resizing
uses `mesh.size` directly (same as the current code) — this is simpler than
transform scaling and avoids complications with combined rotation + non-uniform
scale for ladders:

- **Platform/building/carve:** set `mesh.size = Vector3(width, h, depth)` and
  reposition to the center of the bounding box.
- **Ladder:** set `mesh.size = Vector3(0.9, height, 0.05)` and rotate to match
  orientation via `_face_rotations`. During drag, if orientation is set to
  Auto, the ghost faces the surface that was clicked for the start point
  (stored as `_drag_start_face: int`) as a visual hint.
- **Color:** same tier-based coloring as today (blue=Ok, orange=Warning,
  red=Blocked). Validation runs when drag inputs change (change-detection on
  the integer voxel coordinates, not every frame).

In PREVIEW phase, the ghost is frozen at the final drag dimensions. The player
can adjust height/orientation/material via panel controls, which update the
ghost and re-run validation. For ladders, changing height with Auto orientation
recomputes the auto-orientation.

### Panel UI changes

The dimension +/- buttons (Width, Depth) are removed for all modes — dimensions
come from the drag. Mode-specific PREVIEW controls:

- **Platform:** no controls (dimensions fully determined by drag).
- **Building:** Height +/- (range 1–5).
- **Carve:** Height +/- (adjusts top edge of the prism, range 1–10).
- **Ladder:** Orientation cycle (Auto → East → South → West → North → Auto),
  Material toggle (Wood/Rope).

The Confirm and Cancel buttons appear in PREVIEW phase only.

### Rust-side changes

**`raycast_solid()` (sim.rs + sim_bridge.rs):**

Add a new method `raycast_solid()` on `SimState` based on the existing DDA
traversal in `raycast_structure()`. The key difference: it tracks the entry
face by recording `min_axis` and `step[min_axis]` at each DDA step. When a
solid voxel is hit, the entry face is computed as:

```
face = match (min_axis, step[min_axis] > 0) {
    (0, true)  => 1,  // entered through NegX face
    (0, false) => 0,  // entered through PosX face
    (1, true)  => 3,  // entered through NegY face
    (1, false) => 2,  // entered through PosY face
    (2, true)  => 5,  // entered through NegZ face
    (2, false) => 4,  // entered through PosZ face
}
```

Returns `Option<(VoxelCoord, u8)>` — the solid voxel and the face the ray
entered through. The bridge wraps this as a `Dictionary` for GDScript and
hardcodes `max_steps = 500` (same as `raycast_structure`).

**`get_voxel_solidity_slice()` (sim_bridge.rs):**

Query the world grid for a square region at a given Y-level, centered on
`(cx, cz)` with the given integer radius. Returns a `PackedByteArray` of
`(2*radius+1)^2` bytes in row-major order (X varies fastest). Index formula:
`(x - (cx - radius)) + (z - (cz - radius)) * (2*radius+1)`. Value 1 = solid,
0 = non-solid. Out-of-bounds voxels return 0.

**`auto_ladder_orientation()` (sim.rs + sim_bridge.rs):**

For each of the 4 cardinal orientations, count how many voxels in the ladder
column (at `x, y..y+height, z`) have a solid neighbor in that direction.
Return the orientation with the highest count (tie-break: first in
`LADDER_ORIENTATIONS` order, i.e. East → South → West → North).
