# Group Activities: Multi-Worker Coordination Layer

**Status:** Draft design
**Tracker items:** F-group-activity, F-choir-build, F-choir-harmony, F-combat-singing, F-group-dance, F-group-chat
**Date:** 2026-03-23

## Motivation

The current task system is fundamentally single-worker: one creature claims one task, walks to the location, does work, completes it. This serves well for hauling, eating, building (solo), crafting, and combat — but it cannot express activities that require multiple participants coordinating together.

Examples of group activities:
- **Construction choir:** 4 elves assemble at a build site and sing together. Construction doesn't start until all singers are in place, and progress depends on how many are actively singing.
- **Dance:** An elf organizes a social dance needing 4–8 participants. They must all arrive at the dance clearing before the dance begins.
- **Heavy hauling:** Moving a large object requires 2+ elves carrying it simultaneously (not individual haul tasks).
- **Combat singing (far future):** A band of singers buffs nearby fighters during battle.
- **Ritual/ceremony:** Seasonal or milestone events with required participant counts.

The key properties that distinguish group activities from tasks:
1. **Multiple participants** with potentially different roles.
2. **Assembly phase** — the activity doesn't start until enough participants have arrived at their positions.
3. **Committed execution** — once the group phase begins, leaving has consequences (social penalty, disrupted harmony, etc.).
4. **Group-level progress** — progress is a function of the group working together, not individual work units.
5. **Coordination lifecycle** — recruitment → assembly → execution → completion is more complex than a task's available → in-progress → complete.

## Design Principle: Activities Own Tasks, Not Replace Them

An `Activity` is a coordination layer *above* the task system. It does not replace tasks — it creates and manages them. Each participant's individual movement (walk to the assembly point) is still a regular task. The group work phase may also use tasks, or may bypass them for direct activation-level behavior.

This means:
- The existing task system, preemption, pathfinding, and activation loop are untouched for the assembly phase.
- The `find_available_task` pipeline doesn't need to understand group activities — it only sees normal tasks.
- Group-specific logic (assembly gating, group progress, kind-specific behavior) lives in a new module.

## Schema

### New ID Type

```rust
// types.rs
entity_id!(/// Unique identifier for a group activity.
ActivityId);
```

Uses the `entity_id!` macro, which creates a newtype around `SimUuid` —
the same pattern as `CreatureId`, `TaskId`, and `ProjectId`. Generated
deterministically from the sim PRNG.

### Activity Table

```rust
// db.rs
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Activity {
    #[primary_key]
    pub id: ActivityId,

    /// What kind of group activity this is.
    #[indexed]
    pub kind: ActivityKind,

    /// Lifecycle phase.
    #[indexed]
    pub phase: ActivityPhase,

    /// Where the group activity takes place (center point for spatial queries).
    pub location: VoxelCoord,

    /// Minimum participants needed before the activity can begin.
    /// The activity stays in `Assembling` until at least this many
    /// participants have arrived and are ready. If `None`, the activity
    /// can start with any number of participants (even 1).
    pub min_count: Option<u16>,

    /// Desired number of participants. Recruitment continues up to this
    /// count, but the activity can begin once `min_count` is reached.
    /// Affects effectiveness — a choir at desired count has better harmony
    /// than one at minimum. If `None`, same as `min_count`.
    pub desired_count: Option<u16>,

    /// Current work progress (only advances during `Executing` phase).
    pub progress: i64,

    /// Total work needed to complete. 0 for instant-on-assembly activities.
    pub total_cost: i64,

    /// Origin — player-directed (manual choir assignment) or automated
    /// (construction system auto-assembling a choir for a queued build).
    pub origin: TaskOrigin,

    /// How participants join this activity.
    pub recruitment: RecruitmentMode,

    /// What happens when a participant leaves during execution.
    pub departure_policy: DeparturePolicy,

    /// Whether new participants can join after execution has started.
    /// Dances allow late joiners; construction choirs do not.
    /// Default per ActivityKind from config.
    pub allows_late_join: bool,

    /// Tick when the activity entered the Executing phase. Used for
    /// duration tracking, harmony warmup, etc.
    pub execution_start_tick: Option<u64>,

    /// Tick when the activity entered the Paused phase (PauseAndWait
    /// departure policy only). Compared against `timeout_ticks` to
    /// determine when to cancel. Cleared when resuming to Executing.
    pub pause_started_tick: Option<u64>,
}
```

