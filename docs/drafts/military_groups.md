# Military Groups — Design Draft

Design draft for the military group data model, sim integration, and in-game
management UI. This is a standalone feature (F-military-groups) that builds on
the existing combat infrastructure (hostile detection, flee behavior, attack
tasks).

**Parent design:** `combat_military.md` (this doc refines and supersedes its
section 1 "Military Groups" for the initial implementation).

---

## 1. Data Model

### MilitaryGroup Table

```rust
auto_pk_id!(MilitaryGroupId);  // auto-increment u64 newtype

#[derive(Table)]
pub struct MilitaryGroup {
    #[primary_key(auto_increment)]
    pub id: MilitaryGroupId,
    #[indexed]
    pub civ_id: CivId,
    pub name: String,
    pub is_default_civilian: bool,
    pub hostile_response: HostileResponse,
}

// Lives alongside the MilitaryGroup table definition in db.rs.
// Cross-reference: CombatAI in species.rs determines non-civ creature behavior;
// HostileResponse determines civ creature behavior via military group membership.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostileResponse {
    Fight,
    Flee,
}
```

**FK declarations in SimDb:**

- `MilitaryGroup.civ_id -> Civilization` (cascade: deleting a civ deletes its
  groups).
- `Creature.military_group -> MilitaryGroup` (nullify: deleting a group
  unassigns its creatures -- they return to implicit civilian status).

**`is_default_civilian` invariant:** Exactly one group per civ has
`is_default_civilian = true`. Tabulosity does not support partial unique
indexes, so this invariant is enforced in sim code at group creation time
(reject if a civilian group already exists for the civ). The civilian group
cannot be deleted or have its `is_default_civilian` flag changed.
**`is_default_civilian` is write-once** -- it is set at creation time and
immutable thereafter. No command can modify it. Add a `debug_assert!` in
`should_flee()` (or a periodic sim integrity check) that verifies exactly one
group per civ has `is_default_civilian = true`, to catch invariant violations
during development.

### Implicit Civilian Membership

The parent design doc assigns all elves to the Civilians group at spawn. This
draft simplifies: **`military_group: None` means civilian.** The Civilians
group row exists for its *settings* (notably `hostile_response`) but creatures
are not explicitly assigned to it.

- `creature.military_group == None` -> creature is a civilian, governed by the
  civ's default civilian group settings.
- `creature.military_group == Some(group_id)` -> creature is assigned to that
  specific group.
- Non-civ creatures (goblins, wildlife) always have `military_group: None` and
  their behavior comes from `combat_ai` in `SpeciesData`.

**Note on the dual meaning of `None`:** The meaning of
`creature.military_group == None` depends on `creature.civ_id`:

- `civ_id = Some(_), military_group = None` -> civilian (governed by civ's
  default civilian group).
- `civ_id = None, military_group = None` -> wild/unaffiliated (behavior comes
  from species `combat_ai`).

This discrimination is always resolved by checking `civ_id` first. Document
this in the `Creature` struct's field docstring for `military_group`.

**Civilian count** is computed as:
`(total alive civ creatures) - (alive civ creatures with military_group != None)`

This avoids needing to look up the Civilians group ID at elf spawn time and
makes "reassign back to civilian" a simple `military_group = None`.

### Creature Field

Add to `Creature` struct:

```rust
/// The creature's military group assignment. For civ creatures (civ_id is
/// Some), None means civilian (governed by the civ's default civilian group
/// settings). For non-civ creatures (civ_id is None), this is always None
/// and behavior comes from the species' combat_ai instead.
pub military_group: Option<MilitaryGroupId>,  // nullable FK, nullify on group delete
```

**Death behavior:** Group assignment is preserved on death (`vital_status =
Dead`). The death handler does not clear `military_group`, consistent with
how it preserves `civ_id` and other affiliation fields. This supports future
memorial/fallen-soldiers features. All group membership queries must filter
by `vital_status = Alive` (see section 3).

### Worldgen

During worldgen, after the player civilization is created:

1. Create the Civilians group:
   `{ civ_id, name: "Civilians", is_default_civilian: true, hostile_response: Flee }`
2. Create the Soldiers group:
   `{ civ_id, name: "Soldiers", is_default_civilian: false, hostile_response: Fight }`

Creatures spawned during worldgen and via debug UI get
`military_group: None` (implicit civilian). This matches current behavior
where all elves flee.

---

## 2. Sim Integration

### should_flee() Update

Replace the current hardcoded "all civ creatures flee" logic:

```
fn should_flee(creature_id, species) -> bool:
    // Non-civ: check combat_ai as before (FleeOnly -> flee)
    // Civ creature with military_group = Some(group_id):
    //   look up group.hostile_response -> Flee means flee, Fight means don't
    // Civ creature with military_group = None (civilian):
    //   look up civ's default civilian group hostile_response
    // Existing override: creatures with PlayerCombat-level tasks don't flee
```

### wander() Update — Fight-Group Auto-Engage

Currently, `wander()` only calls `hostile_pursue()` for creatures with
`CombatAI::AggressiveMelee` or `CombatAI::AggressiveRanged`. Fight-group civ
creatures need the same behavior: when idle (no task), they should detect and
pursue hostiles autonomously.

In the `wander()` decision cascade, add a check for civ creatures whose
military group has `hostile_response: Fight`. These creatures call
`hostile_pursue()` just like aggressive species do. The updated flow:

```
fn wander(creature_id, current_node, events):
    let species = creature.species
    let combat_ai = species_data.combat_ai

    // Existing: aggressive non-civ creatures pursue hostiles.
    if combat_ai is AggressiveMelee or AggressiveRanged:
        if hostile_pursue(creature_id, current_node, species, events):
            return

    // NEW: Fight-group civ creatures also pursue hostiles.
    if creature.civ_id.is_some():
        let response = resolve_hostile_response(creature)  // group or default civilian
        if response == Fight:
            if hostile_pursue(creature_id, current_node, species, events):
                return

    random_wander(creature_id, current_node, species)
```

This means Fight-group elves will autonomously detect, pursue, and attack
hostiles using the same `hostile_pursue()` / `detect_hostile_targets()`
infrastructure that goblins/orcs/trolls use. The `should_flee()` check (which
runs before the wander/decision cascade) already handles the "don't flee"
part; this adds the "do fight" part.

**Note:** `hostile_pursue()` already handles both melee strikes (when adjacent)
and ranged attacks (bow + arrows, line-of-sight check). Fight-group elves
equipped with bows and arrows will attempt ranged attacks before pathfinding
toward the target, identical to `AggressiveRanged` behavior.

### Commands

New `SimAction` variants:

```rust
/// Create a new military group for the player's civ.
CreateMilitaryGroup { name: String },

/// Delete a non-civilian military group. Members return to civilian status.
DeleteMilitaryGroup { group_id: MilitaryGroupId },

/// Reassign a creature to a different military group, or None for civilian.
ReassignMilitaryGroup {
    creature_id: CreatureId,
    group_id: Option<MilitaryGroupId>,
},

/// Rename a military group.
RenameMilitaryGroup {
    group_id: MilitaryGroupId,
    name: String,
},

/// Change a military group's hostile response setting.
SetGroupHostileResponse {
    group_id: MilitaryGroupId,
    hostile_response: HostileResponse,
},
```

**Validation:**

- `CreateMilitaryGroup`: group belongs to the commanding player's civ.
  Default name is "Group N" (N = count of existing groups + 1). New groups
  default to `hostile_response: Fight`.
- `DeleteMilitaryGroup`: reject if `is_default_civilian` is true. Members get
  `military_group = None` (nullify FK).
- `ReassignMilitaryGroup`: reject if creature is not a civ creature, or if the
  target group belongs to a different civ. `group_id: None` means return to
  civilian status.
- `RenameMilitaryGroup`: reject if the group doesn't exist or doesn't belong
  to the player's civ. Allow renaming the civilian group.
- `SetGroupHostileResponse`: reject if the group doesn't exist or doesn't
  belong to the player's civ. Allow changing the civilian group's response
  (e.g., set civilians to Fight).

### SimEvents

New event variants:

```rust
/// Emitted when a new military group is created.
MilitaryGroupCreated { group_id: MilitaryGroupId },

/// Emitted when a military group is deleted. Generates a notification toast:
/// "Group '<name>' disbanded, N members returned to civilian duty."
MilitaryGroupDeleted { group_id: MilitaryGroupId, name: String, member_count: usize },
```

Group creation and deletion generate notifications via the existing
notification system (SimDb `notifications` table + `notification_display.gd`
toasts). Individual creature reassignments do not generate events or
notifications -- they are too frequent and low-value for toast display.

### Serde

`HostileResponse` and `MilitaryGroupId` need `Serialize`/`Deserialize`
derives. The `MilitaryGroup` table is added to `SimDb` and participates in
save/load. The new `Creature.military_group` field uses `#[serde(default)]`
so that if the field is absent during deserialization, creatures default to
`None` (civilian).

**Old save compatibility:** No migration system exists in tabulosity. Old saves
that predate the `MilitaryGroup` table will fail to load. This is acceptable.
The `#[serde(default)]` on `creature.military_group` is still useful for
potential future schema additions within the creature table but does not
guarantee old-save loading works end-to-end.

