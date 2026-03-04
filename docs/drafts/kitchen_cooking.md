# F-bldg-kitchen — Kitchen Cooking Draft (v6)

Design draft for kitchen cooking: kitchens receive fruit via logistics and
produce bread. This is a focused subset of the full recipe system
(F-recipes) — hardcoded fruit→bread conversion rather than data-driven
recipes. The recipe system can generalize this later.

## Summary of Changes

1. **Don't spawn monkeys at game start** (they steal fruit).
2. **Elves spawn with ≥2 breads** (prevents them from eating fruit during
   manual testing).
3. **Storehouses get default logistics** on furnishing: low priority,
   requesting 10 fruit + 20 bread.
4. **Kitchens get default logistics** on furnishing: high priority,
   requesting 5 fruit.
5. **Surplus item source logic** — `find_haul_source()` enhanced so
   buildings with logistics enabled that have items exceeding their wants
   make those items available as sources regardless of priority.
6. **Kitchen task monitor** — a new periodic check (piggybacks on the
   logistics heartbeat) that creates Cook tasks when conditions are met.
7. **Cook task** — new `TaskKind::Cook` variant where an elf walks to a
   kitchen, works for some duration, and converts 1 fruit → 10 bread
   in the kitchen's inventory.
8. **Kitchen cooking UI** — structure info panel shows cooking config
   (target bread threshold, enable/disable).
9. **Integration test** — full end-to-end: 1 elf, 1 fruit, 1 storehouse,
   1 kitchen → after many ticks → 10 bread in storehouse.

---

## Detailed Design

### 1. Don't Spawn Monkeys at Game Start

**File:** `godot/scripts/main.gd`

Remove the `for i in 3: bridge.spawn_creature("Monkey", ...)` lines from
both `_start_new_game()` and `_start_multiplayer_game()`. Monkeys still
exist as a species — they just won't auto-spawn. The player can spawn them
manually via placement mode if desired.

### 2. Elves Spawn with ≥2 Breads

**File:** `elven_canopy_sim/src/sim.rs` — `spawn_creature()`

After creating the `Creature` struct, if species is `Elf`, add bread
items (owned by the creature) to its inventory:

```rust
if species == Species::Elf {
    creature.inventory.push(Item {
        kind: ItemKind::Bread,
        quantity: config.elf_starting_bread,
        owner: Some(creature_id),
        reserved_by: None,
    });
}
```

New config field: `elf_starting_bread: u32` (default 2).

### 3. Storehouse Default Logistics

**File:** `elven_canopy_sim/src/sim.rs` — `furnish_structure()`

When a building is furnished as `Storehouse`, automatically configure:
- `logistics_priority = Some(2)` (low — storehouses are passive receivers)
- `logistics_wants = [Fruit × 10, Bread × 20]`

These defaults come from new config fields:
- `storehouse_default_priority: u8` (default 2)
- `storehouse_default_fruit_want: u32` (default 10)
- `storehouse_default_bread_want: u32` (default 20)

The player can adjust these via the existing logistics UI after furnishing.

### 4. Kitchen Default Logistics

**File:** `elven_canopy_sim/src/sim.rs` — `furnish_structure()`

When a building is furnished as `Kitchen`, automatically configure:
- `logistics_priority = Some(8)` (high — kitchens actively pull ingredients)
- `logistics_wants = [Fruit × 5]`

New config fields:
- `kitchen_default_priority: u8` (default 8)
- `kitchen_default_fruit_want: u32` (default 5)

### 5. Surplus Item Source Logic (`find_haul_source()` Enhancement)

The existing `find_haul_source()` only lets a building pull from buildings
with *strictly lower* logistics priority. This prevents the storehouse
(priority 2) from pulling bread out of the kitchen (priority 8).

**Fix:** Extend `find_haul_source()` with a new rule: a building is also a
valid source for items it has **in excess of its wants**, provided
logistics is enabled on that building. Specifically, when scanning
buildings as potential sources, a building qualifies (regardless of
priority) if:
1. It has `logistics_priority: Some(_)` (logistics enabled), AND
2. It holds unreserved items of the requested kind in excess of its own
   `logistics_wants` target for that kind.

