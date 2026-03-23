# F-mobile-support: Mobile/Touch Platform Support

**Phase:** 9
**Status:** Draft design

## Overview

Adapt Elven Canopy for mobile phones and tablets with touch-based input. The
game's RTS-style controls (keyboard shortcuts, right-click context commands,
hover tooltips, mouse drag selection) need complete reimagining for a
touchscreen with no keyboard, no mouse hover, and limited screen real estate.

This document maps every current desktop operation to a mobile equivalent,
organized by system.

---

## 1. Camera Controls

Desktop (orbital_camera.gd):
- WASD: move focal point
- Q/E or Left/Right arrows: rotate
- Up/Down arrows: tilt (pitch)
- Middle-mouse drag: freeform orbit (yaw + pitch)
- Scroll wheel / +/-: zoom
- Page Up/Down: vertical focal point

### Mobile mapping

| Gesture / Control | Action |
|-------------------|--------|
| One-finger drag | Pan focal point (replaces WASD) |
| Two-finger slide (both fingers same direction) | Rotate camera yaw (replaces Q/E) |
| Pinch | Zoom in/out (replaces scroll wheel) |
| Elevation slider (right screen edge) | Move focal point vertically (replaces Page Up/Down) |
| Pitch up/down buttons (near elevation slider) | Nudge camera pitch in discrete steps (replaces Up/Down arrows) |

**Elevation slider:** A dedicated vertical slider on the right edge of the
screen. Shows a stylized cross-section of the tree with a draggable handle
marking the current focal height. Doubles as a visual indicator of "where am
I looking" vertically. Always visible during gameplay — elevation is too
important to hide behind a gesture.

**Camera pitch:** Fixed at a comfortable default angle. Small up/down buttons
near the elevation slider allow discrete pitch adjustment. Free pitch via
gesture is omitted — most mobile strategy games lock pitch, and it avoids
gesture disambiguation problems (tilt vs pinch vs rotate with two fingers).

**Two-finger rotate:** Both fingers slide left or both slide right (a
"two-finger swipe"). This is more comfortable and less error-prone than the
"twist" gesture where fingers orbit around each other, which easily triggers
accidental pinch-zoom.

**Follow mode:** Tap creature portrait in info panel to follow. Any one-finger
pan breaks follow (same as WASD on desktop).

**Edge cases:**
- Disambiguating one-finger pan from tap-to-select requires a drag threshold
  (~10px or ~100ms hold before movement starts registering as pan).
- Two-finger gestures need a gesture recognizer that distinguishes rotate vs
  pinch. With pitch and elevation handled by dedicated UI controls, the
  two-finger gesture space is simpler: only pinch (fingers move apart/together)
  and slide (fingers move same direction). Godot's `InputEventScreenDrag` with
  multi-touch tracking can handle this; consider a `GestureRecognizer` utility
  class.

---

## 2. Selection

Desktop (selection_controller.gd):
- Left click: select creature/structure/ground pile
- Shift+click: add/remove from multi-selection
- Left click + drag: box select
- Shift + drag: additive box select
- ESC: deselect all
- F: toggle attack-move mode
- Right-click on hostile: attack
- Right-click on ground/friendly: move-to

### Mobile mapping

| Gesture | Action |
|---------|--------|
| Tap creature/structure/pile | Select (replaces left click) |
| Tap empty ground | Deselect all (replaces ESC for selection) |
| Tap friendly creature while others selected | Toggle add/remove from multi-selection (replaces Shift+click) |
| Lasso button + one-finger drag | Draw freeform selection outline (replaces box select) |
| Double-tap creature | Select all in same military group (replaces F-dblclick-select) |

