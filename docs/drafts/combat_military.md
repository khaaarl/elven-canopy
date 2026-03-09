# Combat & Military — Design Draft (v10)

Design draft for the combat and military system. Covers military groupings,
RTS-style selection and commands, projectile physics, combat actions,
HP/damage, and hostile creature AI. This is a phased design — the first
implementation pass focuses on core mechanics; advanced features are noted
for future work.

**Depends on:** `encyclopedia_civs.md` (civilizations, `CivId`, creature
`civ_id` field). Military groups are per-civilization, not global.

---

## 1. Military Groups

Military groups belong to **civilizations** (see `encyclopedia_civs.md`).
Every creature that belongs to a civilization is assigned to exactly one of
that civ's military groups. Creatures without a civilization (wildlife,
unaffiliated hostiles) do **not** have military groups — their combat
behavior is configured via `SpeciesData` (see §7).

### Default Groups

Each civilization gets two default military groups during worldgen (created
after the civilization itself, before any creatures are spawned):

| Group       | Default Behavior | Default Combat Response |
|-------------|-----------------|------------------------|
| Civilians   | None (live your life) | Flee from hostiles |
| Soldiers    | Train | Fight hostiles |

The player can create additional groups for their civ (e.g., "Archers",
"Home Guard"). Each group is player-named and independently configured.
NPC civs use their default groups; NPC group management is future work.

### Group Configuration

Each group specifies:

- **Armor policy:** What armor, if any, members should wear (initially: none
  vs. whatever's available; future: specific armor types).
- **Weapon policy:** What weapon(s) members should carry (bow, melee weapon,
  both, none).
- **Food carry policy:** How much food to carry around (0 = eat at
  kitchens/storehouses only).
- **Behavior suggestion:** None (live normally), Train, or Patrol (future).
  This influences autonomous task selection — a "Train" suggestion makes the
  elf prefer training tasks over other idle activities.
- **Hostile encounter response:** Fight or Flee. Determines default behavior
  when a hostile creature is spotted, unless overridden by a direct player
  command.

### Data Model

```rust
auto_pk_id!(MilitaryGroupId);  // auto-increment u64 newtype

#[derive(Table)]
pub struct MilitaryGroup {
    #[primary_key(auto_increment)]
    pub id: MilitaryGroupId,
    #[indexed]
    pub civ_id: CivId,             // which civilization owns this group
    pub name: String,
    pub armor_policy: ArmorPolicy,
    pub weapon_policy: WeaponPolicy,
    pub food_carry: u32,           // max food items to carry
    pub behavior: GroupBehavior,
    pub hostile_response: HostileResponse,
}

pub enum ArmorPolicy { None, Any }  // expand later with armor types
pub enum WeaponPolicy { None, Bow, Melee, Any }
pub enum GroupBehavior { None, Train }
pub enum HostileResponse { Fight, Flee }
```

SimDb FK declarations:

```rust
// MilitaryGroup → Civilization (cascade: deleting a civ deletes its groups)
// Creature → MilitaryGroup (nullify: deleting a group unassigns creatures)
```

Each `Creature` gains an `Option<MilitaryGroupId>` field. This is `Some`
for creatures in a civilization and `None` for wild/unaffiliated creatures.
At spawn, elves are assigned to their civ's Civilians group. Goblins,
wildlife, etc. spawned without a civ have `military_group: None`.

**Debug spawning:** Debug-spawned elves (via the debug UI) are
automatically assigned to the player's civ and the Civilians military
group. This matches the existing spawn flow where elves are the player's
species. Non-elf debug spawns (goblins, orcs, trolls, etc.) remain
civ-less with `military_group: None` — their combat behavior comes from
`combat_ai` in `SpeciesData`.

### Future: Equipment Logistics

A future enhancement could add a toggle or button that auto-generates
logistics wants from a group's equipment policy (e.g., 5 soldiers wanting
bows → 5 bow logistics wants). This is explicitly out of scope for the
initial pass.

---

## 2. RTS Selection and Commands

Selection state (which creatures are selected, box selection rectangle) is
**client-local UI state**, not sim state. It is not included in save files,
not synchronized in multiplayer. Commands issued from a selection are
translated to `SimCommand` variants and sent through the normal command
pipeline.

### Box Selection

Currently, creatures are selected one at a time via left-click raycasting.
This extends to support click-and-drag box selection:

- **Single click:** Selects one creature (existing behavior).
- **Click and drag:** Draws a 2D selection rectangle on screen. All friendly
  creatures whose screen-space positions fall within the rectangle are
  selected.
- **Shift+click / Shift+drag:** Add to current selection.
- **Selected group display:** When multiple creatures are selected, show a
  group info panel (portraits/icons, count by species) instead of the
  single-creature info panel.

### Right-Click Commands (Starcraft-style)

Context-sensitive right-click on the world:

| Target | Action |
|--------|--------|
| Ground location (reachable) | Move-to (one-shot, like existing GoTo) |
| Friendly creature | Move to that creature's location (one-shot, not follow) |
| Hostile creature | Attack target (pursue until dead) |

All commands apply to the entire current selection. Each selected creature
gets its own task instance (move-to-location or attack-target).

### SimCommand Variants for Combat

New `SimAction` variants to support combat commands:

```rust
AttackCreature { attacker: CreatureId, target: CreatureId },
AttackMove { creature: CreatureId, destination: VoxelCoord },
```

These flow through the standard command pipeline. `AttackCreature` creates
an `AttackTarget` task (see §5). `AttackMove` creates an `AttackMove` task
(see below).

### Attack-Move

A dedicated hotkey (e.g., `A`) enters attack-move mode:

- **A + left-click on ground:** Attack-move to that location.
- **A + left-click on a creature (friendly or hostile):** Treated as
  attack-move to that creature's location. This avoids accidental friendly
  fire — elves won't deliberately target allies.

Attack-move is a single task kind (`TaskKindTag::AttackMove`) with an
extension table:

```rust
auto_pk_id!(TaskAttackMoveDataId);

#[derive(Table)]
pub struct TaskAttackMoveData {
    #[primary_key(auto_increment)]
    pub id: TaskAttackMoveDataId,
    #[indexed]
    pub task_id: TaskId,          // FK to tasks (cascade on delete)
    pub destination: VoxelCoord,  // where to walk
    pub current_target: Option<CreatureId>,  // hostile engaged en route (plain ID, not FK)
}
```

**Note on PK pattern:** This uses an auto-increment PK with an indexed
`task_id` FK, matching the existing extension table convention in the
codebase (e.g., `TaskHaulData`, `TaskCookData`). A cleaner pattern would
use the parent's PK as the child's PK (1:1 relationship), but that requires
tabulosity support for non-auto-increment PKs referencing another table's PK
— see future tracker item `F-tab-parent-pk`.

**Behavior:** The creature walks toward `destination`. On each activation,
it checks for hostiles within its species-configured
`hostile_detection_range_sq`. If a hostile is detected:

1. Set `current_target` to that hostile's ID.
2. The creature's combat actions (§5/§6) handle the actual fighting — the
   task just tracks which target to pursue.
3. On each activation, poll `current_target`'s `vital_status`. If `Dead`
   or if the creature row is missing (future dead-hostile cleanup may
   delete rows), nullify `current_target` and resume walking toward
   `destination`.
4. On arrival at `destination` with no active target, the task completes.

### Command Applicability

All commands work on any selected creatures regardless of military group.
A civilian can be ordered to attack — but under default Flee settings, if
the player doesn't give explicit commands, civilians will run from hostiles
on their own. Player-issued commands (via `SimAction`) create tasks with
`TaskOrigin::PlayerDirected`, which the preemption system treats as higher
priority than autonomous responses. The military group's `HostileResponse`
only governs autonomous behavior — what the creature does when it detects a
hostile without having received a direct order.

---

## 3. Health and Damage

### Hit Points

Each creature gains `hp`, `hp_max`, and `vital_status` fields:

```rust
// In Creature (db table)
pub hp: i64,
pub hp_max: i64,
#[indexed]
pub vital_status: VitalStatus,
#[serde(default)]
pub last_melee_tick: u64,       // tick of last melee strike (0 = never)
#[serde(default)]
pub last_shoot_tick: u64,       // tick of last ranged shot (0 = never)

// In SpeciesData (config)
pub hp_max: i64,
```

Cooldown checks use `current_tick - last_melee_tick >= melee_interval_ticks`
(and similarly for `last_shoot_tick` / `shoot_cooldown_ticks`). The
`#[serde(default)]` annotations ensure save compatibility — existing saves
without these fields deserialize with 0, meaning no cooldown is active.