Buildings with logistics disabled (`logistics_priority: None`) are **not**
eligible as surplus sources. This prevents items in homes, dormitories,
unfurnished buildings, etc. from being silently hauled away.

**Algorithm for building-as-source surplus availability:**

```
fn surplus_items(building, item_kind) -> u32:
    // Only buildings with logistics enabled can be surplus sources
    if building.logistics_priority.is_none():
        return 0
    let held = unreserved_item_count(building.inventory, item_kind)
    let wanted = building.logistics_wants
        .find(|w| w.item_kind == item_kind)
        .map(|w| w.target_quantity)
        .unwrap_or(0)
    held.saturating_sub(wanted)
```

In `find_haul_source()`, after checking ground piles and lower-priority
buildings (existing logic), add a third pass:

```
// Check logistics-enabled buildings (any priority) for surplus items.
for each building (excluding the requester):
    let surplus = surplus_items(building, item_kind)
    if surplus > 0:
        return (Building(sid), surplus.min(needed), nav_node)
```

Iteration is over `self.structures` (a `BTreeMap`), so source selection is
deterministic by `StructureId`. The first building with surplus wins.

**Example scenarios:**
- Kitchen (priority 8) has 10 bread, wants 0 bread → 10 surplus bread
  available for storehouse to pull.
- Kitchen (priority 8) has 7 fruit, wants 5 fruit → 2 surplus fruit
  available.
- Storehouse (priority 2) has 15 bread, wants 20 bread → 0 surplus bread
  (it still needs 5 more).
- Home (no logistics) has 3 bread, no wants → **not a surplus source**
  (logistics disabled, items stay put).

**Known limitation — surplus oscillation:** With multiple buildings at the
same priority that both want the same item, oscillation could theoretically
occur if one overshoots and the other pulls its surplus, then vice versa.
In practice this is mitigated by `saturating_sub` (surplus is 0 when at or
below the want target) and by the `max_haul_tasks_per_heartbeat` cap. Not
worth solving now — revisit if observed in practice.

### 6. Kitchen Task Monitor

**File:** `elven_canopy_sim/src/sim.rs` — called from
`process_logistics_heartbeat()` (or a new `process_kitchen_heartbeat()`)

Each logistics heartbeat, after creating haul tasks, scan kitchens:

```
for each structure where furnishing == Kitchen:
    if kitchen.cooking_enabled == false: skip
    // Check kitchen's own bread inventory against its per-kitchen target
    let bread_in_kitchen = unreserved_item_count(kitchen.inventory, Bread)
    if bread_in_kitchen >= kitchen.cooking_bread_target: skip
    // Check if kitchen has enough unreserved fruit
    let fruit_count = unreserved_item_count(kitchen.inventory, Fruit)
    if fruit_count < config.cook_fruit_input: skip
    // Check if kitchen already has an active Cook task
    if any task is TaskKind::Cook { structure_id: this kitchen } and not Complete: skip
    // Reserve fruit for the Cook task (prevents logistics from hauling it away)
    reserve cook_fruit_input fruit in kitchen.inventory for the new task_id
    // Create a Cook task
    create Cook task at kitchen's interior nav node
```

**Per-kitchen bread target:** The `cooking_bread_target` field is checked
against the kitchen's *own* bread inventory, not a global sum. This is a
simplification over scanning all buildings. Since the surplus source logic
drains bread from kitchens to storehouses, this self-regulates: kitchen
cooks → bread accumulates → surplus hauled out → kitchen drops below
target → cooks again. The player configures the kitchen's target
independently from its logistics wants (they serve different purposes).

**Fruit reservation at task creation:** The Cook task reserves its input
fruit via `reserve_items()` when the task is created (same pattern as Haul
tasks). This prevents logistics from hauling the reserved fruit away
between task creation and the elf arriving to cook. The reservation
survives if the task returns to `Available` (e.g., elf unassigned) —
another elf can claim it and finish cooking with the same reserved fruit.