---

## 3. SimBridge API

New `#[func]` methods on `SimBridge`:

```rust
/// Returns an array of dicts, one per military group for the player's civ.
/// Each dict: { id: int, name: String, is_civilian: bool,
///              hostile_response: String, member_count: int }
/// The civilian group's member_count is the computed leftover count.
/// All member counts only include creatures with vital_status = Alive.
fn get_military_groups() -> VarArray

/// Returns an array of dicts for living members of a specific group.
/// Only creatures with vital_status = Alive are included.
/// For the civilian group (pass id = civilian group's id), returns creatures
/// with military_group = None and vital_status = Alive.
/// Each dict: { creature_id: String, name: String, species: String }
fn get_military_group_members(group_id: i64) -> VarArray

/// Command wrappers (all route through apply_or_send):
fn create_military_group(name: GString)
fn delete_military_group(group_id: i64)
fn reassign_military_group(creature_id: GString, group_id: i64)
fn reassign_to_civilian(creature_id: GString)
fn rename_military_group(group_id: i64, name: GString)
fn set_group_hostile_response(group_id: i64, response: GString)  // "Fight"/"Flee"
```

---

## 4. UI Design

### 4a. Creature Info Panel Addition

Add a "Military Group" line to the creature info panel (between species and
HP, or after task status -- wherever feels natural). Shows the group name as a
clickable label/button. Clicking it opens the military panel scrolled to that
group's detail view.

For civilians (no explicit group), show the civilian group's current name
(looked up from the `MilitaryGroup` table) and the civilian group's actual
`MilitaryGroupId`. The creature info dict returned by `get_creature_info()`
includes:

- `military_group_id: int` -- the civilian group's actual ID for implicit
  civilians, or the assigned group's ID for explicitly assigned creatures.
- `military_group_name: String` -- the group's current name (reflects renames).

Non-civ creatures (goblins, wildlife) show nothing (no military group line).

### 4b. Military Panel (Summary Page)

Opened by the "Units" button [U] in the action toolbar (this button already
exists and emits `action_requested("Units")`).

**Toggle behavior:** Pressing [U] opens the summary page. Pressing [U] again
closes the panel entirely. If the detail page is currently showing, pressing
[U] closes the entire panel (the back button [<] is for navigating within the
panel, [U] is for toggling it). ESC from the detail page navigates back to the
summary page. ESC from the summary page closes the panel. This follows the
input precedence chain: military panel intercepts ESC before it reaches
pause_menu.

**Layout:** Right-side panel, same anchor pattern as creature_info_panel
(25% screen width, full height). Uses the same MarginContainer -> VBoxContainer
pattern. Has a close button [X] in the header. Opening the military panel
closes the creature info panel and vice versa (they share screen space).

**Content:**

- **Header:** "Military Groups" title + close [X] button.
- **Group list** (ScrollContainer -> VBoxContainer, one row per group):
  - Each row: `[Group Name]  [Member Count]  [>]`
  - The civilian group is always listed first with a "(default)" suffix or
    similar visual distinction.
  - The row is clickable -- clicking navigates to that group's detail view.
  - The ScrollContainer handles overflow when there are many groups.
- **"New Group" button** at the bottom of the list (outside the
  ScrollContainer, always visible). Creates a new group with a default name
  ("Group N") and `hostile_response: Fight`, then navigates to its detail view.

### 4c. Military Panel (Group Detail Page)

Navigated to by clicking a group row on the summary page, or by clicking the
military group label in the creature info panel.

**Layout:** Same right-side panel, replacing the summary page content. Has a
back button [<] in the header to return to the summary page.

**Header:** Group name displayed as a label with a [Rename] button next to it.
Clicking [Rename] replaces the label with a LineEdit for inline editing; Enter
confirms and sends `RenameMilitaryGroup`, Escape cancels and restores the
label. The civilian group's name is also renameable.

**Left column (members list, ~60% width):**

- Scrollable list of group members (ScrollContainer -> VBoxContainer).
- Each member row: `[Name]  [Reassign]`
  - Name is the creature's Vaelith name (or "Species #N" fallback).
  - "Reassign" button opens the reassignment overlay (section 4d).
- For the civilian group: lists all alive civ creatures with
  `military_group = None`.
- For other groups: lists alive creatures with
  `military_group = Some(this_group)`.

**Right column (settings, ~40% width):**

- **Hostile Response:** Label + two toggle buttons (Fight / Flee). The active
  one is highlighted. Clicking the other sends `SetGroupHostileResponse`.
- **Delete Group** button (only for non-civilian groups). Confirms via a
  small "Are you sure?" inline prompt or just deletes immediately (members
  return to civilian). After deletion, navigate back to summary page.