```rust
pub enum VitalStatus {
    Alive,
    Dead,
    // Future: Ghost, SpiritInTree, Undead, etc.
}
```

Suggested initial values:

| Species  | HP   |
|----------|------|
| Elf      | 100  |
| Goblin   | 80   |
| Orc      | 150  |
| Troll    | 400  |

No damage types in the initial pass — damage is a single integer subtracted
from HP. Damage types (piercing, blunt, slashing) and armor mitigation are
future work.

### Death

When HP reaches 0, the creature's `vital_status` transitions to `Dead`.
**The creature row is NOT deleted from SimDb.** This is a critical design
decision — dead creatures persist in the table. This supports future states
(ghost, spirit absorbed into tree, undead raised by necromancy, etc.) and
preserves history for UI/narrative purposes.

**Because the creature row is not deleted, there are NO foreign key
violations from death.** The creature's `current_task`, `assigned_home`,
`inventory_id`, `military_group`, and `civ_id` fields all remain valid
references. Tasks referencing the creature as a target use plain
`CreatureId` fields (not FKs), so they simply check `vital_status` on
each activation to detect death. Item reservations (`reserved_by: TaskId`)
remain valid because tasks are unassigned, not deleted — the task either
returns to `Available` or is cancelled (which clears reservations through
existing cleanup logic), but the task row itself persists until completed.

On death:

- Set `vital_status = Dead`.
- Set `current_task = None`. Handle the task per-kind (**task-specific
  cleanup runs FIRST**, before generic inventory drop):
  - **Resumable tasks** (Build, Furnish): return to `Available` for another
    creature to claim. Clear any item reservations held by this task.
  - **Haul in transit:** drop carried items as a ground pile at death
    location, clear item reservations, cancel task.
  - **Non-resumable tasks** (personal: EatBread, EatFruit, Sleep, Mope,
    AcquireItem, AttackTarget, AttackMove): cancel outright, clear
    reservations.
  - **Cook, Craft:** non-resumable — cancel, clear input item reservations.
  - **Harvest:** non-resumable — cancel (no reservations to clear).

  The above list must be exhaustive over all `TaskKindTag` variants. The
  unified task interruption system (tracked separately) will provide a
  single entry point for task cancellation with per-kind cleanup — the
  death handler calls that system rather than maintaining its own per-kind
  list.
- Remove from creature spatial index.
- Drop all remaining inventory items as a ground pile at death location.
  Because task-specific cleanup already ran (e.g., haul dropped its carried
  items, reservations are cleared), this step sees a reduced or empty
  inventory — no duplication occurs.
- Clear `assigned_home` (free the bed for another creature).
- Emit a `CreatureDied { id, species, position, cause }` sim event.
- Scheduled events (`CreatureActivation`, `CreatureHeartbeat`) that fire
  after death check `vital_status != Alive` and do NOT reschedule the next
  event. The activation/heartbeat chain terminates — dead creatures have
  no further scheduled events.

**Filtering burden:** Dead creatures remain in the `Creature` table. All
queries that iterate creatures must filter by `vital_status == Alive` —
this includes task assignment, heartbeat processing, logistics counting,
rendering queries, etc. The `#[indexed]` annotation on `vital_status`
enables efficient filtering. This is a pervasive change, similar in scope
to the upcoming civilization-wide filtering (`civ_id`). For long-running
games with many hostile waves, consider periodic cleanup of dead hostile
creatures (e.g., cull `Dead` hostiles older than N ticks) to prevent
unbounded table growth — but this is not needed in the initial pass.