**New fields on `CompletedStructure`:**
- `cooking_enabled: bool` — whether this kitchen auto-cooks.
  Default: `true` for newly furnished kitchens. `#[serde(default)]` makes
  this `false` for old saves (which have no kitchens, so harmless).
- `cooking_bread_target: u32` — stop cooking when this kitchen's own bread
  inventory ≥ this value. Default: 50 for newly furnished kitchens.
  `#[serde(default)]` makes this 0 for old saves.

These defaults are applied in `furnish_structure()` when furnishing as
Kitchen, not via serde defaults. The serde defaults only matter for
backward compatibility with old saves.

### 7. Cook Task (`TaskKind::Cook`)

**File:** `elven_canopy_sim/src/task.rs`

New variant:
```rust
Cook {
    structure_id: StructureId,
}
```

**Behavior script** (in `sim.rs` `execute_task_behavior()`):

New match arm in `execute_task_at_location()` for `TaskKind::Cook`.
**Must `return` after dispatching** (like the Haul arm does) so the
activation chain doesn't schedule a duplicate activation.

- If not at task.location → walk toward it (same as other tasks).
- If at location → do cook work:
  - Increment progress by 1.0 per activation.
  - When progress ≥ total_cost (`cook_work_ticks`):
    - Remove the reserved fruit from the kitchen's inventory via
      `remove_reserved_items()` (consumes `cook_fruit_input` fruit).
    - If removal returned less than expected (fruit somehow missing),
      call `clear_reservations(task_id)` on the kitchen inventory and
      complete the task with no conversion.
    - Otherwise, add `cook_bread_output` bread (unowned, unreserved) to
      the kitchen's inventory via `add_item(kind: Bread, quantity,
      owner: None, reserved_by: None)`.
    - Complete the task.

**Cleanup on abandonment:** Add a `cleanup_cook_task()` function that
calls `clear_reservations(task_id)` on the kitchen's inventory to release
the reserved fruit, then sets the task to `Complete`. This must be called
from the **node-invalidation path** in `execute_task_behavior()` — when a
creature's nav node becomes invalid mid-task. Currently that path
dispatches `cleanup_haul_task`; add `cleanup_cook_task` alongside it.

**Why only the node-invalidation path:** The other abandonment path —
`unassign_creature_from_task()` — resets the task to `Available`, letting
another elf claim it. In this case the fruit reservation should stay
intact so the next elf can finish cooking. Only the node-invalidation path
needs cleanup, because it leaves the task stuck in `InProgress` with no
worker. Without completing it, the orphaned task permanently blocks the
kitchen monitor from creating a new Cook task (the monitor skips kitchens
with any non-Complete Cook task). Setting the task to `Complete` in the
cleanup function lets the monitor create a fresh task on the next
heartbeat.

**Note:** The same orphan-task issue exists for Haul tasks in the
node-invalidation path (they get stuck InProgress too). This is a
pre-existing bug not addressed here — but for Cook tasks the consequence
is worse (permanently disabled kitchen vs. leaked reservations), so we
fix it specifically for Cook.

**Task origin:** `TaskOrigin::Automated` (created by the kitchen monitor,
not the player).

**New config fields:**
- `cook_work_ticks: u64` (default 5000 = 5 sim seconds of work)
- `cook_fruit_input: u32` (default 1)
- `cook_bread_output: u32` (default 10)

### 8. Kitchen Cooking UI

**File:** `godot/scripts/structure_info_panel.gd`

For Kitchen-furnished buildings, add a "Cooking" section below the
logistics section:
- **Enable checkbox** — toggles `cooking_enabled`
- **Bread target SpinBox** — sets `cooking_bread_target` (range 0–500,
  step 10, default 50). Label: "Max bread in kitchen"
- **Status label** — "Cooking: idle" / "Cooking: in progress" / "Cooking:
  bread target reached"

**New signals:** `cooking_config_changed(structure_id, enabled, bread_target)`

**New SimAction variant:**
```rust
SetCookingConfig {
    structure_id: StructureId,
    cooking_enabled: bool,
    cooking_bread_target: u32,
}
```

The `SetCookingConfig` handler in `apply_command()` must validate that the
structure exists and `furnishing == Some(Kitchen)` before modifying fields
(same defensive pattern as `SetLogisticsPriority`).

**File:** `elven_canopy_gdext/src/sim_bridge.rs` — new `set_cooking_config()`
method exposed to GDScript.

### 9. Integration Test

**File:** `elven_canopy_sim/src/sim.rs` (tests module)

Test name: `kitchen_cooks_fruit_into_bread_end_to_end`

Setup:
1. Create sim with test seed.
2. Spawn 1 elf carrying many owned breads (e.g. 20, well above what it
   could eat during the test) so it never gets hungry. The elf's heartbeat
   hunger check will find owned bread and create instant `EatBread` tasks
   when hungry, keeping it functional. Using a generous count avoids
   timing fragility.
3. Place 1 fruit on the ground as a ground pile item (not a voxel — use
   `ground_piles` with `Item { kind: Fruit, quantity: 1, ... }`).
   Position must be at y=1 (above ForestFloor) with a reachable nav node.
4. Insert 1 completed+furnished Storehouse with default logistics (priority
   2, wants: 10 fruit + 20 bread).
5. Insert 1 completed+furnished Kitchen with default logistics (priority 8,
   wants: 5 fruit) and cooking enabled (bread target 50).
6. Buildings need two non-overlapping sites. The existing
   `find_building_site()` helper returns one site; the test will need to
   find a second site by scanning further along the walkable area, or by
   using offset positions from the first site.

Expected flow:
1. Logistics heartbeat fires → kitchen wants fruit → haul task created
   (ground pile → kitchen).
2. Elf claims haul task → walks to ground pile → picks up fruit → walks
   to kitchen → deposits fruit.
3. Next logistics heartbeat → kitchen monitor: kitchen has 1 fruit ≥
   `cook_fruit_input`, bread in kitchen (0) < `cooking_bread_target` (50),
   no active cook task → reserves 1 fruit, creates Cook task.
4. Elf claims cook task → walks to kitchen → works for `cook_work_ticks` →
   reserved fruit consumed, 10 unowned bread added to kitchen inventory.
5. Next logistics heartbeat → storehouse wants bread → surplus scan finds
   kitchen has 10 bread with 0 bread want → haul task created (kitchen →
   storehouse).
6. Elf claims haul task → walks to kitchen → picks up bread → walks to
   storehouse → deposits bread.
7. **Result:** storehouse has 10 unowned bread. Test passes.

**Tick budget:** The test runs the sim for as many ticks as needed (tens
of thousands). Sim ticks are decoupled from real time in tests — the
event queue processes events at their scheduled tick with no wall-clock
delay. The test uses default config intervals without overrides.

**Verification:** After running N ticks, assert:
- Ground pile is empty (fruit was picked up).
- Kitchen inventory has 0 fruit (consumed by cooking).
- Kitchen inventory has 0 bread (hauled to storehouse).
- Storehouse inventory has 10 unowned, unreserved bread.

---

## Config Summary

New `GameConfig` fields:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `elf_starting_bread` | `u32` | 2 | Bread given to each elf on spawn |
| `storehouse_default_priority` | `u8` | 2 | Default logistics priority for storehouses |
| `storehouse_default_fruit_want` | `u32` | 10 | Default fruit want for storehouses |
| `storehouse_default_bread_want` | `u32` | 20 | Default bread want for storehouses |
| `kitchen_default_priority` | `u8` | 8 | Default logistics priority for kitchens |
| `kitchen_default_fruit_want` | `u32` | 5 | Default fruit want for kitchens |
| `cook_work_ticks` | `u64` | 5000 | Ticks of work to complete one cook cycle |
| `kitchen_default_bread_target` | `u32` | 50 | Default bread target for kitchens |
| `cook_fruit_input` | `u32` | 1 | Fruit consumed per cook cycle |
| `cook_bread_output` | `u32` | 10 | Bread produced per cook cycle |

Also add all 10 fields to `default_config.json`.

New `CompletedStructure` fields (all `#[serde(default)]` for save compat):