- The settings area is mostly empty for now -- future settings (armor policy,
  weapon policy, behavior) will fill this space.

### 4d. Reassignment Overlay

Opened by clicking "Reassign" on a member in the group detail page.

**Layout:** Modal overlay (same pattern as save_dialog / load_dialog):
ColorRect full-screen semi-transparent background + CenterContainer +
PanelContainer.

**Content:**

- **Header:** "Reassign [Creature Name]"
- **Group list:** One button per military group (including "Civilians"). The
  creature's current group is visually distinguished (grayed out or marked
  with a checkmark). Clicking a different group sends
  `ReassignMilitaryGroup` and closes the overlay.
- **Cancel button** at the bottom. Also closes on Escape key.

### 4e. Data Flow

**Summary page refresh:** The summary page calls
`bridge.get_military_groups()` when opened and on each `_process` frame (or
at a throttled interval, e.g., every 0.5s) to keep member counts current.

**Detail page refresh:** The detail page calls
`bridge.get_military_group_members(group_id)` with the same refresh pattern.
Member lists can change due to creature death, new spawns, or reassignment by
other players (multiplayer).

**Creature info panel:** The group name for the selected creature comes from
the existing `get_creature_info()` dict -- add `military_group_name` and
`military_group_id` fields. For implicit civilians, `military_group_id` is the
civilian group's actual ID and `military_group_name` is its current name
(reflecting any renames).

---

## 5. Deferred / Future Work

- **Armor/weapon/food policies** on groups (design doc section 1 "Group
  Configuration"). Settings area in the detail page is intentionally spacious
  to accommodate these.
- **Behavior suggestion** (None/Train/Patrol) -- influences autonomous task
  selection.
- **Equipment logistics** -- auto-generate logistics wants from group policy.
- **NPC civ group management** -- NPC civs use default groups only.
- **Group hotkeys** -- select all members of a group.
- **Patrol routes** -- draw patrol paths on the map, assign to groups.
- **Bulk reassignment** -- "Select All" + "Reassign All" affordance on the
  detail page for moving many creatures between groups at once.

---

## 6. Implementation Plan

### Phase A: Data Model + Sim Logic
1. Add `HostileResponse` enum and `MilitaryGroup` table to `db.rs`.
2. Add `military_group: Option<MilitaryGroupId>` to `Creature` with field
   docstring documenting the dual meaning of `None` (civilian vs. wild).
3. Create default groups during worldgen.
4. Add all `SimAction` variants and `process_command` handlers.
5. Update `should_flee()` to check group hostile_response. Add `debug_assert!`
   for the one-civilian-per-civ invariant.
6. Update `wander()` to call `hostile_pursue()` for Fight-group civ creatures.
7. Add `MilitaryGroupCreated` and `MilitaryGroupDeleted` SimEvent variants
   with notification integration.
8. Tests:
   - Group creation (worldgen creates default groups, manual creation).
   - Creature reassignment (to group, back to civilian, between non-civilian
     groups).
   - `should_flee` with Fight group (does not flee).
   - `should_flee` with Flee group (does flee).
   - `should_flee` with PlayerCombat override (Flee group + PlayerCombat task
     does not flee -- existing behavior preserved).
   - Reassign during active flee: creature reassigned to Fight group stops
     fleeing on next activation.
   - Fight-group civ creature auto-engages hostiles via `wander()`.
   - Serde roundtrip for `HostileResponse`, `MilitaryGroupId`,
     `MilitaryGroup` table.
   - FK cascade (civ deletion cascades to groups).
   - FK nullify (group deletion nullifies creature.military_group -- all
     former members have `military_group = None`).
   - Civilian group deletion rejection.
   - Dead creature not counted in group membership queries.
   - Dead creature preserves group assignment.
   - Cross-civ reassignment rejection (creature from civ A cannot join civ B
     group).
   - Non-civ creature reassignment rejection (goblin cannot join any group).
   - Rename civilian group -- verify the new name is returned in creature
     info.

### Phase B: SimBridge API
9. Add query methods (`get_military_groups`, `get_military_group_members`)
   with `vital_status = Alive` filtering.
10. Add `military_group_name`/`military_group_id` to creature info dict.
    Civilians get the civilian group's actual ID and current name.
11. Add command wrapper methods.

### Phase C: UI
12. Military panel summary page (opens via Units button) with ScrollContainer
    for group list.
13. Group detail page with ScrollContainer for member list, [Rename] button,
    and hostile_response toggle.
14. Reassignment overlay.
15. Creature info panel: add military group label with click-to-navigate.
16. Wire Units button [U] toggle (open/close) and ESC navigation (detail ->
    summary -> close).