**Lasso select:** A dedicated "Lasso" button in the command bar. Tap the lasso
icon to enter lasso mode (button highlights to show it's active), then place
a finger on the viewport and drag a freeform shape around the creatures you
want to select. On finger lift, the shape auto-closes (straight line from end
to start point) and all creatures inside are selected. The mode auto-exits
after one use — no need to tap the button again.

**Additive lasso:** If creatures are already selected when you tap the lasso
button, the new lasso adds to the existing selection (like Shift+drag on
desktop). Tapping empty ground still clears everything.

**Visual feedback:** While in lasso mode, a glowing trail follows the finger.
On release, the closed shape briefly flashes, then selected creatures get
their highlight rings.

**Tap-hostile disambiguation:** Tapping a hostile creature while friendlies are
selected issues an attack command (see §3). To *select* a hostile creature
instead (e.g., to view its info), deselect friendlies first by tapping empty
ground, then tap the hostile. This mirrors the desktop pattern where right-click
commands and left-click selection are separate inputs — on mobile, selection
state determines which interpretation wins.

**Selection disambiguation from pan:** A quick tap selects. Any drag pans.
These are the only two one-finger states in normal mode (no long-press
ambiguity — lasso is button-activated).

```
IDLE --tap--> SELECT
IDLE --drag--> PAN
LASSO_MODE --drag--> LASSO_DRAWING --release--> SELECT_LASSO → IDLE
```

### Planned desktop features and mobile equivalents

| Desktop feature | Mobile equivalent |
|-----------------|-------------------|
| F-alt-deselect (Alt+click remove) | Tap selected creature to toggle out |
| F-dblclick-select (double-click same group) | Double-tap creature to select all in same military group |
| F-tab-cycle (Tab through selection) | Swipe left/right on creature info panel to cycle |
| F-selection-groups (Ctrl+number) | Long-press group slot in a groups toolbar to save; tap to recall |
| F-selection-bar (SC2-style bottom bar) | Bottom bar with scrollable portrait strip; especially important on mobile for visible selection state |

---

## 3. Commands (Move, Attack-Move)

Desktop: Right-click dispatches context-sensitive commands. F key toggles
attack-move mode.

### Mobile mapping: Explicit command bar

Since there is no right-click on mobile, commands are issued via explicit
buttons in a **command bar** that appears when friendly creatures are selected:

```
[ Lasso ] [ Move ] [ Attack-Move ]
```

The bar is a horizontal strip near the bottom of the screen, sized for
comfortable tapping. Future commands (Stop, Patrol, Hold Position) add
buttons to the right as they're implemented.

| Button | Behavior |
|--------|----------|
| Lasso | Enter lasso selection mode (see §2). |
| Move | Tap destination on map. Creature(s) move there, ignoring hostiles. |
| Attack-Move | Tap destination. Creature(s) walk there, engaging hostiles en route. |

**Attacking a specific target:** Tap a hostile creature while friendlies are
selected (no button needed — mirrors desktop right-click-on-hostile). This
works regardless of whether a command button is active.

**Flow:** Player selects creatures → taps a command button → taps the map
target. A visual mode indicator (tinted border or icon near finger) shows
which command is active. Tap empty space or the active button again to cancel
command mode without issuing the command.

**Smart defaults:** If no command button is active, tapping the map with
creatures selected defaults to move (tapping ground) or attack (tapping a
hostile). The explicit buttons exist for when you need attack-move
specifically, or want to be unambiguous.

**Command queue (F-command-queue):** A "Queue" toggle button in the bar. When
active, tapped commands append rather than replace. Visual waypoint markers
show the queue.

---

## 4. Construction Mode

Desktop (construction_controller.gd):
- B: enter/exit construction mode
- P/G/L/C: switch sub-mode (Platform/Building/Ladder/Carve)
- Strut: button only
- Left click + drag: designate area
- Enter: confirm placement
- +/- buttons: adjust height
- Rotate button: cycle ladder orientation
- Right-click/ESC: cancel

### Mobile mapping

**Entry:** Tap "Build" button in toolbar (replaces B key).

**Sub-mode selection:** A horizontal mode bar appears:

```
[ Platform ] [ Building ] [ Ladder ] [ Carve ] [ Strut ]
```

Each mode has an icon. Tapping switches mode (replaces P/G/L/C keys).

**Placement:** Touch + drag to designate the area (same as desktop click-drag).
On release, enter preview state showing dimensions and validation tier.

**Preview adjustments:**
- Height +/- buttons remain as on-screen controls
- Ladder orientation: tap a "Rotate" button (same as desktop)
- Confirm: tap a green checkmark button (replaces Enter key)
- Cancel: tap a red X button (replaces Right-click/ESC)

**Height-slice navigation:** Use the elevation slider on the right edge to set
the Y-level for construction, same control as normal camera elevation.

**Construction mode gesture remapping:** While in construction mode, camera
rotation is disabled. The gesture mapping changes as follows:
- **One-finger drag:** Designates the build area (not pan).
- **Two-finger slide (same direction):** Pans camera (replaces rotate, since
  rotate is disabled in construction mode).
- **Pinch:** Zooms camera (unchanged).

This avoids the conflict between rotate and pan — rotate is simply not needed
while placing structures, so two-finger slide is freed up for panning. The
player can look around with two-finger slide + pinch, then use one-finger drag
to designate the build area. Exiting construction mode restores normal gesture
mapping (two-finger slide = rotate, one-finger drag = pan).

---

## 5. Toolbar and Panels

Desktop: Top toolbar with keyboard shortcuts. Right-side info panels with
mutual exclusion. Full-screen overlay panels.

### Mobile mapping

**Top toolbar:** Simplified icon-only bar. Speed controls collapse into a
single button that opens a speed picker popup. Remaining buttons:

```
[ Build ] [ Structures ] [ Units ] [ Military ] [ Tasks ] [ Tree ] [ Help ] [ Menu ]
```

Smaller than desktop — labels hidden, icons only, with tooltip on long-press.

**Speed controls:** Tap the speed indicator (e.g., "2x") to cycle through
Pause / 1x / 2x / 5x. Or tap-and-hold to show a popup picker.

**Right-side panels (creature info, tree info, military):**
On mobile, these become **bottom sheets** that slide up from the bottom edge.
Half-height by default (showing key info), draggable to full-height for
details. Swipe down to dismiss.

- Creature info: name, HP bar, species at half-height. Food and rest gauges
  display at half-height as compact horizontal bars. Follow/Unfollow becomes a
  bottom-sheet button (prominent, since it is the primary way to track a
  creature on mobile). Zoom-to-task is a tap target next to the current task
  label. Military group link is a tappable badge that opens the military bottom
  sheet. Expand for needs, thoughts, inventory.
- Tree info: key stats at half-height. Expand for full breakdown.
- Ground pile info: position and inventory list, displayed as a half-height
  bottom sheet (same slot as creature info).
- Military: group list at half-height. Tap group for detail (pushes a new
  sheet).

### Structure info bottom sheet

Tapping a structure opens a multi-tab bottom sheet with tabs for:
- **Crafting:** Hierarchical recipe browser with search. On mobile, recipe
  categories expand inline rather than using nested panels. Per-recipe output
  targets and auto-logistics toggles use standard mobile form controls (switches,
  steppers).
- **Logistics:** Wants list with add/remove. The wants_editor widget's two-step
  inline picker (kind then material filter) should expand as a full-width
  dropdown within the sheet rather than a small popup, to ensure adequate touch
  target size. Quantity controls use steppers. Priority uses a slider.
- **Furnishing:** Grid of furnishing type icons (7 types). Tap to select.
- **Home assignment:** Elf picker list with search/filter. Tap elf to assign.

Structure name editing uses an inline text field with the on-screen keyboard.
At half-height, the sheet shows the structure name, type, and active tab
summary. Expand to full-height for detailed editing.

**Full-screen overlays (task panel, units panel, structure list):**
Same as desktop but with larger touch targets and swipe-to-dismiss. These
already work well as full-screen overlays. Increase row height to ~48dp for
comfortable tapping.

**Mutual exclusion:** Same rules apply. Opening a bottom sheet closes any
other bottom sheet in the same slot.

---

## 6. Information Display

Desktop:
- Hover tooltips (tooltip_controller.gd)
- Status bar (bottom-left)
- Toast notifications (bottom-right)
- HP bars (on damaged creatures)

### Mobile mapping

| Desktop | Mobile |
|---------|--------|
| Hover tooltip | Tap-and-hold on creature/structure/pile shows tooltip popup. Also shows briefly after selection. |
| Status bar | Same location, slightly larger text for readability. |
| Toast notifications | Same location and behavior. May need larger font. |
| HP bars | Same (world-space, not affected by input changes). |
| Help overlay (? key) | Accessible via the Help toolbar button, showing the gesture reference card. |
| Elfcyclopedia | The elfcyclopedia runs as a localhost HTTP server and opens the system browser. On mobile this works via the system browser, but may need an in-app WebView for better UX (avoids app-switching and keeps the player in context). |

---

## 7. Menus

Desktop (main_menu.gd, escape_menu.gd, new_game_menu.gd):
- Main menu with keyboard shortcuts (N/L/Q)
- Escape menu via ESC
- New game setup with seed input

### Mobile mapping

**Main menu:** Touch-friendly large buttons, no keyboard shortcuts. Same
layout works on mobile with increased button sizes (~60dp height).

**Escape menu:** Accessed via a "Menu" button in the top toolbar (replaces ESC).
Same overlay layout with larger buttons.

**New game:** Seed input uses on-screen keyboard. Tree parameter sliders work
natively with touch. Preset buttons work as-is with larger touch targets.

**Save/Load dialogs:** Same modal approach. File list rows need ~48dp height.
Filename input uses on-screen keyboard. During gameplay, save/load is accessed
through the escape menu's existing Save and Load buttons (replacing the desktop
keyboard shortcuts S and Q).