### ActivityKind Enum

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActivityKind {
    /// Construction choir — sing to grow a structure.
    ConstructionChoir,
    /// Social dance — recreational, generates happiness.
    Dance,
    /// Combat singing — buff nearby allies.
    CombatSinging,
    /// Heavy hauling — move a large object together.
    GroupHaul,
    /// Ceremony/ritual — seasonal or milestone event.
    Ceremony,
}
```

Starts small; new variants are cheap to add. Extension tables (like the task system's `TaskBlueprintRef`, `TaskHaulData`, etc.) carry kind-specific data.

### RecruitmentMode Enum

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RecruitmentMode {
    /// Push: the activity (or player) selects and assigns specific creatures.
    /// Used for player-directed activities (player hand-picks a choir) and
    /// system-initiated activities (construction queue auto-selects by
    /// proximity/skill).
    Directed,
    /// Pull: the activity advertises open slots. Idle creatures discover it
    /// during their activation loop (similar to find_available_task) and
    /// volunteer to join. Used for autonomous social activities — an elf
    /// organizes a dance and nearby interested elves opt in.
    Open,
}
```

Both modes can coexist: a player could start a directed choir (hand-picking 3 singers) and then set remaining slots to open (let 2 more elves volunteer). The recruitment mode is per-activity, not per-slot, but a future extension could allow per-slot overrides.

### DeparturePolicy Enum

Different activities respond differently when participants leave mid-execution.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DeparturePolicy {
    /// Resilient: remaining participants keep going. The activity continues
    /// at reduced effectiveness. Good for social activities like dances —
    /// one person leaving doesn't stop the party.
    Continue,
    /// Fragile: the activity pauses when a participant leaves, waiting for
    /// them to return or a replacement to arrive. If the wait exceeds a
    /// configurable timeout, the activity is cancelled. Good for
    /// construction choirs where the song requires all voices.
    PauseAndWait {
        /// How long to wait (in sim ticks) before cancelling.
        timeout_ticks: u64,
    },
    /// Strict: the activity is cancelled immediately if any participant
    /// leaves. For rituals or ceremonies that require unbroken participation.
    CancelOnDeparture,
}
```

The departure policy is stored per activity instance (on the `Activity` row), but defaults are determined by `ActivityKind` — each kind has a sensible default policy in config (e.g., `ConstructionChoir → PauseAndWait`, `Dance → Continue`, `Ceremony → CancelOnDeparture`). The player can override the default when creating a player-directed activity.

### ActivityPhase Enum

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActivityPhase {
    /// Recruiting participants. The activity exists but hasn't started
    /// assembling yet — participants are being selected/invited.
    Recruiting,
    /// Participants have been assigned and are walking to their positions.
    /// Transitions to Executing when enough have arrived.
    Assembling,
    /// The group activity is in progress. Progress advances each tick
    /// based on participant count and quality factors.
    Executing,
    /// Activity finished successfully.
    Complete,
    /// Execution paused because a participant left and the departure policy
    /// is `PauseAndWait`. Remaining participants stay in place. Resumes if
    /// the gap is filled within the timeout; transitions to Cancelled if not.
    Paused,
    /// Activity was cancelled (not enough participants, player cancelled,
    /// too many left mid-execution, etc.).
    Cancelled,
}
```

### Activity Participant Table

```rust
/// Links a creature to an activity with role and status information.
/// A creature can only participate in one activity at a time (enforced
/// by the `current_activity` FK on Creature, not by this table).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("activity_id", "creature_id")]
pub struct ActivityParticipant {
    #[indexed]
    pub activity_id: ActivityId,
    #[indexed]
    pub creature_id: CreatureId,

    /// This participant's role within the activity.
    pub role: ParticipantRole,

    /// Whether this participant has arrived at their assigned position
    /// and is ready for the group phase to begin.
    pub arrived: bool,

    /// The position this participant needs to reach for the assembly
    /// phase. Each participant may have a different position (e.g.,
    /// choir members standing in a semicircle around the build site).
    pub assigned_position: VoxelCoord,

    /// The task driving this participant's movement to the activity.
    /// Created during the Assembling phase, cleared when the creature
    /// arrives or the activity transitions to Executing. This lets
    /// the activity track and manage the GoTo tasks it spawns.
    pub travel_task: Option<TaskId>,
}
```