| Field | Type | Serde default | Furnish default | Description |
|-------|------|---------------|-----------------|-------------|
| `cooking_enabled` | `bool` | `false` | `true` | Whether this kitchen auto-creates cook tasks |
| `cooking_bread_target` | `u32` | 0 | 50 | Stop cooking when kitchen bread ≥ this |

The serde defaults are for backward compatibility with old saves. The
"furnish default" column shows values set by `furnish_structure()` when
furnishing as Kitchen.

---

## Code Touchpoints Checklist

Complete list of files and functions that need changes:

### `elven_canopy_sim/src/config.rs`
- [ ] Add 10 new fields to `GameConfig` with `#[serde(default = "...")]`
- [ ] Add default value functions for each
- [ ] Add fields to `GameConfig::default()` impl

### `elven_canopy_sim/src/task.rs`
- [ ] Add `TaskKind::Cook { structure_id: StructureId }` variant

### `elven_canopy_sim/src/command.rs`
- [ ] Add `SimAction::SetCookingConfig { structure_id, cooking_enabled, cooking_bread_target }`

### `elven_canopy_sim/src/building.rs`
- [ ] Add `cooking_enabled: bool` and `cooking_bread_target: u32` to `CompletedStructure` with `#[serde(default)]`
- [ ] Update test struct literals in `building.rs` tests that construct `CompletedStructure` directly (add new fields or use `..Default::default()`)