---

## 8. Debug Tools

Desktop: F12 toggles debug row with spawn buttons and test controls.

### Mobile mapping

Debug tools are development-only and don't need mobile adaptation. If needed
for testing, a three-finger tap could toggle a debug overlay, or debug
controls can live in a submenu of the escape menu.

---

## 9. Minimap (F-minimap, planned)

Desktop design: Bottom-right corner, ~15% viewport height, mouse wheel to
zoom minimap, click to jump camera.

### Mobile mapping

Same position and size ratio. Interactions:
- Tap minimap: jump camera to that location
- Pinch on minimap: zoom minimap levels
- Drag on minimap: pan minimap view
- Minimap gestures must be isolated from main viewport gestures (hit-test
  the minimap region first)

May need to be slightly larger on phone screens (~20% viewport height) for
usable tap targets.

---

## 10. Multiplayer

Desktop: Host/Join menus with keyboard shortcuts, lobby overlay.

### Mobile mapping

Straightforward — these are form UIs that work with touch natively. IP/port
input uses on-screen keyboard. Lobby player list uses standard list layout.
Chat (F-mp-chat, planned) would use a standard mobile chat input pattern
(text field at bottom, messages scrolling above).

---

## 11. ESC Precedence Chain

Desktop: ESC cascades through a priority chain (placement > construction >
selection > panels > escape menu).

