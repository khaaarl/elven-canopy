# F-creature-actions: Creature Action System

## Overview

Formalize creature actions as a first-class concept. Currently, creatures have
no explicit "action" state — movement timing is implicit (scheduled via event
queue delay), and work is a per-tick increment loop. This design replaces that
with a proper action model where every thing a creature does is a typed,
duration-bearing action with clear start/end semantics.

**Goals:**
- Every creature activity (move, build, eat, sleep, etc.) is an explicit Action
  with a kind, duration, and completion effect.
- Shared action state (kind + timing) lives inline on the Creature row.
- Per-action detail tables exist only when the action genuinely needs extra data
  beyond what the task already provides (currently just Move).
- Action completion is the single point where creatures decide what to do next.
- Work actions have meaningful duration (not 1-tick increments), making the sim
  more legible and opening the door for combat actions, interrupts, and
  animation triggers.

## Data Model

### Creature (modified fields)

```rust
pub struct Creature {
    // ... existing fields (id, species, position, name, etc.) ...

    /// What the creature is currently doing. NoAction when idle.
    pub action_kind: ActionKind,

    /// Tick when the current action completes and the creature becomes
    /// available to pick its next action. None when idle (action_kind == NoAction).
    pub next_available_tick: Option<u64>,

    /// Cached nav path for multi-edge traversal. Persists across individual
    /// Move actions — each Move consumes one edge. Only populated during
    /// task-directed movement (walk_toward_task); wander does not use this.
    pub path: Option<CreaturePath>,

    // REMOVED: move_from, move_to, move_start_tick, move_end_tick
    // (moved to MoveAction table)
}
```

New fields get `#[serde(default)]` for robustness even though we bump the
schema version.