Dead creatures are excluded from:
- Task assignment and autonomous behavior.
- Hostile detection scans and target selection.
- Combat actions (cannot attack or shoot).
- Spatial index (removed on death, not re-added).
- Rendering (Godot filters by `vital_status`).
- Pathfinding obstruction (dead creatures don't block movement).

Future: surviving elves gain a negative thought when an ally dies. Different
`VitalStatus` variants could trigger different behaviors (ghosts wander,
spirits generate mana, etc.).

### Healing

Out of scope for the initial pass. Future: rest-based regeneration, healer
elves, healing items.

---

## 4. Projectile System (Arrows)

The most architecturally novel subsystem. Arrows are physics-simulated
entities that move through voxel space on ballistic trajectories.

### Projectile Entity

Arrows in flight are a new entity type — not creatures, not items. They
exist only while airborne.

```rust
auto_pk_id!(ProjectileId);  // auto-increment u64 newtype

#[derive(Table)]
pub struct Projectile {
    #[primary_key(auto_increment)]
    pub id: ProjectileId,
    pub shooter: CreatureId,       // not FK — shooter may die mid-flight
    pub position: SubVoxelCoord,   // high-precision position
    pub velocity: SubVoxelVec,     // high-precision velocity
    pub prev_voxel: VoxelCoord,    // last air voxel (for surface-hit ground pile placement)
    pub item_kind: ItemKind,       // Arrow (future: javelin, etc.)
    pub durability: i32,           // remaining durability
    pub base_damage: i64,          // damage at reference speed
    pub launch_speed_sq: i64,      // speed² at launch, for damage scaling
}
```

Note: `shooter` is stored as a plain `CreatureId`, not a tabulosity FK.
The shooter may die while the arrow is in flight — the arrow should
continue regardless. The shooter ID is used only for attribution (kill
credit, notifications).

`prev_voxel` is initialized to the shooter's voxel position at spawn time,
then updated each tick to the arrow's current voxel before advancing. When
the arrow impacts a solid surface, the ground pile is created at
`prev_voxel` (the last air voxel), not inside the solid voxel.

The `Projectile` table is added to `SimDb`. It has no FK constraints (both
`shooter` and `item_kind` are plain values). On save/load, projectiles are
serialized normally — active projectiles in flight survive a save/load
cycle.

### Sub-Voxel Coordinate System

Projectile positions use a high-precision integer coordinate system to
maintain determinism and accumulate small deltas (gravity, wind) without
floating-point drift.

- **Scale factor:** 2^30 sub-units per voxel (~1.07 billion). This
  precision is deliberately high to ensure that gravity and wind
  accumulation over long flight times (several seconds) does not lose
  significant bits. Lower precisions (2^16, 2^20) risk visible trajectory
  jitter on long arcing shots where many small gravity increments
  accumulate. 2^30 is the chosen precision; this is not negotiable without
  strong justification.
- **Type:** `i64` per axis. Range: ±8.6 billion voxels — far beyond any
  plausible world size.
- **Voxel extraction:** Arithmetic right-shift by 30 to get the containing
  voxel coord. Rust guarantees arithmetic right-shift for signed integers
  (rounds toward negative infinity), which correctly maps negative sub-voxel
  coordinates to the containing voxel. This would not be portable to
  languages where signed right-shift is implementation-defined (C/C++), but
  this is a Rust-only codebase.
- **Velocity and acceleration** use the same scale: sub-units per tick.

```rust
pub struct SubVoxelCoord {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

pub type SubVoxelVec = SubVoxelCoord;  // same type, different semantics

const SUB_VOXEL_SHIFT: u32 = 30;

impl SubVoxelCoord {
    pub fn to_voxel(&self) -> VoxelCoord {
        VoxelCoord {
            x: (self.x >> SUB_VOXEL_SHIFT) as i32,
            y: (self.y >> SUB_VOXEL_SHIFT) as i32,
            z: (self.z >> SUB_VOXEL_SHIFT) as i32,
        }
    }

    pub fn to_render_floats(&self) -> (f32, f32, f32) {
        let scale = (1i64 << SUB_VOXEL_SHIFT) as f64;
        (
            (self.x as f64 / scale) as f32,
            (self.y as f64 / scale) as f32,
            (self.z as f64 / scale) as f32,
        )
    }
}
```

### Event-Driven Integration

The sim is event-driven (priority queue), not fixed-timestep. Projectiles
integrate into this model via a **batched tick event:**

- When a projectile is spawned, schedule a `ProjectileTick` event for the
  next tick **if and only if** the projectile table was empty before this
  spawn (count was 0, now 1). This prevents duplicate scheduling when
  multiple archers fire on the same tick — the second spawn sees a
  non-empty table and does not schedule a redundant event.
- The `ProjectileTick` handler advances **all** active projectiles in one
  batch. After processing, if projectiles remain in the table, it
  schedules the next `ProjectileTick` for tick + 1.
- When the last projectile resolves (impact or despawn), the handler sees
  an empty table and does not schedule a follow-up event. The event stops
  firing when no arrows are in flight.

This avoids flooding the event queue (1 event per tick regardless of arrow
count) and avoids modifying the core tick loop. Cost: one priority queue
push/pop per tick while any arrows are airborne — negligible.

**Note:** This weakens the "empty ticks are free" principle during combat
with active projectiles. When arrows are in flight, every tick has work.
This is acceptable — combat is inherently an active period, and the per-tick
cost is minimal (a few integer operations per arrow). The principle holds
outside of combat.

### Per-Tick Update Loop

Projectiles update every tick (1000 Hz), making them the highest-frequency
sim entity. The update is simple and cheap per projectile:

1. **Save current voxel** as `prev_voxel` (for ground pile placement on
   surface impact).
2. **Apply acceleration** to velocity: `velocity.y -= gravity` (and wind
   components, when added). **Order matters:** velocity is updated before
   position (symplectic Euler integration). This is more stable than
   standard Euler (position then velocity) for ballistic trajectories.
   Do not reorder these steps.
3. **Apply velocity** to position: `position += velocity`.
4. **Determine containing voxel** via right-shift.
5. **Solid voxel check:** If the voxel is solid (trunk, branch, structure),
   the arrow impacts the surface → resolve as surface hit (ground pile at
   `prev_voxel`).
6. **Creature check:** Look up creatures occupying this voxel (via spatial
   index). If any are present, resolve as creature hit. Once a creature is
   hit, the projectile resolves immediately — it is not checked against
   further voxels on subsequent ticks.
7. **Bounds check:** If position is outside world extents, despawn silently
   (no ground pile — arrow is lost). **Important:** The bounds check must
   be performed on the `i64` sub-voxel coordinates BEFORE converting to
   `VoxelCoord`. Compare against world extents scaled to sub-voxel units
   (i.e., `world_size << SUB_VOXEL_SHIFT`), or equivalently, right-shift
   and check against world size as `i64` before casting to `i32`. This
   ensures out-of-bounds projectiles are caught before the `as i32`
   truncation in `to_voxel()`, which could silently wrap extreme `i64`
   values into apparently-valid `i32` coordinates.
8. If none of the above, continue to next tick.

At 1000 ticks/second with an arrow speed of ~50 voxels/second, an arrow
moves ~0.05 voxels per tick. This means the arrow will never skip a voxel,
so simple per-tick position checks are sufficient without raycasting between
positions.

**Multi-voxel creature dedup:** A large creature (e.g., troll with 2×2×2
footprint) occupies multiple voxels. An arrow moving through the troll's
volume might be "inside" the troll for many consecutive ticks. To avoid
hitting the same creature multiple times, the projectile resolves on first
creature contact and is removed from the sim. (Future: if pierce-through
is added, track a `BTreeSet<CreatureId>` of already-hit creatures.)

### Creature Spatial Index

A `BTreeMap<VoxelCoord, Vec<CreatureId>>` maintained on creature movement.
This follows the codebase convention of using `BTreeMap` over `HashMap`
everywhere in the sim crate. While the spatial index is not order-dependent
(lookups are point queries, not range scans), maintaining a single
convention avoids confusion and keeps the "no HashMap in sim" invariant
simple.

The spatial index is **derived state**, stored on `SimState` (not in
`SimDb`), marked `#[serde(skip)]`. It is rebuilt from creature positions
on load, the same pattern used for the nav graph. The rebuild function
iterates all `Alive` creatures and registers each in its occupied voxels.

**Ordering constraint:** The spatial index rebuild must run **after** the
`species_table` is populated (since footprint data comes from
`SpeciesData` in the config). The `species_table` is `#[serde(skip)]`
and rebuilt from config after deserialization. If the spatial index
rebuild runs before `species_table` is populated, the footprint lookup
for large creatures will fail. Same ordering constraint as the nav graph
rebuild — both depend on config-derived data being available.

**Note on determinism:** When an arrow enters a voxel containing multiple
creatures, the candidates are sorted by `CreatureId` before the PRNG selects
a target. `CreatureId` wraps `SimUuid` which derives `Ord` — this sorts by
UUID byte order, which is deterministic but has no semantic meaning (not
spawn order, not name order). The sort step ensures determinism regardless
of the order creatures entered the voxel. The PRNG roll is the only source
of randomness.

**Alternative considered:** A flat `Vec<Vec<CreatureId>>` indexed by flat
voxel index would be O(1) lookup but requires allocating for the entire
world volume. For current world sizes this is feasible, but the world may
eventually grow large and sparse (mostly empty air), making a flat array
wasteful. `BTreeMap` scales with occupied voxels, not world volume.

Large creatures (e.g., trolls with 2×2×2 footprint) register in all occupied
voxels. A creature with `footprint = [fx, fy, fz]` at anchor position `pos`
registers at all voxels `(pos.x + dx, pos.y + dy, pos.z + dz)` for
`dx in 0..fx, dy in 0..fy, dz in 0..fz`. Registration is updated on every
creature movement (remove from old voxels, add to new voxels).

**Maintenance hooks:** The spatial index must be updated at every point where
a creature's position changes: `wander()`, `walk_toward_task()`,
`handle_creature_movement_complete()`, `resnap_creatures()` (after
construction), creature spawn, and creature death. A centralized
`update_creature_position()` helper that handles both the `Creature.position`
field update and the spatial index update is the cleanest approach.

### Damage Calculation

Arrow damage is speed-dependent, using **integer-only arithmetic** to
maintain determinism:

```
damage = base_damage * impact_speed_sq / launch_speed_sq
```

Where `impact_speed_sq = vx*vx + vy*vy + vz*vz` (squared magnitude of
velocity at impact) and `launch_speed_sq` is stored on the projectile at
spawn time. **All intermediate calculations use `i128`** — velocity
components are `i64` at 2^30 scale, so squaring produces values up to ~2^60,
and the sum of three squared components could reach ~3×2^60. This fits in
`i128` with room to spare. Cast to `i128` before squaring. Note:
`launch_speed_sq` is stored as `i64` on the Projectile struct (the max
value ~3×2^60 fits within `i64::MAX` ≈ 9.2×10^18) to avoid serde
compatibility issues with `i128`. The widening to `i128` happens only in
this local calculation:

```rust
let vx = self.velocity.x as i128;
let vy = self.velocity.y as i128;
let vz = self.velocity.z as i128;
let impact_speed_sq: i128 = vx * vx + vy * vy + vz * vz;
let damage = (self.base_damage as i128 * impact_speed_sq / self.launch_speed_sq as i128)
    .max(1) as i64;  // minimum 1 damage to avoid ghost hits
```

**Minimum damage:** The formula produces 0 damage if the arrow is near-zero
velocity (e.g., at the apex of a lob). A floor of 1 prevents "ghost hit"
confusion where a projectile visually impacts a creature but deals no
damage.

Using squared speeds avoids the need for a square root (which would require
floating-point). The damage scales with the square of speed rather than
linearly — this somewhat amplifies the height advantage, but the effect is
physically motivated (kinetic energy scales with v²) and creates strong
tactical incentives to control high ground.

This means:

- **Shooting downward** from height: arrows accelerate with gravity, dealing
  bonus damage. Archers in the tree canopy have a natural advantage.
- **Shooting upward:** arrows decelerate against gravity, dealing reduced
  damage and reaching shorter range. Attacking elves in the tree is hard.
- **Long horizontal shots:** arrows lose height and eventually hit the
  ground, creating a natural maximum range.

### Arrow Impact Resolution

**Surface hit (solid voxel):**
- Roll random durability loss (PRNG, range configured in game config).
- If arrow survives (durability > 0): create or join a ground pile at
  `prev_voxel` (the last air voxel before impact).
- If arrow is destroyed: nothing (it shatters).

**Creature hit:**
- Apply speed-based damage to the creature.
- Roll random durability loss (separate from surface hit config).
- If arrow survives: it falls to the ground — create or join a ground pile
  at the creature's voxel position.
- If arrow is destroyed: nothing remains.

**Out of bounds:**
- Despawn. No ground pile, no event. Arrow is simply lost.

### Rendering (Godot Side)

A new `projectile_renderer.gd` (pool pattern, like creature renderers)
renders arrows as small oriented 3D meshes. `SimBridge` exposes:

```rust
/// Returns a flat array with stride 6:
/// [pos_x, pos_y, pos_z, vel_x, vel_y, vel_z, pos_x, pos_y, pos_z, ...]
/// Position: SubVoxelCoord converted to float world coords (voxel units).
/// Velocity: converted to float voxels-per-tick (divide sub-voxel by 2^30).
/// GDScript iterates with: for i in range(0, arr.size(), 6): ...
fn get_projectile_positions(&self) -> PackedFloat32Array
```

Projectile rendering interpolation: unlike creatures (which interpolate
between two discrete nav nodes), projectiles have continuous positions and
a velocity vector. The renderer can compute the interpolated position as
`position + velocity * fractional_tick_offset` using the float velocity
already returned by the bridge — no separate `prev_position` field needed.
Both position and velocity are in the same units (voxels and voxels/tick),
so the math is straightforward on the GDScript side.

---

## 5. Combat Actions: Shooting and Melee

**Shooting and melee attacks are NOT tasks.** They are **actions** — things
a creature does on its own logic during an activation, similar to how idle
creatures meander aimlessly or how creatures move from one voxel to another.
A task represents a broader goal ("go kill that goblin", "attack-move to
this location", "build this platform"). Actions are the moment-to-moment
behaviors the creature performs to accomplish those goals or to react to
immediate circumstances.

This means:

- A creature with an `AttackTarget` task pathfinds toward the target, and
  when adjacent/in-range, performs melee or ranged attack **actions**.
- A creature with no task at all can still perform combat actions if its
  military group says "Fight" and a hostile is nearby.
- A fleeing creature performs flee movement actions, not a "flee task."
- An idle creature with a bow might autonomously shoot at a visible hostile
  even without an explicit attack task, if its group behavior says Fight.

**Action timing and the activation chain:** Combat actions (melee strikes,
shooting) integrate into the creature activation chain. After performing an
action, the creature schedules its next activation at
`current_tick + cooldown_ticks`. Between strikes, the creature stands idle
(does nothing until cooldown expires). This depends on a proper action
timing system — currently the codebase conflates movement timing
(`move_end_tick`) with general action availability. A dedicated
`next_action_tick` field on `Creature` is needed to cleanly separate "when
can this creature act next" from "when does this creature finish moving."
See tracker item `F-creature-actions` for the formalization of this system.

### Melee Attack Action

When a creature is adjacent to a hostile target, it can perform a melee
strike. **Melee adjacency uses euclidean voxel distance, not nav edge
existence.** The distance check uses the **closest points of each
creature's footprint**, not anchor-to-anchor distance. For a creature at
anchor `pos` with footprint `[fx, fy, fz]`, the closest point to a
target coordinate is `(clamp(target.x, pos.x, pos.x + fx - 1),
clamp(target.y, pos.y, pos.y + fy - 1), clamp(target.z, pos.z,
pos.z + fz - 1))`. The squared euclidean distance from the closest point
of the attacker's footprint to the closest point of the target's
footprint must be ≤ `melee_range_sq` (default 2 for 1-voxel reach,
accommodating diagonal adjacency). This handles large creatures correctly
— a 2x2x2 troll adjacent to an elf measures distance from the troll's
nearest occupied voxel, not from its anchor corner. Note: squared
distance 2 covers face-adjacent offsets (dist² = 1) and 2D diagonals
like (1,1,0) (dist² = 2), but excludes the pure 3D corner diagonal
(1,1,1) (dist² = 3). This is intentional — 3D corner adjacency feels
like too much reach for melee. If melee feels too restrictive in
practice, increase `melee_range_sq` to 3. Nav edges are for pathfinding,
not melee range checking. This sidesteps the cross-graph problem
entirely — a troll (large nav graph) attacking an elf (standard nav
graph) simply compares voxel positions without consulting either graph's
edge set.

1. Check cooldown — `current_tick - last_melee_tick >= melee_interval_ticks`.
2. Apply `melee_damage` (from `SpeciesData`) to the target.
3. Emit `CreatureDamaged` sim event.
4. If target HP ≤ 0, trigger death (§3).

### Ranged Attack Action (Shooting)

When a creature has arrows in inventory **and** a bow equipped/carried
(checked via inventory `item_kind`), has LOS and range to a target (see
§5.1, §5.2), and shooting cooldown has elapsed. No bow = no shooting,
even with arrows — this ties into the `WeaponPolicy` system (§1), which
determines whether a creature should acquire and carry a bow.

1. Compute aim velocity via iterative guess-and-simulate (§5.2).
2. Consume one arrow from inventory.
3. Spawn a `Projectile` entity.
4. Emit `ProjectileLaunched` sim event.
5. Enter cooldown (`shoot_cooldown_ticks`).

### Task System Integration: Moving Targets

Combat tasks introduce a case where the task's destination is a creature
rather than a fixed location. This is a significant departure from the
existing task system, which assumes a static `location: NavNodeId` —
creatures compute a path once on task claim and walk it.

**The `Task` base table's `location` field stays as `NavNodeId`.** It is
NOT made optional. For `AttackTarget`, `location` is set to the target's
current position at task creation time. For `AttackMove`, `location` is
the walk destination. The target creature reference lives in the extension
tables (`TaskAttackTargetData.target`, `TaskAttackMoveData.current_target`)
— not on the base table. This preserves the decomposition pattern where
variant-specific data lives in extension tables, keeping the base `Task`
row small and generic.

**Dynamic repathfinding** is needed for `AttackTarget` and the engagement
phase of `AttackMove`, since the target moves. This is a foundational
change to the task/pathfinding system — see the tracker for the dedicated
feature covering this work. Key design points that feature must resolve:

- **Repath frequency:** How often does the creature recompute A* toward
  the target? Every activation is too expensive. A reasonable approach:
  repath when the target has moved more than N voxels from the path's
  original destination, or when the creature reaches its current path
  endpoint and the target isn't there.
- **Path staleness check:** On each activation, compare the target's
  current position to the path's destination. If they differ by more than
  a threshold, recompute. If the target is adjacent, skip pathing entirely
  and proceed to combat actions.
- **`task.location` updates:** When repathfinding, update the task's
  `location` to the target's current position. This keeps UI/rendering
  code that reads `task.location` working correctly.
- **Integration with the activation chain:** The existing
  `walk_toward_task()` walks a pre-computed path. For pursuit tasks, the
  activation logic must check path validity before walking.

This is explicitly **not a minor change** — it touches task claiming,
pathfinding, and the activation chain. It is a prerequisite for combat
and is tracked separately.

### Attack Tasks

Tasks provide the **strategic layer** — they tell the creature what goal
to pursue, and the creature uses combat actions to achieve it.

**`TaskKindTag::AttackTarget`** — pursue and kill a specific creature:

```rust
auto_pk_id!(TaskAttackTargetDataId);

#[derive(Table)]
pub struct TaskAttackTargetData {
    #[primary_key(auto_increment)]
    pub id: TaskAttackTargetDataId,
    #[indexed]
    pub task_id: TaskId,          // FK to tasks (cascade on delete)
    pub target: CreatureId,       // plain ID, not FK — target may die
}
```

(Same PK convention note as `TaskAttackMoveData` — see §2.)

Behavior: pathfind toward target. When in melee range, perform melee
actions. When in ranged-but-not-melee range with LOS and ammo, perform
shooting actions. On each activation, poll target's `vital_status` — if
`Dead` or row missing (future cleanup), task completes.

**Failed pathfinding:** If pathfinding fails (target unreachable), the
creature retries pathfinding on the next activation. After N consecutive
failures (configurable, e.g., `attack_path_retry_limit = 3`), the task
is cancelled — the target is deemed unreachable. The creature returns to
normal behavior (idle/wander). It may re-detect the target on a
subsequent activation and create a new AttackTarget task if the target
has moved to a reachable location.

**Immediate claiming:** Autonomous combat tasks (created by hostile
detection) are immediately claimed by the detecting creature in the same
activation — they are NOT left in `Available` state. The creature creates
the task and claims it atomically. This prevents a race where a different
idle creature could claim the task via `find_available_task` before the
detecting creature does.

**`TaskKindTag::AttackMove`** — move to a location, fighting en route
(defined in §2 above).

### Line of Sight (§5.1)

LOS is computed via **3D voxel ray march** (DDA) from the shooter's voxel
position to the target's voxel position through the `VoxelWorld` grid.

- A ray that hits any **solid voxel** (Trunk, Branch, Root, or any
  structure voxel) is blocked — no LOS.
- **Leaf voxels** do not block LOS (leaves are sparse canopy, not solid
  walls).
- The ray checks each voxel along the path in order. If all voxels between
  shooter and target are air or leaf, LOS exists.

**Multi-voxel targets:** For creatures with footprints larger than 1×1×1,
LOS is checked from the shooter to **any** occupied voxel of the target
(not just the anchor). If any ray succeeds, LOS exists. This means large
creatures are harder to hide behind cover, which is consistent with the
spatial index making them easier to hit.

**Diagonal corner leaks:** Standard DDA can "leak" through diagonal corners
of solid voxels (the ray passes through the mathematical corner between two
solids, hitting neither). This is acceptable for the initial pass — it may
occasionally allow shots through tight diagonal gaps in walls, but these
cases are rare and arguably reasonable (a narrow gap between two blocks could
plausibly allow an arrow through). Stricter traversal (e.g., 6-connected
stepping) is future work if needed.

LOS checks happen when a creature evaluates whether to shoot (on activation
while pursuing a target). Each archer checks LOS to its current target (not
all hostiles) on its activation — this is at most 1 ray per archer per
activation. With 10 archers at ~500-tick activation intervals, that's ~20
LOS checks/sec — negligible given each check traverses at most ~50 voxels.

### Aim Computation (§5.2)

Computing the initial velocity vector for a ballistic shot is non-trivial
under the integer-only determinism constraint (the analytic solution
involves square roots and trigonometry).

**Approach: iterative guess-and-simulate.** Rather than solving the
ballistic equation analytically:

1. **Initial guess:** Compute a rough aim direction from shooter to target
   (or to the target's predicted future position, for skilled/expert tiers).
   Estimate initial velocity magnitude from config `arrow_base_speed`.
   Apply a first-order gravity compensation: aim above the target by an
   amount proportional to `distance * gravity / (2 * speed²)`, computed
   in integer arithmetic (approximate — the iterative solver corrects
   any error).

2. **Simulate:** Run the exact same per-tick projectile update loop (§4)
   without creature collision, just tracking the trajectory. Check if the
   trajectory passes through the target's voxel (or within 1 voxel for
   tolerance).

3. **Adjust:** If the simulated trajectory misses, adjust the aim direction
   (raise/lower the arc, lead more/less) and re-simulate.

4. **Attempt cap:** Maximum `aim_max_iterations` (default 5) adjustment
   iterations. If no valid trajectory is found, the archer fires their best
   guess anyway (the arrow may miss — this is realistic and creates a
   natural difficulty curve for extreme range/angle shots).

**Performance:** Each simulation runs ~600 ticks for a 30-voxel shot
(50 voxels/sec ÷ 1000 ticks/sec × 30 voxels). At 5 iterations, that's
~3000 iterations of integer math per shot. With 10 archers firing every
3 seconds, this is ~10,000 integer ops/sec — negligible. But worth noting
that aim cost scales with archer count × shot rate × range.

This approach is fully deterministic (uses the same integer physics as real
projectiles), avoids floating-point math in the sim, and naturally handles
all edge cases (uphill, downhill, obstructed arcs).

### Aim Skill Tiers

Archer accuracy depends on skill level (future: trained via practice). Three
tiers of target-leading intelligence:

1. **Novice:** Aims at the target's current position. The guess-and-simulate
   loop targets the voxel where the target is right now.
2. **Skilled:** Leads based on the target's current velocity vector. The aim
   point is `target_pos + target_velocity * estimated_flight_time`. Good
   against straight-line movement, fooled by direction changes.
3. **Expert:** Reads the target's path plan — peeks at the next N upcoming
   nav nodes (capped, not the full path) and computes the intercept point
   where the arrow will arrive when the target does. The lookahead cap
   prevents expert archers from being omniscient while still making them
   meaningfully better than skilled archers.

The sim already stores creature paths (`CreaturePath`), so tier 3 is a
natural extension — the archer queries the target creature's planned path
and picks the optimal intercept node.

**Implication:** Erratic movement (frequent replanning, no clear path) is a
natural defense against expert archers.

### Archer Retreat Behavior

When a hostile melee attacker closes within a configurable
`archer_retreat_range_sq` (in voxels²), an archer with available movement
should attempt to **kite**: move away from the threat while maintaining
shooting range. Implementation: on the archer's combat heartbeat, if any
hostile is within retreat range, pathfind to a node that is (a) outside
retreat range from the threat and (b) within shooting range of the threat.
If no such node exists, the archer stands and fights.

This is not in the initial pass but should be added shortly after basic
shooting works, since archers that stand still while goblins run up to them
feel broken.

### Ammunition

Archers consume `Arrow` items from their inventory. When out of arrows:

- Stop shooting actions.
- If behavior is autonomous (not player-directed): attempt to acquire more
  arrows (existing `AcquireItem` task flow).
- If player-directed: emit a notification ("Archer out of arrows").

---

## 6. Hostile Creature AI

### Combat Behavior via SpeciesData

Creatures without a civilization (wildlife, unaffiliated hostiles) do not
have military groups. Their combat behavior is configured entirely through
`SpeciesData`:

```rust
// In SpeciesData (config)
pub combat_ai: CombatAI,
```

```rust
pub enum CombatAI {
    /// No combat behavior. Will not attack or flee. (Wildlife default.)
    Passive,
    /// Flee from hostiles within detection range. (Prey animals, future.)
    FleeOnly,
    /// Attack hostiles within detection range using melee.
    AggressiveMelee,
    /// Attack hostiles within detection range using ranged if possible,
    /// melee as fallback. (Future: ranged hostiles.)
    AggressiveRanged,
}
```

This replaces the `HostileResponse` for non-civ creatures. Civilization
members use their military group's `HostileResponse` instead; `combat_ai`
is ignored for civ creatures.

**Dual-system clarification:** `combat_ai` on `SpeciesData` serves as the
fallback behavior for creatures without a civ. When NPC civs gain creature
instances, those creatures use their military group's `hostile_response`
instead. If a civ is deleted (`civ_id` nullified via FK cascade, which
also nullifies `military_group`), the creature falls back to `combat_ai`.
This dual-system is intentional — species-level defaults for the
unaffiliated case, civ-level overrides for organized creatures. The
`combat_ai` value must therefore be set correctly for every species, even
those that are "always" in a civ, because civ deletion is a possible
(if rare) runtime event.

### Initial Behavior (Goblin, Orc, Troll)

Simple aggression AI. These species have `combat_ai: AggressiveMelee`:

1. On each activation, scan for hostile targets (elves, i.e., creatures
   whose civilization's faction is hostile) within detection range
   (`hostile_detection_range_sq`, integer, in voxels²).
2. If already engaged (has an AttackTarget task with a living target):
   continue pursuing. Skip the scan.
3. Filter to reachable targets (pathfinding check — or cheaper: same
   connected nav component, if tracked).
4. If targets found: select the closest by squared euclidean distance,
   then pathfind to it.
5. If already adjacent to a target: perform melee attack action.
6. If no targets found: wander (existing behavior).

**Distance calculation note:** Squared euclidean distance uses `i64`
intermediates: `let dx = pos_a.x as i64 - pos_b.x as i64;` then
`dx*dx + dy*dy + dz*dz`. Casting `i32` to `i64` before squaring prevents
overflow for any plausible world size.

**Two-phase proximity:**
- Phase 1: Squared euclidean distance filter (cheap, integer-only) to get
  candidates within detection range.
- Phase 2: Pathfind to the nearest few candidates (nav-graph distance is
  what actually matters — a goblin can't walk through a wall).

**Pathfinding budget:** Detection runs on activation (~500 ticks for most
species), but step 2 short-circuits when already engaged. For
disengaged creatures, the expensive part is pathfinding (step 3-4), not
the spatial scan. With 20 disengaged goblins at ~500-tick activations,
worst case is ~40 pathfinds/sec. If hostile counts scale up, consider a
per-tick pathfinding budget or caching recent paths.

**Nav graph connectivity:** The "same connected nav component" optimization
requires tracking connected components in the nav graph. This is O(V+E)
to compute on graph rebuild and would prevent wasted pathfinding attempts
across disconnected regions. Not required for the initial pass (just let
the pathfinder fail), but worth adding to `nav.rs` if hostile counts grow.

### Faction / Hostility

Hostility is **per-direction, not mutual.** A creature is hostile *toward*
another if its own civ considers the other creature's civ Hostile. This is
evaluated from the perspective of the creature doing the detection — civA
may consider civB hostile while civB considers civA neutral (e.g., civB
hasn't encountered civA's aggression yet). The check is:

- Both belong to civilizations: creature A is hostile toward creature B if
  A's civ has `CivOpinion::Hostile` toward B's civ. This is directional —
  B may or may not reciprocate.
- One is a civ creature and the other has no civ, and the non-civ
  creature's species has `combat_ai: AggressiveMelee` or
  `AggressiveRanged` (i.e., it's a hostile species by nature). The civ
  creature treats the aggressive non-civ creature as hostile; the non-civ
  creature treats civ creatures as hostile per its `combat_ai`, **except**
  creatures of the same species — a non-civ goblin does not attack goblin
  civ members. This same-species exemption prevents debug-spawned hostiles
  from attacking their own species' civilization when NPC civs gain
  creature instances.
- Both are non-civ creatures with aggressive `combat_ai` — they don't
  attack each other (hostiles of the same faction don't infight). This
  requires a lightweight "faction affinity" on the species level to
  distinguish goblin-allied vs. independent hostiles. For the initial
  pass, all aggressive non-civ creatures are considered allied (they
  don't attack each other).

Wildlife (`combat_ai: Passive`) is never hostile unless player-commanded.

**Auto-escalation:** When a civ creature attacks another civ's creature,
the victim's civ auto-escalates its opinion of the attacker's civ to
`CivOpinion::Hostile` if not already. This ensures civs react
appropriately to unprovoked aggression even if the diplomatic state hasn't
been manually set. The escalation is one-directional — the attacking civ's
opinion is unchanged (they were already aggressive). Auto-escalation only
applies when the attacker **has** a `civ_id` — non-civ attackers
(debug-spawned goblins, unaffiliated wildlife) do not trigger diplomatic
escalation because there is no civ to escalate against. This mechanism is
for future civ-vs-civ raids; unaffiliated hostiles are threats by species
nature, not diplomatic actors.

**Transition from species-level `Faction` enum:** The v3 design proposed a
simple `Faction` enum on `SpeciesData`. With civilizations, faction is
primarily determined by civ relationships, not species. The `CombatAI` enum
on `SpeciesData` replaces the species-level `Faction` for non-civ creatures.
For civ creatures, the civ's diplomatic relationships determine who is
hostile.

### Debug Spawning

Goblins, Orcs, and Trolls spawned via the debug spawner have no `civ_id`
and have `combat_ai: AggressiveMelee` in their `SpeciesData`, so they are
automatically hostile to the player's civ creatures. No additional setup
needed.

---

## 7. Detection and Engagement

### Hostile Detection

Hostile detection is checked **on every creature activation**, not on
heartbeat. This is consistent with the preemption system (§8), which also
runs on activation. Heartbeat-driven detection would create unacceptable
latency — with 3000-tick heartbeat intervals, a creature could be blind
to an adjacent hostile for up to 3 seconds.

Detection range is per-species configurable (`hostile_detection_range_sq`
in `SpeciesData`, integer, in voxels²).

**Height and detection range:** Squared euclidean distance includes the Y
component, so ground-level goblins with `hostile_detection_range_sq=225`
(15 voxels) cannot detect canopy elves at y=40 — the Y distance alone
(39² = 1521) far exceeds the detection range. This is intentional:
goblins must climb into the tree to threaten canopy elves, which creates
natural tactical gameplay — the tree itself is a defensive structure. If
this proves too limiting (e.g., goblins never find the elves and just
wander uselessly), a future option is separate horizontal/vertical range
weights or a detection trigger when goblins enter the nav graph's
trunk-climb edges.

For civ creatures, "hostile" is determined by checking the civ relationship
of nearby creatures (see §6). For non-civ aggressive creatures, all
civ creatures (and non-allied non-civ creatures) are potential targets.

**Performance note:** Detection on activation means every creature scans
for hostiles every ~500 ticks (elf walk speed). The scan iterates all
creatures in the spatial index and filters by squared euclidean distance
— this is O(n) where n is total alive creatures, not a spatial range
query. The `BTreeMap<VoxelCoord, Vec<CreatureId>>` spatial index is
ordered lexicographically by `(x, y, z)`, which does not support
efficient 3D bounding-box queries. Each scan must check all entries and
compute squared distance for each. With 30 elves and 20 goblins at
500-tick activations, this is ~100 scans/sec, each touching ~50 entries
— ~5,000 distance checks/sec. At expected creature counts (50-100), this
is still cheap in absolute terms. The O(n²) scaling per activation cycle
is real but manageable at these counts. If creature counts grow
significantly (200+), a spatial hash grid or octree would provide actual
range queries, but this is not needed for the initial pass.

When a creature detects a hostile:

- **Civ creatures with military group:**
  - If group `hostile_response` is Fight (or player-commanded): engage the
    hostile (pathfind toward, then use combat actions).
  - If group `hostile_response` is Flee: flee.
- **Non-civ creatures:** follow their `combat_ai` behavior (aggressive →
  attack, passive → ignore, flee-only → flee).

### Flee Behavior

Creatures with Flee response (civilian group default, or `FleeOnly`
combat AI):

1. Detect hostile within detection range.
2. Interrupt current task (preemption — see §8).
3. Pathfind away from the hostile using greedy retreat: at each decision
   point, pick the nav neighbor that maximizes squared euclidean distance
   from the threat. For multi-voxel threats (e.g., 2×2×2 trolls), distance
   is computed to the threat's **anchor voxel** (not the closest occupied
   voxel) — the anchor is sufficient because the distance difference is at
   most 1-2 voxels, and the greedy heuristic is approximate anyway. Ties
   are broken by `NavNodeId` for determinism. This is a cheap local
   heuristic, not a full A* search.

   **Future enhancement:** Add a secondary preference to flee toward
   friendly soldiers (military group with `HostileResponse::Fight`, same
   civ, alive, within some range). This would require defining "in the
   direction of" (e.g., positive dot product of flee-direction with
   soldier-direction) and a detection range for nearby soldiers. Deferred
   — the pure greedy retreat is sufficient for the initial pass.
4. Once the hostile is outside detection range, resume normal behavior.

**Flee persistence:** Flee does not require an explicit "fleeing" state
field on the creature. The detection check on every activation (§7) IS
the persistence mechanism. On each activation, if a hostile is within
detection range and the creature's response is Flee, the creature
performs a flee step (greedy retreat) instead of normal behavior. When
the hostile leaves detection range, the creature naturally resumes
normal behavior on its next activation. **Tradeoff:** the creature
stops fleeing the instant the threat leaves detection range, even if
the threat is still chasing from just outside range. This is acceptable
for the initial pass — the creature "loses sight" of the threat and
calms down. If this proves too exploitable (hostiles can herd
creatures by bobbing in and out of range), a future option is a
`flee_cooldown_ticks` that keeps the creature fleeing for N ticks
after last detection.

**Dead ends:** Greedy retreat can trap creatures in dead ends (e.g., the
end of a branch with no other exit). This is acceptable — it mirrors
real-life panic behavior and creates tactical situations where building
escape routes matters. A creature trapped in a dead end with a hostile
approaching will eventually have to fight or die. Future work could add
"cornered" behavior (desperate fighting with a damage bonus, or climbing
to escape if the species can climb).

**Tree topology note:** Tree-based maps are particularly trap-prone for
greedy retreat — the topology is mostly branches, which are mostly dead
ends. A creature fleeing along a branch has nowhere to go but back the way
it came. Greedy per-step retreat will happily walk to the tip of a branch
(maximizing distance at each step) only to be cornered. A future
improvement could use bounded A* away from the threat, maximizing distance
at the search horizon rather than greedily per-step. This would detect
dead ends within the search budget and prefer paths with throughput. Greedy
is acceptable for the initial pass — the "cornered" behavior covers the
failure case, and building bridges/escape routes is a natural player
response.

Future: panic/fear thoughts, morale breaks for soldiers.

---

## 8. Task Priority and Preemption

The current task system lacks explicit priority. Combat introduces the need
for priority-based preemption — without it, a civilian building a platform
won't interrupt to flee from a goblin.

### Preemption Levels

A new `PreemptionLevel` enum (distinct from the existing `Priority` enum
used for build project ordering — those are separate concepts).

**Ordering convention:** Higher numeric value = higher priority. The enum
does NOT derive `Ord` — comparisons use an explicit `fn level(&self) -> u8`
method to avoid subtle bugs from variant ordering:

```rust
pub enum PreemptionLevel {
    Idle,             // 0 — wander, do nothing
    Autonomous,       // 1 — haul, cook, craft, harvest
    PlayerDirected,   // 2 — player-directed non-combat (GoTo, Build)
    Survival,         // 3 — eat, sleep
    Mood,             // 4 — mope
    AutonomousCombat, // 5 — group behavior = Fight
    PlayerCombat,     // 6 — player-directed attack/attack-move
    Flee,             // 7 — emergency escape
}

impl PreemptionLevel {
    pub fn level(&self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Autonomous => 1,
            Self::PlayerDirected => 2,
            Self::Survival => 3,
            Self::Mood => 4,
            Self::AutonomousCombat => 5,
            Self::PlayerCombat => 6,
            Self::Flee => 7,
        }
    }
}
```

**Mapping from `(TaskKindTag, TaskOrigin)` to `PreemptionLevel`:**

Preemption level is computed, not stored. A function
`fn preemption_level(kind: TaskKindTag, origin: TaskOrigin) -> PreemptionLevel`
resolves the level:

| TaskKindTag    | Origin           | PreemptionLevel   |
|----------------|------------------|-------------------|
| (no task)      | —                | Idle (0)          |
| Haul           | Autonomous       | Autonomous (1)    |
| Haul           | Automated        | Autonomous (1)    |
| Cook           | Autonomous       | Autonomous (1)    |
| Cook           | Automated        | Autonomous (1)    |
| Craft          | Autonomous       | Autonomous (1)    |
| Craft          | Automated        | Autonomous (1)    |
| Harvest        | Autonomous       | Autonomous (1)    |
| Harvest        | Automated        | Autonomous (1)    |
| GoTo           | PlayerDirected   | PlayerDirected (2)|
| Build          | PlayerDirected   | PlayerDirected (2)|
| Furnish        | PlayerDirected   | PlayerDirected (2)|
| EatBread       | Autonomous       | Survival (3)      |
| EatFruit       | Autonomous       | Survival (3)      |
| Sleep          | Autonomous       | Survival (3)      |
| AcquireItem    | Autonomous       | Survival (3)      |
| Mope           | Autonomous       | Mood (4)          |
| AttackTarget   | Autonomous       | AutonomousCombat (5) |
| AttackMove     | Autonomous       | AutonomousCombat (5) |
| AttackTarget   | PlayerDirected   | PlayerCombat (6)  |
| AttackMove     | PlayerDirected   | PlayerCombat (6)  |
| (flee action)  | —                | Flee (7)          |

`TaskOrigin::Automated` (used by logistics heartbeat, kitchen monitor,
workshop monitor for Haul/Cook/Craft/Harvest tasks) maps to the same
preemption levels as `Autonomous`. The two origins are semantically
different (Automated = created by a management system, Autonomous =
created by the creature's own heartbeat), but they have identical
preemption priority — both represent background work that should yield
to higher-priority needs.

`AcquireItem` is Survival (3), not Autonomous (1), because it represents
personal self-care (acquiring food, equipment the creature needs). This
matches its behavioral role alongside eating and sleeping. **Note:** For
the initial pass, all AcquireItem tasks are Survival(3) regardless of
item type. Future refinement: distinguish food acquisition (Survival)
from equipment acquisition (Autonomous). This requires checking the
want's `item_kind` in the preemption function, which adds complexity.
Defer to when military equipment wants are implemented.

**Mapping function structure:** The `preemption_level()` function uses
`TaskKindTag` as the primary discriminator. `TaskOrigin` only matters for
combat tasks (`AttackTarget`, `AttackMove`) — all other task kinds map to
a fixed level regardless of origin (with `Autonomous` and `Automated`
treated identically). Combinations not listed in the table (e.g.,
`PlayerDirected` `EatBread`) do not currently occur in the codebase. If
new combinations arise, they should be added to the table explicitly.
**The function should use an exhaustive match (no wildcard) so that new
`TaskKindTag` or `TaskOrigin` variants cause compile errors**, forcing
the developer to assign the correct preemption level.

Flee is not a task — it's an action-level behavior (see §7). It gets the
highest preemption level because it must interrupt anything.

**Note on mope:** Mood is above Survival in the numeric ordering. This
means a moping elf cannot be interrupted by hunger or tiredness — they
will finish moping before eating. This is intentional: moping represents
a mood crisis that overrides routine needs. However, moping IS
interrupted by combat (Flee, PlayerCombat, AutonomousCombat all outrank
it). The existing `mope_can_interrupt_task` config field is superseded by
this system — moping always preempts Autonomous and PlayerDirected tasks,
and is always preempted by combat and above. The config field should be
deprecated.

**Hardcoded exception — Mood never preempts Survival.** Despite
Mood(4) > Survival(3) in the numeric ordering, the preemption check has
an explicit exception: mope cannot interrupt eating, sleeping, or item
acquisition tasks. The existing code (`sim.rs`) explicitly prevents mope
from interrupting autonomous self-care tasks to avoid a death spiral — a
chronically unhappy elf could otherwise mope-interrupt eating, become
hungrier, get more unhappy, mope more, and starve. The preemption system
preserves this protection. The preemption check is "can preempt if
current level < new level", with the additional rule: **Mood never
preempts Survival**. This is a hardcoded exception in the preemption
function, not a level ordering change — Mood(4) remains above Survival(3)
so that Survival cannot preempt Mood either (a moping elf finishes moping
before eating, but cannot START moping over eating).

**Note on mope frequency:** The existing mope system rolls a probability
check on heartbeat (~3000-tick intervals), with config values
(`mope_mean_ticks_*`) calibrated for that frequency. The preemption system
checks on activation (~500-tick intervals), but this does NOT change mope
frequency — the preemption check only asks "should the current task be
interrupted by a higher-priority need?" The mope probability roll itself
stays on heartbeat. The preemption system and the mope initiation system
are separate concerns: heartbeat decides whether to START moping,
activation-time preemption decides whether moping can INTERRUPT the
current task.

### Preemption Mechanics

On each creature activation, the creature checks whether a higher-priority
action is available (hostile detected, player command issued). If so:

- The current task is **unassigned** from the creature — the creature's
  `current_task` is set to `None`, and the task's state is set back to
  `Available` if it's resumable (Build, Furnish) or cancelled if not.
  Item reservations held by the task are cleared. The creature may need
  to re-navigate to the task location if it later reclaims it (or a
  different creature may claim it).
- The higher-priority task is created/claimed and executed.
- When the preempting task completes (enemy killed, fled to safety), the
  creature goes through normal task selection — it may reclaim its old
  task if it's still Available, or pick something else.

This avoids the need for a task stack or `Suspended` state. The cost is
that a creature that was 80% done building something might lose that task
to another creature after fleeing — but this is acceptable and realistic
(another elf finishes the job while you were busy fleeing).

### PlayerDirected Override of AutonomousCombat

The standard preemption rule is "can preempt if new level > current
level." There is one special-case override: **any PlayerDirected command
(GoTo, Build, Furnish, etc.) can preempt AutonomousCombat**, even though
PlayerDirected(2) < AutonomousCombat(5) in the numeric ordering. The
rationale is that the player is the ultimate authority — if they tell a
soldier to stop fighting and go build a platform, the soldier obeys.

This override applies **only** to AutonomousCombat, not to PlayerCombat.
PlayerCombat(6) means the player explicitly told the creature to attack —
another PlayerDirected command at a different level does not override it.
A new PlayerDirected command replaces PlayerCombat through normal
same-level replacement (the player issued a new order, overriding the
previous one).

**Same-level replacement defined:** When a new task has the same
preemption level as the creature's current task, the new task does NOT
preempt — the creature finishes what it's doing. The exceptions are
explicit player commands: a new PlayerDirected command replaces an
existing PlayerDirected task (the player changed their mind), and a new
PlayerCombat command replaces an existing PlayerCombat task (retargeting).
This is normal task reassignment, not preemption — the old task is
abandoned because the player issued a superseding order of the same kind.

Implementation: in the preemption check function, after the standard
level comparison, add: if the current level is `AutonomousCombat` and the
new task's origin is `PlayerDirected`, allow preemption regardless of the
new task's numeric level.

### Implementation Note

Task preemption is **foundational** to combat — it must be implemented
alongside melee combat (Phase B), not deferred to polish. Without
preemption, neither flee nor autonomous engagement works.

---

## 9. Sim Events for Combat

New `SimEventKind` variants for combat, consumed by the Godot side for
rendering effects, sounds, and notifications:

```rust
// Existing SimEventKind extended with:
CreatureDied { creature_id: CreatureId, species: Species, position: VoxelCoord, cause: DeathCause },
CreatureDamaged { creature_id: CreatureId, damage: i64, source: DamageSource },
ProjectileLaunched { projectile_id: ProjectileId, shooter_id: CreatureId },
ProjectileImpact { projectile_id: ProjectileId, position: VoxelCoord, impact: ImpactKind },
CombatEngaged { attacker: CreatureId, defender: CreatureId },

pub enum DeathCause { Arrow { shooter: CreatureId }, Melee { attacker: CreatureId } }
pub enum DamageSource { Arrow { shooter: CreatureId }, Melee { attacker: CreatureId } }
pub enum ImpactKind { Creature(CreatureId), Surface, OutOfBounds }
```

---

## 10. Config Fields

New fields needed in `GameConfig` / `SpeciesData`:

### SpeciesData additions

```rust
pub hp_max: i64,                        // max hit points
pub melee_damage: i64,                  // damage per melee hit
pub melee_interval_ticks: u64,          // ticks between melee attacks
pub melee_range_sq: i64,               // squared euclidean distance for melee reach
pub hostile_detection_range_sq: i64,    // squared euclidean voxels for detection
pub archer_range_sq: i64,              // squared max shooting range
pub archer_retreat_range_sq: i64,      // squared distance to trigger kiting (future)
pub combat_ai: CombatAI,              // non-civ combat behavior
```

All range fields are squared integers — comparisons use
`dx*dx + dy*dy + dz*dz <= range_sq` with `i64` intermediates (cast `i32`
coords to `i64` before squaring), avoiding both square roots and overflow.

### GameConfig additions

```rust
pub arrow_base_speed: i64,             // sub-voxel units per tick at launch
pub arrow_gravity: i64,                // sub-voxel units per tick² (downward)
pub arrow_base_damage: i64,            // damage at reference speed
pub arrow_durability_max: i32,         // starting durability
pub arrow_durability_loss_surface: (i32, i32),  // (min, max) loss on surface hit
pub arrow_durability_loss_creature: (i32, i32), // (min, max) loss on creature hit
pub shoot_cooldown_ticks: u64,         // ticks between shots (global; per-species if needed later)
pub aim_max_iterations: u32,           // max aim adjustment attempts
```

**Note on `shoot_cooldown_ticks` placement:** Melee cooldown is per-species
(in `SpeciesData`) because different species fight at different speeds. Ranged
cooldown is global (in `GameConfig`) because initially only elves can shoot.
When ranged hostiles are added (`AggressiveRanged`), this should move to
`SpeciesData` for consistency. This is a known simplification.

### Suggested Default Values

These are starting points for tuning, not final balance:

| Field | Value | Rationale |
|-------|-------|-----------|
| `arrow_base_speed` | 53,687,091 | ~50 voxels/sec (50/1000 × 2^30) |
| `arrow_gravity` | 10,737 | ~10 voxels/sec² (10/1000² × 2^30) |
| `arrow_base_damage` | 20 | ~1/5 of a goblin's HP |
| `arrow_durability_max` | 3 | arrows survive a few impacts |
| `arrow_durability_loss_surface` | (0, 1) | surface hits rarely break |
| `arrow_durability_loss_creature` | (1, 2) | creature hits usually damage |
| `shoot_cooldown_ticks` | 3000 | 3 seconds between shots |
| `aim_max_iterations` | 5 | reasonable compute budget |
| `hostile_detection_range_sq` (elf) | 400 | 20-voxel radius |
| `hostile_detection_range_sq` (goblin) | 225 | 15-voxel radius |
| `melee_interval_ticks` (goblin) | 1500 | 1.5 sec between strikes |
| `melee_interval_ticks` (troll) | 3000 | 3 sec (slow but heavy) |
| `melee_range_sq` | 2 | 1-voxel reach, covers face-adjacent + 2D diagonal |
| `melee_damage` (goblin) | 15 | |
| `melee_damage` (orc) | 25 | |
| `melee_damage` (troll) | 50 | |

---

## 11. Implementation Phases

### Phase A: Foundation + Military Groups

- Add `hp` / `hp_max` / `vital_status` (with `#[indexed]`) to `Creature`.
- Add `hp_max` to `SpeciesData`, configure for all 10 species.
- Add `CombatAI` enum and `combat_ai` to `SpeciesData`.
- Death: HP ≤ 0 → transition to `Dead`, task cleanup (per-kind with
  reservation clearing), drop inventory, clear home assignment, emit event.
- `MilitaryGroup` data model and SimDb table with `civ_id` FK. Create two
  default groups per civilization during worldgen (after civ creation,
  before creature spawns).
- `military_group: Option<MilitaryGroupId>` field on `Creature` (nullable
  FK, nullify on group delete). Civ creatures get their civ's Civilians
  group; non-civ creatures get `None`.
- Save compatibility: `#[serde(default)]` on all new fields.
- Creature death rendering (Godot side: creature disappears).

### Phase B: Melee Combat + Task Preemption

Split into sub-phases:

**B1 — Task Preemption:**
- `PreemptionLevel` enum with explicit `level()` method (no derived Ord).
- Preemption check on every creature activation: unassign current task
  (with reservation cleanup) if higher-priority action available.
- Testable without combat: verify mope still preempts idle, player GoTo
  preempts autonomous hauling.

**B2 — Melee Combat + Hostile AI:**
- `SimAction` variants: `AttackCreature`, `AttackMove`.
- `AttackTarget` task kind with `TaskAttackTargetData` extension table
  (target creature ID as plain field).
- Melee attack as a creature action (not a task): adjacency check,
  cooldown, damage application.
- Hostile AI: activation-driven "attack closest reachable elf" (squared
  euclidean filter with `i64` intermediates → pathfind).
- Death triggering from melee damage.
- Sim events: `CreatureDied`, `CreatureDamaged`, `CombatEngaged`.

**B3 — Flee:**
- Flee behavior: civilians detect hostiles, preempt current task, greedy
  retreat with `NavNodeId` tie-breaking.
- Testable: spawn goblins near elves, watch soldiers fight and civilians
  flee.

### Phase C: Projectile System

Split into sub-phases:

**C1 — Projectile Core:**
- `SubVoxelCoord` type with 2^30 precision.
- `Projectile` entity and SimDb table (no FK constraints, serialized
  normally for save/load).
- Creature spatial index (`BTreeMap<VoxelCoord, Vec<CreatureId>>`), stored
  on `SimState` as `#[serde(skip)]`, rebuilt on load. Centralized
  `update_creature_position()` helper, maintenance at all position mutation
  points.
- `ProjectileTick` batched event integration.
- Per-tick update loop: gravity (symplectic Euler), position advancement,
  `prev_voxel` tracking (initialized to shooter position at spawn).
- Voxel collision (solid voxels → surface impact at `prev_voxel`).
- Bounds check (out of world → despawn).
- Arrow impact → ground pile or destruction.

**C2 — Creature Hits:**
- Creature collision via spatial index (resolve on first contact, no
  multi-hit).
- Candidates sorted by `CreatureId` (UUID byte order), PRNG selection.
- Speed-dependent damage calculation (integer, i128, min 1 damage).
- Sim events: `ProjectileLaunched`, `ProjectileImpact`.

**C3 — Shooter Integration:**
- Ranged attack as a creature action (not a task): LOS check, range check,
  cooldown, ammo consumption, projectile spawn.
- LOS via voxel ray march (DDA, checking any occupied voxel for multi-voxel
  targets, accepting diagonal corner leaks for now).
- Aim computation: iterative guess-and-simulate (novice tier only in first
  pass).

**C4 — Rendering:**
- `projectile_renderer.gd` (pool pattern, oriented 3D meshes).
- `SimBridge.get_projectile_positions()` (stride-6 PackedFloat32Array,
  velocity in voxels-per-tick float units).
- Interpolation via `position + velocity * fractional_offset`.

### Phase D: RTS Selection and Commands

Godot-side UI work, plus sim-side command routing:

- Box selection (click-drag rectangle) in `selection_controller.gd`.
- Multi-creature selection state and group info panel.
- Right-click context commands: translate to `SimAction::AttackCreature`
  or existing GoTo commands.
- Attack-move hotkey: translate to `SimAction::AttackMove`.
- `AttackMove` task kind with `TaskAttackMoveData` extension table
  (destination + optional current target, polled on activation).

### Phase E: Military Group UI + Polish

- Group configuration UI (armor, weapon, food, behavior, hostile response).
- Group behavior influencing task selection.
- Skill tiers for archery (skilled, expert leading).
- Archer retreat/kiting behavior.
- Combat-related thoughts: new `ThoughtKind` variants (`AllyDied`,
  `KilledEnemy`, `WonBattle`, `FledFromHostile`), corresponding mood
  weights and dedup/expiry config.
- Notifications for combat events.
- Balance tuning.

---

## Open Questions

- **Training:** What does "Train" behavior look like concretely? Sparring
  with another soldier? Target practice (shooting at a target structure)?
  What stats improve?
- **Armor:** When armor is added, is it item-based (equip a leather
  chestplate) or species-inherent (trolls have natural armor)?
- **Ranged hostiles:** When should goblins/orcs get ranged capability?
  Different species, or upgrades?
- **Siege behavior:** Do hostile groups coordinate (focus fire, flank), or is
  each creature independent? Coordination requires a group AI layer.
- **Friendly fire:** Can arrows hit friendly creatures? Realistic but
  potentially frustrating. Could be a skill-gated risk (novice archers might
  friendly-fire).
- **Creature size and hit probability:** Should large creatures (trolls,
  elephants) be easier to hit beyond just occupying more voxels? Or is the
  multi-voxel footprint sufficient?
- **Wind:** If added, wind must be deterministic sim state (PRNG-driven
  weather system), not random per-tick noise. Design deferred.
- **Hostile civ raids:** When NPC civs gain creature instances (raiders,
  traders), their combat behavior should come from their civ's military
  groups, not `CombatAI`. The `CombatAI` field is for truly unaffiliated
  creatures only.
