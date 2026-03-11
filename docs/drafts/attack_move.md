# Attack-Move Design (F-attack-move)

Supersedes the attack-move section in `combat_military.md` (§2 "Attack-Move").
This document incorporates that design plus discussion from March 2026.

## Overview

Attack-move is a classic RTS command: send selected creatures to a destination,
and they engage any hostiles they encounter en route. This differs from a
regular GoTo (which ignores enemies) and from AttackTarget (which pursues a
specific creature). Attack-move is destination-oriented with opportunistic
combat.

## Hotkey

**F** — "fight-move" mnemonic. The F key is currently mapped to `focal_down`
(vertical camera descent) alongside Page Down. Remove the R and F bindings
from `focal_up`/`focal_down`, leaving Page Up/Page Down as the sole vertical
camera keys. This frees F for attack-move and R for a future gameplay command.
Q and E (camera rotation) are unchanged.

**Implementation:** Remove the R and F entries from the `focal_up`/`focal_down`
input map actions in `godot/project.godot`. Update the camera controls
docstring in `orbital_camera.gd` and update `keybind_help.gd` to reflect the
change and add the attack-move binding.

### Attack-move activation

**F + left-click on ground:** Attack-move all selected creatures to that
location.

**F + left-click on a creature (friendly or hostile):** Treated as attack-move
to that creature's current location. This avoids accidental friendly fire —
elves won't deliberately target allies, they just walk to where the creature
was.

The input flow lives in `selection_controller.gd`, alongside the existing
right-click context commands. Pressing F enters a transient "attack-move mode";
the next left-click dispatches the command and exits the mode. Pressing ESC or
F again cancels the mode. A cursor or status indicator should signal the mode
to the player (details deferred to implementation).

**Multi-select:** Each selected creature gets its own `AttackMove` task to the
same destination, dispatched via the standard command pipeline. This matches
how right-click GoTo and AttackCreature already work.

## Sim-side design

### SimAction variant

```rust
AttackMove { creature: CreatureId, destination: VoxelCoord },
```

Flows through the standard command pipeline. GDScript dispatches one
`AttackMove` per selected creature.

### Task kind

```rust
// In TaskKindTag:
AttackMove,

// In TaskKind:
AttackMove,  // unit variant — all data in extension table and base Task row
```

`TaskKind::AttackMove` is a unit variant. The destination lives in the
extension table (`TaskAttackMoveData.destination`). The base `Task` row's
`location` field is set to the nav node nearest the destination (the walk
target).

### Extension table

```rust
auto_pk_id!(TaskAttackMoveDataId);

#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskAttackMoveData {
    #[primary_key(auto_increment)]
    pub id: TaskAttackMoveDataId,
    #[indexed]
    pub task_id: TaskId,
    pub destination: VoxelCoord,
}
```

Follows the existing extension table convention (`TaskHaulData`,
`TaskCraftData`, `TaskAttackTargetData`). Uses auto-increment PK with indexed
`task_id` FK (cascade on task delete).

The transient combat target is tracked on the base `Task` row's
`target_creature` field (see below), not in the extension table.

### Target tracking via base Task row

When the creature detects a hostile and engages, set the base `Task` row's
`target_creature` to `Some(target_id)`. This leverages the existing dynamic
pursuit infrastructure in the activation loop (sim.rs): the task's `location`
is automatically updated to the target's current nav node, and paths are
invalidated when the target moves. When the target dies or goes missing, set
`target_creature` back to `None` and restore `location` to the destination
nav node to resume walking.

This means the `current_target` field in the extension table is unnecessary —
the base `Task.target_creature` serves the same purpose while also enabling
automatic repathfinding. The extension table only needs `destination` (the
walk target to return to after combat).

**Dependency on "dead creatures remain in DB" invariant:** The dynamic pursuit
code in `execute_task_behavior()` (sim.rs ~line 2340) handles a missing target
(no creature row or `current_node` is `None`) by calling `interrupt_task()`.
Dead creatures currently remain in the DB with `vital_status=Dead` and retain
their `current_node`, so the pursuit code harmlessly updates the task location
to the corpse's position. The attack-move behavior handler then detects the
dead target via `vital_status` and clears `target_creature`. If a future change
removes dead creatures from the DB (e.g., corpse cleanup), it MUST first clear
`target_creature` on any tasks referencing them, or the pursuit code will
abort the entire attack-move task. This fragility is shared with
`AttackTarget` tasks.