### ActionKind enum

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ActionKind {
    #[default]
    NoAction    = 0,
    Move        = 1,
    Build       = 2,
    Furnish     = 3,
    Cook        = 4,
    Craft       = 5,
    Sleep       = 6,
    Eat         = 7,
    Harvest     = 8,
    AcquireItem = 9,
    PickUp      = 10,
    DropOff     = 11,
    Mope        = 12,
}
```

### MoveAction (new table, PK = CreatureId)

The only action that needs a detail table — stores render interpolation data
for the edge being traversed.

```rust
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct MoveAction {
    #[primary_key]
    pub creature_id: CreatureId,
    /// Visual start position for interpolation.
    pub move_from: VoxelCoord,
    /// Visual end position for interpolation.
    pub move_to: VoxelCoord,
    /// Tick when movement started (for render lerp).
    pub move_start_tick: u64,
}
```

Note: `move_end_tick` is not stored here — it's the creature's
`next_available_tick`. The interpolation function reads both:
`creature.next_available_tick` as the end tick, and `move_action.move_start_tick`
/ `move_action.move_from` / `move_action.move_to` for the lerp.

**SimDb declaration:**

```rust
#[table(singular = "move_action", fks(creature_id = "creatures" on_delete cascade))]
pub move_actions: MoveActionTable,
```

Cascade-delete ensures a removed creature doesn't leave an orphaned MoveAction
row.

### Other actions — no detail tables

All other actions get their context from the creature's current task:

- **Build / Furnish / Cook / Craft**: Task has the structure/recipe/blueprint
  reference. The action just means "working for N ticks, effect fires on
  completion."
- **Sleep**: Task has bed position and location type. Each Sleep action restores
  a chunk of rest. Sleep rate varies by location (bed vs ground) — see
  "Sleep math" below.
- **Eat**: Task has fruit position or bread reference. Action completes →
  food restored, item consumed. **The Eat action's completion effect is
  polymorphic on the underlying task kind** — EatBread removes owned bread from
  inventory and restores `bread_restore_pct`, while EatFruit removes a fruit
  voxel from the world and restores `food_restore_pct`. The resolution handler
  must dispatch on the task kind tag, not just the action kind.
- **Harvest / AcquireItem**: Task has target position. Action completes →
  item collected.
- **PickUp / DropOff**: Haul task has source/destination and phase. Action
  completes → inventory transfer. See "Haul phase transitions" for ordering
  details.
- **Mope**: Task has location. Action completes → increment mope progress,
  check if mope duration fulfilled.

## Action Lifecycle

### Starting an action

When a creature decides to do something (at activation time, when
`next_available_tick` has been reached or creature is idle):

1. Set `creature.action_kind` to the appropriate variant.
2. Set `creature.next_available_tick = Some(tick + duration)`.
3. For Move: insert a `MoveAction` row. Update creature position/current_node
   immediately (as today).
4. For Build: if this is the first Build action for this blueprint (task
   progress == 0), mark the composition as `build_started` so the rendering
   layer begins playback. This must happen at action *start*, not completion.
5. Schedule `CreatureActivation` at `tick + duration`.

**Important:** The `do_*` functions no longer self-schedule their next
activation. All scheduling is done by the action system — either at action
start (scheduling the completion activation) or at action completion
(scheduling the next action's activation). This replaces the current pattern
where every `do_build_work()`, `do_cook()`, etc. ends with
`self.event_queue.schedule(self.tick + 1, CreatureActivation { ... })`.

### Completing an action

When `CreatureActivation` fires at `next_available_tick`:

1. **Resolve the completed action's effects** based on `action_kind`:
   - `Move`: Delete the `MoveAction` row. (Position was already updated at
     action start.)
   - `Build`: Materialize one voxel (or remove one for Carve). Increment
     task progress by 1.
   - `Furnish`: Place one furniture item. Increment task progress by 1.
   - `Cook`: Consume inputs, produce outputs. Increment task progress by 1.
   - `Craft`: Same as Cook pattern.
   - `Sleep`: Restore `rest_per_sleep_action` rest to creature. If rest would
     reach full, complete the task.
   - `Eat`: Dispatch on task kind — EatBread: remove bread from inventory,
     restore `bread_restore_pct`; EatFruit: remove fruit voxel, restore
     `food_restore_pct`. Create thought. Complete the task.
   - `Harvest`: Remove fruit voxel, create ground pile. Complete the task.
   - `AcquireItem`: Transfer item ownership. Complete the task.
   - `PickUp`: Transfer items from source to creature inventory. Update haul
     phase to GoingToDestination. **Update task location to destination nav
     node.** Clear creature's cached path (so it re-pathfinds to the new
     destination). Do *not* complete the task.
   - `DropOff`: Transfer items from creature to destination inventory. Complete
     the haul task.
   - `Mope`: Increment mope progress by `mope_action_ticks`. Check if
     `progress >= total_cost` → complete task.

   If the creature's task no longer exists (e.g., blueprint was cancelled
   mid-action), skip the effect, abandon the task, and fall through to the
   decision cascade.

2. **Clear action state**: `action_kind = NoAction`, `next_available_tick = None`.

3. **Pick next action** (the decision cascade):
   - If creature has a task:
     - **GoTo special case**: if task kind is GoTo and creature is at the task
       location, complete the task immediately (no work action needed) and fall
       through to find a new task.
     - If not at task location → start Move action toward task.
     - If at task location → start the appropriate work action for the task
       kind. (For single-action tasks like Eat, Harvest, AcquireItem, this is
       the only work action. For multi-action tasks like Build, Sleep, Mope,
       this starts the next iteration.)
     - If task just completed → fall through to find new task.
   - If creature has no task:
     - `find_available_task()` → if found, claim and start first action.
     - If no task available → wander (start Move action to random neighbor).

### Haul phase transitions

When a PickUp action completes, the haul task mutates its own state:

1. Transfer items from source to creature inventory.
2. Update `TaskHaulData.phase` to `GoingToDestination`.
3. Update `task.location` to the destination nav node.
4. Clear `creature.path` (force re-pathfind to new destination).
5. Clear action state.
6. The decision cascade sees "has task, not at task location" → starts Move
   toward the (newly updated) destination.

This ordering is critical — the task location must be updated *before* the
decision cascade runs.

### Idle creatures

A creature with `action_kind == NoAction` and `next_available_tick == None` is
truly idle. This only happens momentarily during the decision cascade within
an activation — by the end of the activation, the creature either has a new
action or a wander Move.

## Action Interruption

Several systems can invalidate a creature's current action mid-execution:

### Sources of interruption

1. **Nav node invalidation** (construction solidifying a voxel the creature
   occupies). The creature's current node becomes dead. Current behavior:
   resnap to nearest live node, abandon task, wander.
2. **Blueprint cancellation** (player cancels a build project). The task
   disappears. Detected at activation time: task no longer exists → skip
   effect, fall through.
3. **Mope interruption** (preemption system). The heartbeat uses `can_preempt()`
   to check if Mood(4) can preempt the current task, then abandons it to mope.
4. **Creature removal** (death, despawn). Cascade-delete on MoveAction FK
   handles cleanup automatically.

### Cleanup procedure

When aborting a creature's current action (for cases 1-3):

1. If `action_kind == Move`: delete the `MoveAction` row.
2. Set `action_kind = NoAction`, `next_available_tick = None`.
3. Perform any task-specific abandonment cleanup (e.g., haul reservation
   release, path clearing).
4. Run the decision cascade (find new task or wander).

Extract this as a helper — `abort_current_action(creature_id)` — since multiple
code paths need it.

## Duration Reference

All durations are in ticks (1000 ticks = 1 sim second).

| Action | Duration | Notes |
|--------|----------|-------|
| Move | `ceil(edge.distance * tpv)` | Same formula as today. `tpv` varies by edge type and species. |
| Build | `build_work_ticks_per_voxel` (default 1000 = 1s) | One action per voxel. Carve uses `carve_work_ticks_per_voxel` (same action kind, different duration). |
| Furnish | `furnish_work_ticks_per_item` (default 2000 = 2s) | One action per furniture item. |
| Cook | `cook_work_ticks` (existing, default 5000 = 5s) | One action per cook batch. Reuse existing config field. |
| Craft | `recipe.work_ticks` (per-recipe, varies) | One action per recipe. Duration comes from the recipe definition, not a global config. |
| Sleep | `sleep_action_ticks` (default 1000 = 1s) | Repeated until rest full. Rate varies by bed vs ground. |
| Eat | `eat_action_ticks` (default 1500 = 1.5s) | Covers both bread and fruit. |
| Harvest | `harvest_action_ticks` (default 1500 = 1.5s) | Pick fruit from tree. |
| AcquireItem | `acquire_item_action_ticks` (default 1000 = 1s) | Pick up item from ground. |
| PickUp | `haul_pickup_action_ticks` (default 1000 = 1s) | Haul source pickup. |
| DropOff | `haul_dropoff_action_ticks` (default 1000 = 1s) | Haul destination dropoff. |
| Mope | `mope_action_ticks` (default 1000 = 1s) | Repeated until mope duration fulfilled. |

### Sleep math

Current: `rest_per_sleep_tick = 60_000_000_000` (per species), applied every
tick for `sleep_ticks_bed = 10_000` ticks → total restore = 6e14.
`rest_max = 1_000_000_000_000_000` (1e15). A creature at 50% rest restores
~6e14, bringing it above max. Ground sleep uses `sleep_ticks_ground = 20_000`
(2x longer, same per-tick rate), making it slower.

New: each Sleep action lasts `sleep_action_ticks` (default 1000) and restores
`rest_per_sleep_action`. The task runs until rest is full — no fixed
`total_cost`.

**Bed-vs-ground distinction (simplified):** both bed and ground sleep use the
same per-action restoration rate (`rest_per_sleep_tick * sleep_action_ticks`).
The distinction comes from the total number of actions: ground sleep has a
higher `total_cost` (2x), so it takes longer to fully restore. This is simpler
than separate per-location rates and achieves the same gameplay effect.

**Behavior change (intentional):** under the new model, more exhausted
creatures sleep longer (they need more actions to reach full rest). The current
model has a fixed sleep duration regardless of how tired the creature is. The
new model is more realistic.

### Mope math

Current: `mope_duration_ticks = 10_000` with +1.0 progress per tick.

New: each Mope action lasts `mope_action_ticks` (default 1000 = 1s). The task
`total_cost` stays the same (10_000), but progress increments by
`mope_action_ticks` per action instead of 1.0 per tick. So 10 Mope actions
to complete a mope task.

### Balance changes to note

- **PickUp/DropOff add ~2s per haul cycle.** Currently pickup and dropoff are
  instant on arrival. Under the new model, each haul trip takes ~2 seconds
  longer (1s pickup + 1s dropoff). This reduces logistics throughput. This is
  intentional — hauling should feel like physical work, not teleportation.
- **Eat/Harvest/AcquireItem add 1-1.5s.** Currently instant on arrival. Minor
  impact individually, but noticeable in aggregate if many creatures eat
  simultaneously.
- **Sleep becomes adaptive.** More exhausted creatures sleep longer. Less
  exhausted creatures sleep shorter. Current: always fixed duration.

## Rendering Changes

### interpolated_position()

Currently reads `creature.move_from/to/start_tick/end_tick`. New version:

```rust
impl Creature {
    pub fn interpolated_position(&self, render_tick: f64, move_action: Option<&MoveAction>) -> (f32, f32, f32) {
        if let (Some(ma), Some(end_tick)) = (move_action, self.next_available_tick) {
            if self.action_kind == ActionKind::Move {
                let duration = end_tick as f64 - ma.move_start_tick as f64;
                if duration > 0.0 {
                    let t = ((render_tick - ma.move_start_tick as f64) / duration).clamp(0.0, 1.0) as f32;
                    let x = ma.move_from.x as f32 + (ma.move_to.x as f32 - ma.move_from.x as f32) * t;
                    let y = ma.move_from.y as f32 + (ma.move_to.y as f32 - ma.move_from.y as f32) * t;
                    let z = ma.move_from.z as f32 + (ma.move_to.z as f32 - ma.move_from.z as f32) * t;
                    return (x, y, z);
                }
            }
        }
        (self.position.x as f32, self.position.y as f32, self.position.z as f32)
    }
}
```

### Call sites in sim_bridge.rs

All callers of `interpolated_position()` need updating to pass the MoveAction
lookup:

- `get_creature_positions()` — iterates creatures by species, used by
  `get_elf_positions()` and `get_capybara_positions()`.
- `get_creature_info()` — single creature lookup for the info panel.

Each call site adds an O(1) `db.move_actions.get(&creature.id)` lookup.

## Task System Changes

### total_cost semantics

For tasks where actions have fixed per-unit duration:
- **Build**: `total_cost` = number of voxels (not ticks). Progress increments
  by 1 per completed Build action.
- **Furnish**: `total_cost` = number of furniture items. Progress increments
  by 1 per completed Furnish action.
- **Cook**: `total_cost` = 1 (one batch per task). Progress increments by 1.
- **Craft**: `total_cost` = 1 (one recipe per task). Progress increments by 1.
- **Mope**: `total_cost` stays in ticks (e.g., 10_000). Progress increments by
  `mope_action_ticks` per action.

For tasks with dynamic completion:
- **Sleep**: No fixed total_cost. Completes when creature rest is full.
- **Haul**: Completes when both phases (PickUp + travel + DropOff) finish.
- **Eat / EatFruit / Harvest / AcquireItem / GoTo**: Single-action tasks.
  Complete when the one action finishes (after travel if needed).

The UI progress bar (`progress / total_cost`) continues to work — the ratio
is the same, just the scale changes. Tests that assert specific progress values
will need updating.

### GoTo special case

GoTo is currently instant on arrival (total_cost = 0). Under the new model,
arrival *is* the last Move action completing. No work action needed — the task
completes when the creature reaches the destination node and its Move action
resolves. If the creature is already at the GoTo location when the task is
claimed, the task completes immediately (no action started).

### EatBread at-location case

EatBread tasks set their location to the creature's current node (the creature
eats bread from inventory, no travel needed). When claimed, the decision
cascade sees "at task location → start work action" and immediately begins
an Eat action.

## Event System Changes

### CreatureMovementComplete — removed

`ScheduledEventKind::CreatureMovementComplete` is vestigial code — it forms a
self-chaining loop (`handle_creature_movement_complete` schedules the next
`CreatureMovementComplete`) but nothing external ever initiates the first event
in the chain. It is dead code. Remove it entirely: delete the enum variant,
the handler function, and the dispatch arm in the tick loop.

All movement goes through `CreatureActivation` + `MoveAction` under the new
model.

## Refactoring Pattern: Splitting do_* Functions

Each current `do_*` function (e.g., `do_build_work`, `do_sleep`, `do_cook`)
combines action start, effect resolution, completion check, and self-scheduling
into one function. Under the new model, these split into two halves:

**Start function** (`start_build_action`, etc.):
- Set `action_kind` and `next_available_tick`.
- Perform any start-time side effects (e.g., `build_started` flag for music).
- Schedule `CreatureActivation` at completion tick.

**Resolve function** (`resolve_build_action`, etc.):
- Apply the action's effect (materialize voxel, restore rest, etc.).
- Increment task progress if applicable.
- Check task completion.
- Does NOT self-schedule — returns control to the activation handler, which
  runs the decision cascade.

This split is the core refactoring pattern for every task kind.

## Migration Notes

### Fields removed from Creature
- `move_from: Option<VoxelCoord>`
- `move_to: Option<VoxelCoord>`
- `move_start_tick: u64`
- `move_end_tick: u64`

### Fields added to Creature
- `action_kind: ActionKind` (default `NoAction`, `#[serde(default)]`)
- `next_available_tick: Option<u64>` (default `None`, `#[serde(default)]`)