### Mobile mapping

There is no ESC key on mobile. Instead:

- **Android back button / iOS swipe-back:** Replaces ESC with the same
  precedence chain logic.
- **Explicit close buttons:** Every panel and mode has a visible close/cancel
  button (X or back arrow), so the user never depends solely on a system
  gesture.
- The back-button handler walks the same precedence chain as desktop ESC.

---

## 12. Screen Layout

**Orientation:** Phone orientation should be locked to landscape during
gameplay. The 3D viewport, elevation slider, command bar, minimap, and bottom
sheets leave almost no usable viewport space in portrait on a phone screen.
Portrait mode may be allowed for menus (main menu, settings) where the layout
is simpler.

### Phone (landscape — required for gameplay)

```
+----------------------------------------------+
| [toolbar icons]                        [Menu] |
|                                           [E] |
|                                           [L] |
|         3D viewport                       [E] |
|                                           [V] |
| [cmd bar]                      [minimap]      |
+----------------------------------------------+
| [bottom sheet - info panel]                   |
+----------------------------------------------+
```

### Tablet (landscape)

Closer to desktop layout. Side panels can remain as side panels rather than
bottom sheets, since there's enough horizontal space. Toolbar can show labels.
Elevation slider remains on right edge.

---

## 13. Gesture Reference Card