### `elven_canopy_sim/src/sim.rs`
- [ ] `spawn_creature()` — add starting bread for elves
- [ ] `furnish_structure()` — set default logistics + cooking fields for Storehouse and Kitchen
- [ ] `find_haul_source()` — add surplus-item third pass (logistics-enabled buildings only)
- [ ] `process_logistics_heartbeat()` — call kitchen monitor after haul task creation
- [ ] New `process_kitchen_monitor()` — scan kitchens, reserve fruit, create Cook tasks
- [ ] `execute_task_at_location()` — new `TaskKind::Cook` match arm (must `return`)
- [ ] New `do_cook()` — increment progress, convert fruit→bread on completion, handle partial removal fallback with `clear_reservations`
- [ ] New `cleanup_cook_task()` — clear reservations on the kitchen's inventory AND set task to Complete
- [ ] `execute_task_behavior()` node-invalidation path — add Cook cleanup (calls `cleanup_cook_task` which completes the task, preventing orphaned InProgress tasks from permanently blocking the kitchen)
- [ ] `apply_command()` — new `SimAction::SetCookingConfig` match arm with Kitchen validation
- [ ] Update `insert_completed_building()` test helper and all test struct literals that construct `CompletedStructure` directly (add new fields)

### `elven_canopy_gdext/src/sim_bridge.rs`
- [ ] New `set_cooking_config()` method exposed to GDScript
- [ ] `get_structure_info()` — add cooking fields (`cooking_enabled`, `cooking_bread_target`, active cook task status) to the returned dictionary
- [ ] Three `TaskKind` match-to-string blocks — add `Cook` arms (task description, task kind name, etc.)

### `godot/scripts/structure_info_panel.gd`
- [ ] Add "Cooking" section for Kitchen-furnished buildings
- [ ] New signal: `cooking_config_changed`

### `godot/scripts/main.gd`
- [ ] Remove monkey spawning from `_start_new_game()` and `_start_multiplayer_game()`
- [ ] Wire `cooking_config_changed` signal to `bridge.set_cooking_config()`

### `default_config.json`
- [ ] Add all 9 new config fields

---

## Implementation Order