### New table: MoveAction
- Added to SimDb with cascade-delete FK to creatures.
- Tabulosity's schema versioning defaults missing tables to empty on
  deserialization, so the new table alone doesn't require a version bump.

### Removed event: CreatureMovementComplete
- Delete enum variant, handler, and dispatch arm.

### Config changes
- `rest_per_sleep_tick` kept as-is. Each Sleep action restores
  `rest_per_sleep_tick * sleep_action_ticks`. Bed vs ground distinction comes
  from different `total_cost` (number of actions), not different rates.
- `sleep_ticks_bed` / `sleep_ticks_ground` kept. Divided by
  `sleep_action_ticks` to get the number of Sleep actions (`total_cost`).
- New config fields: `sleep_action_ticks`, `eat_action_ticks`,
  `harvest_action_ticks`, `acquire_item_action_ticks`, `haul_pickup_action_ticks`,
  `haul_dropoff_action_ticks`, `mope_action_ticks`.
- Cook: reuse existing `cook_work_ticks` (default 5000) as the single Cook
  action duration. No new config field needed.
- Craft: duration comes from `recipe.work_ticks` (per-recipe). No new
  global config field needed.

### Serde / save compatibility
- Bump schema version on SimDb.
- Old saves won't load (acceptable at this stage of development).
- New Creature fields get `#[serde(default)]` for robustness.