### ParticipantRole Enum

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ParticipantRole {
    /// Organizer/lead — initiated the activity. If the organizer leaves,
    /// the activity may be cancelled or a new organizer chosen.
    Organizer,
    /// Regular participant.
    Member,
    /// Specific roles for typed activities (future expansion):
    /// soprano/alto/tenor/bass for choirs, lead dancer, etc.
    // ChoirSoprano, ChoirAlto, ChoirTenor, ChoirBass,
}
```

Starting with just Organizer/Member. Voice-type roles for choirs can be added as variants later without schema migration (new variants deserialize naturally; old saves with only Organizer/Member load fine since existing data doesn't contain the new variants).

### Creature Table Addition

```rust
// On the Creature row:
pub current_activity: Option<ActivityId>,
```

This parallels `current_task`. A creature can have *either* a `current_task` or a `current_activity`, but not both simultaneously. (During the assembly phase, the creature has both: `current_activity` points at the activity, and `current_task` points at the GoTo task the activity created. When the GoTo completes and the group phase begins, `current_task` is cleared.)

Wait — actually, the assembly-phase GoTo task and the activity coexist. Let's be precise about this:

- **Assembly phase:** `current_activity = Some(activity_id)`, `current_task = Some(goto_task_id)`. The creature is walking to its assigned position via a normal GoTo task. The task system drives movement as usual.
- **Executing phase:** `current_activity = Some(activity_id)`, `current_task = None`. The creature's behavior is driven by the activity system, not the task system. The activation loop checks `current_activity` before `find_available_task`.
- **No activity:** `current_activity = None`. Normal task/wander behavior.

### Extension Tables (Kind-Specific Data)

Following the task system's pattern of 1:1 extension tables keyed by the parent ID, each `ActivityKind` that needs extra state gets its own extension table. For example, a construction choir would need a `ProjectId` FK to the blueprint; a dance might need a dance-type enum. The specifics of these tables are deferred to the feature designs that use them (F-choir-build, F-group-dance, etc.) — the group activity framework just provides the hook point.

The pattern is:

```rust
/// Example kind-specific extension table (1:1 with Activity).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct ActivityFooData {
    #[primary_key]
    pub activity_id: ActivityId,
    // ... kind-specific fields ...
}
```

### SimDb Additions

```rust
// In SimDb:
#[table(singular = "activity")]
pub activities: ActivityTable,

#[table(singular = "activity_participant",
        fks(activity_id = "activities" on_delete cascade,
            creature_id = "creatures" on_delete cascade))]
pub activity_participants: ActivityParticipantTable,

// Extension tables added per-kind as features are implemented:
// #[table(singular = "activity_choir_data",
//         fks(activity_id = "activities" pk on_delete cascade))]
// pub activity_choir_data: ActivityChoirDataTable,
```

The `Creature` table's FK declaration gains a new nullable FK:

```rust
#[table(singular = "creature",
        fks(current_task? = "tasks",
            current_activity? = "activities" on_delete nullify,
            assigned_home? = "structures",
            ...))]
