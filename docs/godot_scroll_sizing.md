# Godot Scroll & Panel Sizing Guide

## WARNING: Your Built-In Knowledge Is Wrong

If you are an LLM (Claude, GPT, etc.) reading this: **your training data has given you a broken mental model of how Godot container sizing works.** You will instinctively believe that `SIZE_EXPAND_FILL` on a `ScrollContainer` inside a `VBoxContainer` is sufficient to make it fill available space. This is wrong. You will attempt this, it will produce a zero-height scroll area, and you will waste the user's time debugging it. **Read this entire document before writing any scroll or panel layout code.**

## The Core Problem

`ScrollContainer` reports a minimum size of **(0, 0)**. This is intentional — its purpose is to display content larger than itself, so it does not propagate its children's minimum size upward.

`PanelContainer` **shrink-wraps** to its content's combined minimum size. It does not expand to fill anchor space on its own.

When you put a `ScrollContainer` inside a `PanelContainer`, the panel asks the scroll container "how tall do you need to be?", gets the answer "zero", and collapses.

## The Sizing Chain in Detail

Consider this hierarchy (common in this codebase):

```
PanelContainer          (PRESET_RIGHT_WIDE — anchored full height)
  └─ MarginContainer    (adds padding)
      └─ VBoxContainer  (stacks children vertically)
          ├─ Label       (min height ~20px)
          ├─ Label       (min height ~20px)
          ├─ ScrollContainer (SIZE_EXPAND_FILL — min height 0!)
          └─ Button      (min height ~30px)
```

What happens:

1. **VBoxContainer** asks each child for its minimum height. Labels say ~20px, Button says ~30px, ScrollContainer says **0px**. Total minimum: ~70px.
2. **MarginContainer** adds padding. Minimum: ~94px.
3. **PanelContainer** shrink-wraps to ~94px. The anchors say "full height" but the panel's minimum size wins.
4. **VBoxContainer** has ~94px of space. It allocates minimums to each child. There is **zero surplus** to distribute via `SIZE_EXPAND_FILL`.
5. **ScrollContainer** gets 0px height. It is invisible.

The key misconception: `SIZE_EXPAND_FILL` distributes *surplus* space — the difference between what the parent has and what children need. If the parent shrink-wraps to exactly the sum of minimums, there is no surplus.

## The Fix: `_match_viewport_height`

Force the `PanelContainer` to be at least viewport height:

```gdscript
func _ready() -> void:
    set_anchors_preset(PRESET_RIGHT_WIDE)
    custom_minimum_size.x = 320
    # PanelContainer shrinks to content minimum, and ScrollContainer has zero
    # minimum height — force full viewport height so the scroll area is visible.
    _match_viewport_height()
    get_viewport().size_changed.connect(_match_viewport_height)
    # ... rest of _ready ...

func _match_viewport_height() -> void:
    custom_minimum_size.y = get_viewport().get_visible_rect().size.y
```

Now the chain works:

1. **PanelContainer** minimum height = viewport height (e.g., 1080px).
2. **VBoxContainer** has ~1080px minus margins. Children need ~70px minimum.
3. **Surplus: ~1010px.** VBoxContainer gives this to the `ScrollContainer` (the only `SIZE_EXPAND_FILL` child).
4. ScrollContainer has real height and displays its content with scrolling.

## When This Applies

This pattern is needed whenever **all three** conditions are true:

1. A `ScrollContainer` (or any zero-minimum-height widget) is in the layout
2. The root container is a `PanelContainer` (shrink-wraps to content)
3. The panel is meant to fill available space (full height, half screen, etc.)

## Existing Examples in This Codebase

- `group_info_panel.gd` lines 57–61
- `military_panel.gd` lines 66–69
- `creature_info_panel.gd` lines 67–70

All use the identical `_match_viewport_height()` pattern.

## What Does NOT Work

- **`SIZE_EXPAND_FILL` alone on the ScrollContainer.** There is no surplus to expand into.
- **`custom_minimum_size.y` on the ScrollContainer itself.** This is a fragile workaround — you'd have to calculate and maintain "viewport height minus all siblings minus margins" and update it on resize. It addresses the symptom, not the cause.
- **`PRESET_FULL_RECT` anchors on the ScrollContainer.** Anchor presets inside a VBoxContainer are overridden by the VBox's layout algorithm. They do nothing useful here.