**FK semantics:** `Task.target_creature` has restrict-on-delete FK semantics
(db.rs line 1012: `fks(target_creature? = "creatures")`). The sim must clear
this field before removing any referenced creature from the DB. This is safe
today because dead creatures are never removed, but it is another facet of the
same invariant dependency described above — shared with `AttackTarget`.

### Preemption

Attack-move follows the same preemption rules as AttackTarget:

| Task kind   | Origin           | Preemption level      |
|------------|------------------|-----------------------|
| AttackMove | Autonomous       | AutonomousCombat (5)  |
| AttackMove | Automated        | AutonomousCombat (5)  |
| AttackMove | PlayerDirected   | PlayerCombat (6)      |

Player-issued attack-move (via the F hotkey) uses `TaskOrigin::PlayerDirected`,
giving it `PlayerCombat` priority — it can preempt autonomous combat, mood,
survival, and work tasks.

**Flee exemption:** Player-directed attack-move has `PlayerCombat` preemption
level, which exempts the creature from the `should_flee()` check. This is
intentional — player-commanded attack-moving elves should not flee. This
guarantee depends on the preemption level; a test should verify it.

`Autonomous` and `Automated` origins are included for exhaustive matching in
`preemption_level()` even though they are not currently created by any code
path.

### Behavior loop

On each activation:

1. **Check current target.** If `target_creature` is `Some(id)`:
   - Look up the creature. If dead or missing, set `target_creature = None`,
     restore `location` to the destination nav node, and fall through to step 2.
   - If alive, pursue and engage using shared combat helpers (see "Combat
     function refactoring" below): melee strike if in range, ranged shoot if
     LOS + ammo + range, otherwise pathfind toward target.
   - **Path failure during engagement:** If pathfinding to the target fails,
     immediately disengage — set `target_creature = None` and restore
     `location` to the destination nav node. Do NOT retry (no failure counter).
     This differs from `AttackTarget`, which increments `path_failures` on
     `TaskAttackTargetData` and cancels after `attack_path_retry_limit`. Since
     attack-move has no `TaskAttackTargetData`, it needs its own walking logic
     for the engaged state (see "Walking logic during engagement" below).
   - Do NOT resume walking while engaged.

2. **Scan for hostiles.** If `target_creature` is `None`, check for hostiles
   within the creature's `hostile_detection_range_sq` (species config). Use the
   existing `detect_hostile_targets()` spatial index scan.
   - If hostiles detected: pick the nearest by squared euclidean distance. Ties
     broken by `CreatureId` ordering for determinism (no PRNG needed).
     Set `target_creature` to the chosen target and engage.
   - If no hostiles: continue walking toward destination.

3. **Walk toward destination.** Pathfind and move one step toward the
   destination nav node. Standard A*/Dijkstra pathfinding.

4. **Arrival check.** Only checked when `target_creature` is `None`. If the
   creature reaches the destination nav node, the task completes. When engaged
   (`target_creature` is `Some`), arrival at the destination is irrelevant —
   the creature is pursuing the target, not walking to the destination.

**Note on drift:** There is no leash range in this initial implementation. A
creature may drift from the destination while pursuing a target. After killing
the target, it scans again from its current position and may engage another
hostile further away. This is acceptable for now; a leash radius can be added
later if drift proves problematic in practice.

### Death and interruption

- If the creature performing attack-move dies: standard death handler cleans up
  the task (`interrupt_task`, cascade delete removes extension row).
- If interrupted by a higher-priority task: standard preemption. The
  attack-move task is cancelled (non-resumable), matching AttackTarget behavior.
- Attack-move is listed as non-resumable in the death/interruption handler,
  alongside AttackTarget and AcquireItem.

### Combat function refactoring

The existing combat functions `try_attack_target_combat()` (sim.rs line 4093)
and `execute_attack_target_at_location()` (sim.rs line 4006) both begin by
looking up `task_attack_target_data(task_id)` to extract the target
`CreatureId`. Attack-move tasks have no `TaskAttackTargetData` row — the
target lives in `Task.target_creature`.

**Approach:** Refactor these functions to accept a `target_id: CreatureId`
parameter instead of reading from the extension table. The call sites in the
`AttackTarget` activation handler extract `target_id` from
`TaskAttackTargetData` and pass it in; the `AttackMove` handler extracts it
from `Task.target_creature` and passes it in. The combat logic itself
(melee range check, ranged LOS check, action cooldowns) is unchanged.

Additionally, `try_attack_target_combat()` resets `path_failures` on
`TaskAttackTargetData` when melee contact is made (sim.rs line 4132). After
refactoring, this side effect should be handled by the caller (the
`AttackTarget` handler resets its failure counter; the `AttackMove` handler
has no failure counter to reset).

### Walking logic during engagement

The existing `walk_toward_attack_target()` (sim.rs line 4186) is tightly
coupled to `TaskAttackTargetData`: it reads and increments `path_failures`,
and cancels the task when `attack_path_retry_limit` is reached (sim.rs lines
4261-4275). It also resets the failure counter on successful path usage
(sim.rs lines 4220-4230).

Attack-move needs different path-failure semantics: on any failure, immediately
disengage (clear `target_creature`, restore `location` to destination) with no
retry counter. Two options:

1. **Separate walking function** for attack-move engagement that handles path
   failure by disengaging rather than incrementing a counter.
2. **Parameterize** `walk_toward_attack_target()` to accept an on-failure
   callback or mode enum.

Option 1 is simpler and avoids complicating the existing AttackTarget code
path. The walking logic itself (pathfinding, edge traversal, move action
creation) can be extracted into a shared helper if desired, but the
failure-handling policy should be separate per task kind.

## GDScript-side design

### Input handling

The attack-move input lives in `selection_controller.gd`, alongside the
existing right-click context commands (`_try_right_click_command`). It reuses
the same raycast logic, creature position lookup, and bridge dispatch pattern.

Flow:

1. Player presses F with creatures selected → enter attack-move mode. Show a
   visual indicator (cursor change or status text — details at implementation
   time).
2. Player left-clicks → raycast to determine target (ground voxel or creature
   position). Dispatch `AttackMove` command for each selected creature via
   SimBridge. Exit attack-move mode.
3. ESC or F again → cancel, exit attack-move mode.

Attack-move mode should be cancelled when entering construction mode or
placement mode, matching how selection is handled.

### SimBridge

Add `attack_move(creature_id: String, x: i32, y: i32, z: i32)` method to
`SimBridge`, mirroring the existing `attack_creature()` and
`directed_goto()` patterns.

## Implementation plan

1. **Remove R/F camera bindings** (project.godot, orbital_camera.gd docstring,
   keybind_help.gd): remove R and F from focal_up/focal_down, leaving only
   Page Up/Page Down.
2. **Sim-side:**
   - Add `TaskAttackMoveDataId`, `TaskAttackMoveData` table (destination only),
     `TaskKindTag::AttackMove`, `TaskKind::AttackMove` (unit variant),
     `SimAction::AttackMove`.
   - Add `TaskKindTag::AttackMove` arm to `display_name()` (db.rs line 145).
   - Add `TaskKind::AttackMove` arm to `insert_task()` (sim.rs line 6920) to
     create the `TaskAttackMoveData` extension row. Update the "no extra data"
     comment at line 7141.
   - Add `TaskKindTag::AttackMove` to the non-resumable group in the
     `interrupt_task` match (sim.rs line 5379), alongside `AttackTarget`.
   - Refactor `try_attack_target_combat()` and
     `execute_attack_target_at_location()` to accept a `CreatureId` parameter
     (see "Combat function refactoring" above).
   - Implement attack-move walking logic with immediate-disengage path failure
     handling (see "Walking logic during engagement" above).
   - Command processing: validate destination, create task with
     `location = destination nav node`.
   - Activation logic: engage/scan/walk/arrive loop using `target_creature` on
     base Task row.
   - Preemption mapping.
   - If `SimDb` uses `#[schema_version(N)]` at implementation time, bump it.
     (Currently `SimDb` has no schema version attribute; tabulosity's
     missing-tables-default-to-empty ensures old saves load the new table as
     empty regardless.)
3. **SimBridge:** Add `attack_move()` method.
4. **GDScript:** Add F-key attack-move mode to selection_controller.gd,
   dispatch commands via SimBridge, visual mode indicator.
5. **Tests:**
   - Task creation and serde roundtrip.
   - Walk to destination with no hostiles (degenerates to GoTo).
   - Detect hostile en route, engage, kill, resume walking, arrive.
   - Target dies mid-engagement → clear target, resume walking.
   - Path failure during engagement → disengage, resume walking.
   - Multiple hostiles in range → nearest by distance selected.
   - Player-directed attack-move is exempt from flee.
   - Preemption: attack-move preempts lower-priority tasks.
   - Death during attack-move: task cleaned up, extension row cascade-deleted.
   - Unreachable destination: task creation should validate.
   - Save/load with active target_creature preserved.