| Gesture / Control | Context | Action |
|-------------------|---------|--------|
| Tap | Viewport | Select creature/structure/pile |
| Tap | Empty ground (with selection) | Deselect all |
| Tap | Command mode active | Issue command at location |
| Tap hostile | With friendlies selected | Attack that target (deselect first to inspect instead) |
| Double-tap | Creature | Select all in same military group |
| Tap-and-hold | Object | Show tooltip |
| One-finger drag | Viewport (normal) | Pan camera |
| One-finger drag | Viewport (construction) | Designate build area |
| Lasso button + drag | Viewport | Draw freeform selection outline |
| Two-finger slide (same direction) | Viewport (normal) | Rotate camera yaw |
| Two-finger slide (same direction) | Viewport (construction) | Pan camera (rotate disabled) |
| Pinch | Viewport | Zoom camera |
| Pinch | Minimap | Zoom minimap |
| Tap | Minimap | Jump camera to location |
| Elevation slider drag | Right edge | Move focal point vertically |
| Pitch buttons | Near elevation slider | Nudge camera pitch |
| Swipe down | Bottom sheet | Dismiss panel |
| Swipe left/right | Creature info | Cycle through selection |
| Back button (Android) | Any | ESC precedence chain |

---

## 14. Implementation Considerations

### Gesture recognizer

A central `GestureRecognizer` class should interpret raw `InputEventScreenTouch`
and `InputEventScreenDrag` events and emit high-level gesture signals:

- `tap(position)`
- `double_tap(position)`
- `long_press(position)`
- `drag_start(position)`, `drag_update(position)`, `drag_end(position)`
- `pinch(center, scale_delta)`
- `two_finger_slide(direction, delta)`

With pitch and elevation moved to dedicated UI controls, the two-finger gesture
space only needs to distinguish pinch (fingers converge/diverge) from slide
(fingers move in parallel). This is a much simpler discrimination problem than
the original four-way split.

This replaces the scattered `_unhandled_input` handlers that currently check
for specific key/mouse events.

### Godot touch support

Godot 4 supports multi-touch natively. Key classes:
- `InputEventScreenTouch` (touch down/up with finger index)
- `InputEventScreenDrag` (finger movement with relative/velocity)
- `DisplayServer.screen_get_size()` for responsive layout
- `ProjectSettings.set("display/window/handheld/orientation", ...)` for
  orientation locking

### Conditional UI

The game should detect touch vs desktop at startup and swap UI layouts
accordingly. Options:
- Godot's `OS.has_feature("mobile")` or `OS.has_touchscreen_capability()`
- A setting in the options menu to force touch/desktop mode
- Both UIs share the same underlying data and signals; only the presentation
  layer differs

### Performance

The game's voxel rendering and creature simulation are already lightweight
(no heavy shaders, simple meshes, event-driven sim). Main concerns:
- Draw call batching (already using MultiMesh) is mobile-friendly
- Reduce chunk render distance if needed on low-end devices
- Sprite resolution can be halved for small screens with no visual loss
- Target 30fps on phones, 60fps on tablets

### Export targets

- **Android:** Godot 4 has official Android export. GDExtension (.so) needs
  cross-compilation for ARM64 (aarch64-linux-android target in Rust).
- **iOS:** Godot 4 has official iOS export. GDExtension uses `.framework`
  bundles (not `.dylib`, which is macOS-only). Needs cross-compilation for
  ARM64 (aarch64-apple-ios target in Rust).
- Both require setting up NDK/SDK toolchains for the Rust GDExtension crate.

---

## 15. Scope and Phasing

This is a large feature that touches nearly every input handler and UI panel.
Suggested sub-phases:

1. **Gesture foundation:** GestureRecognizer class, touch camera controls
   (pan, pinch-zoom, two-finger-rotate), elevation slider, tap-to-select.
   Playable but limited.
2. **Command bar and lasso:** Explicit Move/Attack-Move buttons, lasso
   button for multi-select. Core RTS gameplay functional on touch.
3. **UI adaptation:** Bottom sheets, responsive toolbar, panel layout for
   phone/tablet. Full UI usable on mobile.
4. **Construction touch:** Touch-based construction placement, sub-mode
   bar, height adjustment via elevation slider.
5. **Polish:** Gesture tuning, animation, haptic feedback, performance
   profiling on real devices.
6. **Export pipeline:** Android/iOS build setup, cross-compilation, app store
   packaging.