## Implementation Order

1. **Add ActionKind enum and new Creature fields.** Add MoveAction table to
   SimDb with cascade-delete FK. Remove old move_* fields from Creature.
   Remove `CreatureMovementComplete` event variant and handler. Update serde,
   bump schema version.

2. **Refactor movement to use actions.** Wander and walk_toward_task both
   create MoveAction rows, set action_kind/next_available_tick. Wander picks
   a single random neighbor (does not use creature.path). Update
   interpolated_position to read from MoveAction. Update all call sites in
   sim_bridge.rs.

3. **Refactor activation handler.** Single entry point: resolve completed
   action → clear action state → pick next action. Replace the current
   "has task / no task" branch with the decision cascade. Extract
   `abort_current_action()` helper for interruption paths.

4. **Convert work tasks to actions.** For each task kind, split the existing
   `do_*` function into start/resolve halves. Remove all self-scheduling from
   work functions. Update total_cost semantics and progress tracking.
   Order: Build → Furnish → Sleep → Eat → Harvest → AcquireItem →
   Haul (PickUp/DropOff) → Mope → Cook → Craft.

5. **Config migration.** Add new duration configs, update defaults, split
   sleep restoration into bed/ground rates, adjust mope math.

6. **Test updates.** Every existing sim test that checks movement, task
   progress, or activation timing will need updating. Specific areas:
   - Progress value assertions (now count in units, not ticks).
   - `move_from`/`move_to` assertions → MoveAction table lookups.
   - `build_started` timing (now fires at action start).
   - Sleep tests (adaptive duration, bed-vs-ground rate difference).
   - Haul tests (PickUp/DropOff now have duration).