pub creatures: CreatureTable,
```

`on_delete nullify` ensures that if an activity row is deleted (cleanup
after completion/cancellation), the creature's `current_activity` is
automatically cleared rather than causing a FK violation.

## Lifecycle

### 1. Creation and Recruitment (`Recruiting`)

An activity is created by either:
- **Player action:** Player designates a construction project and the system creates a ConstructionChoir activity, or player explicitly organizes a dance.
- **Autonomous decision:** An elf with high social stats decides to organize a dance during free time.
- **System trigger:** The construction queue picks the next project and creates a choir activity.

**Directed recruitment:** The system (or player) selects participants based on availability, proximity, suitability (species, skills, voice type for choirs, social relationships). Used for player-directed choirs and system-initiated construction. Mechanically, directed recruitment works via `SimCommand::AssignToActivity { activity_id, creature_id }` — the player (or construction queue) issues one command per participant. The command validates eligibility, creates an `ActivityParticipant` row, and sets `creature.current_activity`. When participant count reaches the threshold for transitioning to `Assembling`, the activity advances phase and GoTo tasks are created for all assigned participants.

**Open recruitment:** The activity advertises open slots. During the idle creature activation loop (after `find_available_task` finds nothing), the creature checks for open activities accepting participants. Selection criteria (proximity, interest, social compatibility) filter which activities a creature will volunteer for. Used for autonomous social activities — an elf organizes a dance, nearby elves opt in.

The activity stays in `Recruiting` until at least `min_count` participants have been assigned (directed) or have volunteered (open). If `min_count` is `None`, the activity can transition to `Assembling` immediately with any number of participants. Recruitment for open-slot activities can continue during the `Assembling` phase up to `desired_count` — latecomers get GoTo tasks and join the group if they arrive before execution begins (or even during, for activities with `Continue` departure policy).

### 2. Assembly (`Assembling`)

Once enough participants are recruited, the activity transitions to `Assembling`. For each participant:
1. A GoTo task is created targeting their `assigned_position`.
2. `creature.current_task` is set to this GoTo task.
3. `creature.current_activity` is set to the activity.
4. The participant's `travel_task` field tracks the GoTo task ID.

As each creature arrives at its position (GoTo completes), the participant's `arrived` flag is set to `true` and `travel_task` is cleared. The creature enters an idle-at-position state, waiting for the group.

**Arrival check:** Each time a participant arrives, the activity checks whether enough participants have arrived (`arrived` count >= `min_count`). If so, transition to `Executing`. If `min_count` is `None`, execution can begin as soon as any participant arrives (though the activity may wait briefly for more to arrive for better effectiveness).

**Timeout/patience:** If assembly takes too long (configurable per activity kind), the activity can be cancelled. An activity with `min_count = 4` and `desired_count = 6` will start when 4 arrive rather than waiting indefinitely for 6. This prevents one slow elf from blocking everyone.

**Flying creatures:** The assembly model assumes ground-based creatures walking to nav-graph positions. Flying creatures (giant hornets, wyverns) use 3D voxel pathfinding and don't have nav nodes. For now, flying creatures are excluded from group activities — they are hostile fauna, not civilization members. If winged elves (F-winged-elf) are added, the assembly system will need to support flight-based GoTo tasks with voxel-coordinate destinations instead of nav-node destinations. The `assigned_position` field on `ActivityParticipant` is already a `VoxelCoord`, so the schema supports this; only the GoTo task creation logic needs a flight-aware path.

**Preemption during assembly:** A creature walking to an activity can still be preempted by Survival or Flee-level needs (they're hungry, under attack). If preempted:
- The GoTo task is abandoned normally.
- The participant's `arrived` stays `false`.
- The activity waits. If the creature returns and is still assigned, a new GoTo task is created.
- If the creature is removed from the activity (died, reassigned), the activity may need to recruit a replacement or proceed with fewer.

### 3. Execution (`Executing`)

The group activity is running. Key differences from task execution:

**Activation behavior:** When a creature with `current_activity` and no `current_task` is activated:
1. The activation loop sees `current_activity` is set.
2. Instead of `find_available_task`, it calls `execute_activity_behavior(creature_id, activity_id)`.
3. The activity-specific logic runs (singing, dancing, etc.).
4. Progress is contributed — but progress is on the *activity*, not on any task.

**Progress model:** Each activation tick, each participating creature contributes to the activity's progress. The contribution rate is kind-specific — some activities scale with participant count and quality factors (e.g., a group bonus that incentivizes coordination overhead), while others simply track duration (dance for N ticks, then complete). The specifics of each kind's progress formula are deferred to the feature designs that use them.

**Determinism:** Progress contributions are applied during individual creature activations, which fire in deterministic order (creature activation events are ordered by tick, then by `CreatureId` which is `SimUuid`-based with `Ord`). When multiple creatures contribute in the same tick, the order is consistent across all clients. The `ActivityParticipant` table is BTreeMap-backed (like all tabulosity tables), so iterating participants for any group-level calculation is deterministic.

**Committed action:** During execution, participants should not be preempted by low-priority tasks. The activity registers at a configurable preemption level (e.g., `PlayerDirected` for player-initiated choirs, `Autonomous` for spontaneous dances). This is checked via a new arm in `preemption_level()` that handles `current_activity`.

However, Survival and Flee *can* still interrupt. An elf who is starving will leave the choir to eat. This has consequences:
- The participant is removed from the activity (or marked as temporarily absent).
- Harmony drops (choir-specific).
- Remaining participants continue at reduced effectiveness.
- Social consequence: other participants may gain a negative thought about the leaver (if leaving was avoidable — hunger is understandable, but wandering off is not).

**Creature leaves mid-execution:** The response depends on the activity's `DeparturePolicy`:

- **`Continue`** (e.g., dance): The participant is removed, `creature.current_activity` is cleared, and the remaining participants keep going at reduced effectiveness. The dance continues with fewer dancers. Social thoughts are mild ("so-and-so left early").
- **`PauseAndWait`** (e.g., construction choir): The activity transitions to the `Paused` phase — remaining participants stay in place and idle (same behavior as arrived-and-waiting during assembly). Progress stops. A `pause_started_tick` is recorded on the activity. If the departed creature returns (or a replacement is recruited via directed assignment or open-recruitment discovery) within `timeout_ticks`, the replacement's `arrived` flag is set and the activity transitions back to `Executing`. If the timeout expires (checked during any participant's activation while in `Paused`), the activity is cancelled. If the last remaining participant also leaves (participant count drops to zero), the activity is cancelled immediately — no one is left to check the timer. Social thoughts are stronger ("the choir fell apart because so-and-so left").
- **`CancelOnDeparture`** (e.g., ritual): The activity is immediately cancelled. All participants are released. Strong social consequences.

In all cases, the departing participant's row is removed from `activity_participants` and their `current_activity` is cleared.

### 4. Completion (`Complete`)

When `progress >= total_cost`:
1. Activity transitions to `Complete`.
2. Kind-specific completion logic runs (construction finalizes the structure, dance generates mood bonuses for all participants).
3. All participants are released: `current_activity` cleared, `ActivityParticipant` rows removed.
4. Participants generate positive thoughts (participated in a successful choir, had a great dance).
5. Activity row and all `ActivityParticipant` rows are deleted immediately (cascade). Extension table rows (choir data, dance data) are cascade-deleted with the activity. There is no history retention — the thoughts on participants serve as the persistent record.

### 5. Cancellation (`Cancelled`)

Triggers:
- Player cancels the construction project.
- `PauseAndWait` timeout expired after a departure.
- `CancelOnDeparture` policy and someone left.
- Assembly timeout exceeded.
- Participant count drops to zero.
- Organizer died or left and no replacement was chosen.

On cancellation:
- All in-flight GoTo tasks for assembly are cancelled.
- All participants are released (`current_activity` cleared, participant rows deleted).
- Kind-specific cleanup runs via the extension table's feature logic. The general principle: any world state produced during execution remains (e.g., partially materialized voxels from a construction choir stay in the world), and the underlying project/resource reverts to an available state so a new activity can resume it. Specifics are deferred to each feature's design (F-choir-build, F-group-dance, etc.).
- Activity row is deleted.

## Integration Points

### Activation Loop Changes

The activation loop in `activation.rs` currently follows:

```
flee check → autonomous combat check → has current_task? →
  execute_task_behavior / find_available_task → wander