1. **Config + types first** — add config fields, `TaskKind::Cook`,
   `SimAction::SetCookingConfig`, `CompletedStructure` fields. Update
   all existing `CompletedStructure` struct literals in tests and helpers
   to include new fields (this must happen in the same step to compile).
2. **Spawn changes** — no monkeys, elf starting bread.
3. **Default logistics on furnish** — storehouse and kitchen defaults.
4. **Surplus source logic** — enhance `find_haul_source()` (logistics-
   enabled buildings only).
5. **Cook task behavior** — `do_cook()` and `cleanup_cook_task()` in
   sim.rs. New match arm in `execute_task_at_location()`. Wire cleanup
   into node-invalidation path (completing the task to prevent orphans).
6. **Kitchen task monitor** — scan kitchens in logistics heartbeat, with
   fruit reservation.
7. **Integration test** — the big end-to-end test.
8. **UI** — cooking section in structure info panel.
9. **Bridge** — `set_cooking_config()` in sim_bridge.rs, update
   `get_structure_info()`, update existing `TaskKind` match arms.

Steps 1–7 are pure Rust (sim crate). Steps 8–9 are GDScript + bridge.
The integration test (step 7) validates the core loop before wiring up UI.

---

## Known Limitations / Future Work

- **Completed task accumulation:** Completed Cook tasks (and Haul tasks)
  remain in the `tasks` BTreeMap. The kitchen monitor scans tasks every
  heartbeat to check for active Cook tasks. Over very long sessions this
  could get slow. A future task-cleanup pass (purging completed tasks
  after some delay) would fix this for all task types, not just Cook.

- **`count_in_transit_items` doesn't count Cook output:** If a player
  manually adds a bread want to a kitchen, the bread about to be produced
  by an active Cook task isn't counted as "in-transit" for that want,
  potentially causing over-ordering. This is a minor edge case since
  kitchens don't default to wanting bread, and the practical impact is
  just slightly more bread than needed.

- **Orphaned Haul tasks (pre-existing):** The node-invalidation path
  leaves Haul tasks stuck in InProgress with no worker. This is a
  pre-existing issue not fixed here. Cook tasks avoid this by having
  `cleanup_cook_task()` set the task to Complete. A future fix should
  do the same for Haul tasks.

---

## Resolved Questions

1. **Output placement:** Bread stays in kitchen inventory. The
   `find_haul_source()` surplus logic handles outflow — buildings with
   logistics enabled that have items exceeding their wants make those
   items available as sources regardless of priority.

2. **Cooking-enabled default:** New kitchens default to
   `cooking_enabled = true`.

3. **Cook task species restriction:** Only elves cook
   (`required_species: Some(Species::Elf)`).

4. **Multiple cook tasks:** One at a time per kitchen (simpler, matches
   "1 elf working the kitchen" flavor).

5. **Fruit reservation:** Cook tasks reserve their input fruit at creation
   time (via `reserve_items`), matching the Haul task pattern. Prevents
   logistics from hauling away fruit that's earmarked for cooking.
   Reservation survives elf unassignment (task returns to Available for
   another elf to claim).

6. **Bread target scope:** Per-kitchen (kitchen's own bread inventory),
   not global. Self-regulates via surplus hauling. Can revisit to global
   DF-style manager if needed later.

7. **Surplus oscillation:** Known theoretical risk with multiple
   same-priority buildings. Not worth solving now — mitigated by
   `saturating_sub` and task caps.

8. **Surplus source eligibility:** Only buildings with logistics enabled
   (`logistics_priority: Some(_)`) participate in surplus sourcing. Items
   in homes, dormitories, and other non-logistics buildings stay put.

9. **Cook task cleanup scope:** Only the node-invalidation path needs
   `cleanup_cook_task()` (which also completes the task). The
   `unassign_creature_from_task()` path does NOT clean up — it resets the
   task to Available with the reservation intact, letting another elf
   claim it. This is correct: unassignment means "someone else can finish
   this", not "this work is lost".