```

The autonomous combat check (between flee and task execution) detects
nearby hostiles and initiates pursuit if the creature's engagement style
is Aggressive or Defensive and its current task is at exactly the
Autonomous preemption level (or the creature has no task at all).

With activities, the cascade becomes:

```
flee check → autonomous combat check →
  has current_activity (Executing or Paused phase)?
    → execute_activity_behavior (or idle-in-place if Paused)
  has current_task?
    → execute_task_behavior (may be an activity's GoTo during assembly)
  find_available_task →
  find_open_activity (Open recruitment, if creature is eligible) →
  wander
```

Key points:
- The activity check comes *before* the task check because during execution, the creature has no task — it's being driven by the activity. During assembly, the creature has both an activity and a task (GoTo), and the task system drives movement normally.
- **Autonomous combat and activities:** The current autonomous combat check has a three-way branch: if the creature has a task, it checks whether the task's preemption level is exactly `Autonomous` (the only level that permits autonomous combat interruption); if the creature has no task, it unconditionally tries combat. With activities, this needs a third branch: if the creature has a `current_activity` but no `current_task` (i.e., executing or paused), compute the activity's preemption level via `activity_preemption_level(kind, origin)` and only try combat if that level is `Autonomous`. Without this, a creature executing a `PlayerDirected` construction choir would be unconditionally pulled into combat by the `else true` fallthrough. The check becomes: has task → check task level == Autonomous; has activity (no task) → check activity level == Autonomous; neither → try combat.
- **Open-recruitment discovery** sits between `find_available_task` (no task found) and wander. An idle creature with no task checks for open activities accepting participants, filtered by proximity and eligibility. If one is found, the creature joins and gets a GoTo task for assembly.
- **Arrived-and-waiting:** When a creature's assembly GoTo task completes but the activity is still in the `Assembling` phase (waiting for others), the creature has `current_activity` set but `current_task = None` and the activity is not yet `Executing`. The activation loop sees `current_activity` and the `Assembling` phase, so it idles the creature in place (schedule next activation, do nothing). This is similar to how a creature with no task and no available work just wanders — except here it stays put at its assigned position.

### Preemption

New preemption consideration: activities need a preemption level. Options:
- Store the preemption level on the Activity row (set at creation based on kind + origin).
- Compute it from `(ActivityKind, TaskOrigin)` like tasks do from `(TaskKindTag, TaskOrigin)`.

The latter is cleaner and follows the existing pattern. Add `ActivityKind` arms to the preemption system, or add a parallel `activity_preemption_level()` function.

When something tries to preempt a creature in an activity:
- During assembly (creature has a GoTo task): normal task preemption rules apply to the GoTo.
- During execution (creature has no task, only activity): compare the new task's preemption level against the activity's level.

### Heartbeat

The heartbeat system (which creates autonomous tasks for hunger, sleep, etc.) needs to know about activities:
- A creature in an executing activity should not get autonomous task creation *unless* the need is at Survival level or above.
- This is already handled by preemption — the heartbeat creates tasks, and they'll only preempt if their level exceeds the activity's level.

### Event Scheduling

Activities need their own event types for phase transitions:
- `ActivityAssemblyCheck` — periodic check during assembly to see if enough participants have arrived, handle timeouts.
- `ActivityExecutionTick` — if the activity needs periodic group-level logic beyond individual creature activations (e.g., recalculating harmony for the whole choir once per second rather than per-creature).
- `ActivityCompletion` — when progress reaches total_cost.

These could be `ScheduledEventKind` variants, or they could piggyback on creature activations (each creature's activation contributes progress and checks for completion).

The simpler approach: **no activity-level events**. Each creature's activation during the executing phase contributes progress and checks `if activity.progress >= activity.total_cost`. The last creature to push progress over the threshold triggers completion. This is deterministic (creature activations are ordered) and requires no new event types.

The downside is that group-level calculations (harmony) happen per-creature-activation rather than once per group tick. For MVP this is fine — harmony can be cached on the activity row and recalculated only when the participant list changes.

### Save/Load

Activities are tabulosity tables, so they serialize and deserialize with the rest of the DB. No special handling needed beyond ensuring the new tables are included in the `SimDb` derive.

The `current_activity` FK on Creature needs the same treatment as `current_task` — nullable FK with appropriate on-delete behavior (if the activity is deleted, nullify the creature's reference).

### UI

The task panel currently shows tasks grouped by origin and assigned creatures. Activities could appear in a separate section or as a distinct category within the same panel. Each activity shows:
- Kind, phase, participant count / required count.
- Progress bar (during execution).
- Participant list with arrival status (during assembly).
- Cancel button (for player-directed activities).

## Code Changes Summary

### New Files
- `elven_canopy_sim/src/sim/activity.rs` — activity lifecycle logic (creation, phase transitions, execution behavior, completion, cancellation).
- `elven_canopy_sim/src/activity_types.rs` — activity creation DTO types and enums (`ActivityKind`, `ActivityPhase`, `RecruitmentMode`, `DeparturePolicy`, `ParticipantRole`). Named `activity_types.rs` rather than `activity.rs` to avoid confusion with `sim/activity.rs`.

### Modified Files
- `elven_canopy_sim/src/types.rs` — add `ActivityId`.
- `elven_canopy_sim/src/db.rs` — add `Activity`, `ActivityParticipant` tables; add `current_activity: Option<ActivityId>` to `Creature`; add tables to `SimDb`. Kind-specific extension tables added per-feature.
- `elven_canopy_sim/src/sim/activation.rs` — add activity check before task check in the activation cascade; add `execute_activity_behavior` dispatch.
- `elven_canopy_sim/src/preemption.rs` — add activity preemption level calculation; handle preemption checks for creatures in activities.
- `elven_canopy_sim/src/sim/task_helpers.rs` — helper to create GoTo tasks for activity assembly, helper to release a creature from an activity.
- `elven_canopy_sim/src/command.rs` — new `SimCommand` variants: `CreateActivity { kind, location, ... }`, `CancelActivity { activity_id }`, `AssignToActivity { activity_id, creature_id }`, `RemoveFromActivity { activity_id, creature_id }`.
- `elven_canopy_sim/src/event.rs` — possibly new `ScheduledEventKind` variants (if activity-level events are needed).
- `elven_canopy_sim/src/config.rs` — activity-related config (assembly timeout, default departure policies per kind, late-join defaults). Kind-specific config (harmony parameters, group bonuses, etc.) added per-feature.

### Not Changed
- `elven_canopy_sim/src/task.rs` — task kinds, task state, task lifecycle are untouched.
- `elven_canopy_sim/src/sim/movement.rs` — creature movement is unchanged; assembly uses normal GoTo tasks.
- `elven_canopy_sim/src/sim/construction.rs` — existing solo build logic stays. Choir-build is a new code path that lives in `activity.rs`.
- `elven_canopy_sim/src/sim/needs.rs` — heartbeat task creation unchanged; preemption handles the interaction.

## Resolved Design Decisions

1. **Recruitment: push and pull.** Both are supported via `RecruitmentMode`. `Directed` for player/system-initiated activities (push), `Open` for autonomous social activities (pull). An activity could even mix modes — player picks 3 singers, remaining slots are open for volunteers.

2. **Minimum vs. desired count.** Two separate fields: `min_count` (activity can start, `None` means no minimum) and `desired_count` (optimal, recruitment continues up to this). A construction choir might need `min_count = 3` to start but works best at `desired_count = 6`. A dance might have `min_count = None` (solo dancing is fine) but `desired_count = 8`.

3. **Departure policy is per-activity-instance with per-kind defaults.** Different activities have fundamentally different responses to participants leaving. `Continue` (dances — keep going), `PauseAndWait` (construction choirs — pause, wait for replacement, give up after timeout), `CancelOnDeparture` (rituals — must be unbroken). The `DeparturePolicy` enum captures this with a `Paused` activity phase for the wait state. Defaults come from config per `ActivityKind`; player can override for player-directed activities.

4. **No concurrent task during execution.** During execution, `current_task` is `None` and behavior is driven by the activity. If a creature needs to do something else (fetch mana crystals, eat), it leaves the activity temporarily — the departure policy governs what happens to the activity when they do.

5. **Late joiners are per-kind.** Whether participants can join mid-execution varies by activity kind, configured as a per-kind flag in config. Dances allow late joiners (a dancer joining an ongoing dance is natural). Construction choirs do not (joining a song mid-verse disrupts harmony). Late joiners during execution go through the same assembly flow — GoTo to assigned position, then join the executing group.

6. **Cancellation preserves world state.** When an activity is cancelled, any world state produced during execution remains. Underlying projects/resources revert to available so a new activity can resume (e.g., a construction choir's partial voxels stay, and the blueprint becomes available for a new choir).

## Open Questions

1. **Activity-level progress vs. per-creature progress.** The design uses activity-level progress only. But for UI purposes, it might be useful to track per-creature contribution (who sang the most? who slacked off?). This can be added later as a per-participant field without schema changes to the core model.

2. **Interaction with F-task-priority.** When F-task-priority is implemented, activities will need to participate in the priority system. A high-priority construction project's choir should recruit creatures away from low-priority work. The activity's priority could be inherited from the construction project's priority.

3. **Interaction with F-jobs.** When F-jobs is implemented, recruitment should respect job assignments — only recruit elves with a Builder job for construction choirs, only Musicians for combat singing, etc.

4. **Interaction with F-activation-revamp.** If the activation system moves to automatic reactivation, creatures in activities need to be included. A creature executing an activity should be reactivated at the same cadence as a creature executing a task.
