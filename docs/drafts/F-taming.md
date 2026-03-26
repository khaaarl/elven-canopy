# F-taming: Tame Neutral Creatures via Scout-Path Elves

**Version:** v6
**Status:** Draft
**Depends on:** F-path-core (done), F-creature-skills (in progress)
**Related:** F-civ-pets, F-tame-aggro, F-task-tags

## Motivation

The player's civilization is currently elf-only. Taming adds a new axis of
interaction with the world's fauna — a Scout can win over a wild capybara or,
with enough skill, even a wyvern. Tamed creatures join the player's
civilization and wander the settlement, laying groundwork for future features
(hauling, war animals, cavalry, bonds).

## Design Overview

### Core Loop

1. Player opens a neutral creature's info panel and toggles the **Tame** button.
2. The sim marks the creature as **tame-designated**.
3. Any idle Scout-path elf can claim the open taming task (proximity-based,
   like other tasks).
4. The Scout walks to the creature and attempts taming. Each attempt is a
   quick action (configurable, default ~3 seconds) with a probabilistic
   success roll.
5. On failure: nothing happens (no aggro yet — see F-tame-aggro). The Scout
   immediately tries again.
6. Each attempt advances the Scout's Beastcraft skill.
7. On success: the creature's `civ_id` changes to the player's civilization.
   It wanders freely like other civ creatures. Tamed creatures retain
   their species-level `EngagementStyle` and `hostile_detection_range_sq`,
   so a tamed boar will auto-engage nearby hostiles using its species
   defaults. Suppressing or directing this behavior is deferred to
   F-war-animals. Tamed non-elf creatures do not receive a military group
   assignment (`military_group: None`); military group support for tamed
   creatures is also deferred to F-war-animals. **Note:** tamed non-elf
   creatures have species food/rest decay but no autonomous feeding behavior
   yet — they will eventually starve. This is a known gap; animal feeding
   and husbandry are deferred to F-animal-husbandry. As a stopgap, the
   implementer should consider zeroing `food_decay_per_tick` for tamed
   non-elves, or this can be accepted as a short-lived limitation.
8. Player can untoggle the Tame button at any time, canceling the designation
   and any in-progress taming task.

### Taming Roll

A one-sided threshold check using the same `quasi_normal` noise distribution
as combat (see `combat.rs`), but structurally simpler — no defender side.
A new standalone function, not a reuse of `roll_hit_check`.

```
tamer_total = (WIL + CHA + Beastcraft) + quasi_normal(rng, 50)
success = tamer_total >= tame_difficulty
```

- **WIL (Willpower):** Mental fortitude to persist and project calm.
- **CHA (Charisma):** Ability to project trustworthiness to the animal.
- **Beastcraft:** Learned skill in animal handling. Scouts cap Beastcraft
  at 200 (vs. the default cap of 100 for non-Scout paths), and get +1
  extra advancement roll per attempt.
- **quasi_normal(rng, 50):** The same `quasi_normal` noise function used
  elsewhere in the sim (stdev 50). Adds variance so even a mediocre Scout
  can occasionally tame a hard creature, and a skilled one can still fail.
- **tame_difficulty:** Per-species threshold on `SpeciesData`. `None` means
  untameable (sapient species). Higher values require more stat/skill
  investment.

**PRNG footprint per attempt:** Each attempt makes 1 call to `quasi_normal`
(which always invokes `range_i64_inclusive` exactly 12 times internally) and
1 call to `try_advance_skill` (which always consumes `1 + extra_rolls` raw
`next_u64()` draws, i.e., 2 for a Scout). Note: `range_i64_inclusive` uses
rejection sampling, so each logical call may consume 1+ raw PRNG draws
(though rejection is extremely rare for the range used). The key invariant
is that the call pattern is input-independent (same regardless of roll
outcome or skill cap), preserving determinism.

### Difficulty Values

| Species   | tame_difficulty | Spawns neutral? | Notes                              |
|-----------|-----------------|------------------|------------------------------------|
| Capybara  | 100             | Yes              | Easy — a starting Scout can manage |
| Squirrel  | 100             | Yes              | Easy — small, docile               |
| Deer      | 150             | Yes              | Moderate — skittish                |
| Monkey    | 150             | Yes              | Moderate — clever, evasive         |
| Boar      | 200             | Yes              | Hard — aggressive temperament      |
| Elephant  | 250             | Yes              | Very hard — strong-willed          |
| Hornet    | 350             | No (aggressive)  | Extreme — future-proofing value    |
| Wyvern    | 350             | No (aggressive)  | Extreme — future-proofing value    |
| Elf       | None            | No               | Sapient — untameable               |
| Goblin    | None            | No (hostile civ) | Sapient — untameable               |
| Orc       | None            | No (hostile civ) | Sapient — untameable               |
| Troll     | None            | No (hostile civ) | Sapient — untameable               |

These are `GameConfig` values, easily tuned without code changes.

**Calibration sketch:** A starting Scout with ~50 WIL, ~50 CHA, 0
Beastcraft has a base of 100. The noise (stdev 50) adds variance:
- vs. Capybara (100): base ties difficulty, ~50% success per attempt.
- vs. Deer (150): needs noise +50 (~1σ), ~16% per attempt.
- vs. Boar (200): needs noise +100 (~2σ), ~2% per attempt.
- vs. Elephant (250): needs noise +150 (~3σ), near-impossible without
  skill investment.

A leveled Scout with 80 WIL, 80 CHA, 100 Beastcraft (base 260) tames
elephants comfortably and capybaras almost every attempt. The difficulty
values use the raw linear sum intentionally — `apply_stat_multiplier` is
not used here because the formula combines two stats plus a skill, and the
threshold values are calibrated to that scale.

**Note on flying creatures:** Hornets and wyverns currently spawn as aggressive
wild creatures (`civ_id: None` but nonzero `hostile_detection_range_sq`), not as
neutrals. Since `DesignateTame` validates `civ_id: None` (see below), they are
technically designatable, but the Scout cannot reach an airborne creature — the
task will path-fail until the creature is adjacent to a walkable node. In
practice, hornets and wyverns are not tameable until a "lure to ground" mechanic
or flying-creature-adjacency rule is added. The difficulty values are retained
for future-proofing.

### Attempt Timing

Each taming attempt is a simple action with configurable duration (default:
`tame_attempt_ticks` = 3000 in `GameConfig`, ~3 seconds at 1000 ticks/sec).
The duration is fixed and not modified by stats or skills — the skill benefit
is reflected in the success probability, not attempt speed. The Scout
repeats attempts until success or cancellation. Between attempts, the Scout
re-evaluates: if the target has wandered, the Scout re-paths to it. No
special "hold still" state on the target — the Scout chases.

If the Scout gets preempted by a higher-priority task (combat, fleeing), the
taming task remains available for the same or another Scout to claim later.

### Skill Advancement

Each taming attempt calls `try_advance_skill(elf_id, TraitKind::Beastcraft,
base_probability)` with a configurable base probability. The Scout's path
grants +1 extra advancement roll (from `PathDef.extra_advancement_rolls`),
so Scouts learn Beastcraft roughly twice as fast as non-Scouts would (if
non-Scouts could tame, which they currently can't).

Taming difficult creatures doesn't give more XP per attempt — but it takes
more attempts, so the Scout naturally trains more.

## Data Model

### SpeciesData Addition

```rust
/// Taming difficulty threshold. `None` = untameable (sapient species).
/// `Some(n)` = the tamer needs `(WIL + CHA + Beastcraft) + noise >= n`
/// to succeed on each attempt.
#[serde(default)]
pub tame_difficulty: Option<i64>,
```

### Tame Designation

A new tabulosity table tracks which creatures are designated for taming.
This is intentionally separate from the `Task` table: designations represent
the player's persistent intent, surviving task cancellation/re-creation
cycles (e.g., Scout preempted by combat → task returns to Available → new
Scout claims it). The designation table also gives the UI a single O(1)
lookup to check "is this creature designated?" for the toggle button state.
Old saves load cleanly because tabulosity deserializes missing tables as
empty by default.

**Consistency invariant:** A `tame_designations` entry can become orphaned
if the target creature dies and no Scout ever re-claims the task (e.g., all
Scouts are dead). This is harmless — the designation just shows a checked
button on a dead creature's panel, which the player can uncheck. No
reconciliation sweep is needed. On save/load, `tame_designations` entries
referencing deleted creatures are inert (the creature row still exists with
`vital_status = Dead`; creature rows are never deleted).

```rust
/// Creatures the player has marked for taming via the UI toggle.
/// Presence in this table = "an open taming task should exist."
/// No FK to `creatures` — orphaned entries (dead creatures) are harmless
/// and cleaned up lazily at activation time.
pub tame_designations: Table<CreatureId, TameDesignation>,
```

```rust
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TameDesignation {
    #[primary_key]
    pub creature_id: CreatureId,
    /// Tick when designation was created (for UI display, task ordering).
    pub designated_tick: u64,
}
```

### TaskKind Variant

```rust
TaskKind::Tame {
    target_creature: CreatureId,
}
```

This follows the codebase's task decomposition pattern: `TaskKind` is a
creation DTO passed to `insert_task()`, which decomposes it into the base
`Task` row (with `TaskKindTag::Tame`) plus a `TaskTameData` extension
table row. The `TaskKind` enum is never reconstructed from the DB — reads
use query helpers on `SimState`. See `TaskAttackTargetData` for the model.

**Extension table:**

```rust
/// Tame task extension data — stores the target creature.
/// The target is a plain `CreatureId`, not an FK — the task polls
/// the target's vital_status each activation and completes if dead.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskTameData {
    #[primary_key]
    pub task_id: TaskId,
    /// Target creature to tame. Plain ID, not FK — checked each tick.
    pub target: CreatureId,
}
```

The `TaskTameData` table is added to `SimDb` alongside the existing
extension tables (`task_attack_target_data`, `task_attack_move_data`,
etc.). `insert_task()` gains a `TaskKind::Tame` arm that inserts the
extension row, mirroring the `AttackTarget` arm.

- **Location:** Target creature's current nav node (re-resolved each time
  the Scout needs to path to it).
- **Required species:** Elf — set via `required_species: Some(Species::Elf)`
  on the task. **Required civ:** player's civ — set via
  `required_civ_id: Some(player_civ_id)`. These use the existing
  field-based filters.
- **Path restriction:** Scout only. No path-based filtering exists in
  `find_available_task` today — this is a new filter axis. Implementation:
  add a `match` on `TaskKindTag::Tame` inside `find_available_task`'s
  filter closure that queries the creature's path and rejects non-Scouts.
  This follows the same structural pattern as the existing field-based
  filters (`required_species`, `required_civ_id`) but is kind-specific
  rather than field-driven. A generic `required_path: Option<PathId>`
  field on `Task` would be cleaner but is broader scope — F-task-tags
  will generalize this later.
- **Origin:** `TaskOrigin::PlayerDirected` (since the player initiates
  taming via the UI toggle). `preemption_level()` maps
  `(TaskKindTag::Tame, _)` to `PreemptionLevel::Autonomous` regardless
  of origin, same as Craft/Haul/Harvest.
- **No `total_cost`:** The task doesn't have a progress bar. Each activation
  is an independent success roll. The task completes when a roll succeeds.
- **Preemption level:** Autonomous (same tier as Craft/Haul/Harvest) —
  interruptible by needs, mood, combat, and player commands. Adding the
  `Tame` variant to `TaskKindTag` requires updating its exhaustive match
  arms in `preemption_level()` and `display_name()` (compile errors enforce
  this).

### SimAction Variants

```rust
/// Player toggles tame designation on a neutral creature.
SimAction::DesignateTame {
    target_id: CreatureId,
},

/// Player removes tame designation.
SimAction::CancelTameDesignation {
    target_id: CreatureId,
},
```

Both variants are serialized as part of `SimCommand` for multiplayer relay,
so their serde representation must remain stable once shipped.

`DesignateTame` validates:
- Target is alive
- Target's species has `tame_difficulty: Some(_)`
- Target has `civ_id: None` (wild/unaffiliated — taming hostile-civ creatures
  is out of scope)
- Target is not already in the player's civilization
- Target is not already designated

**Note:** Aggressive wild creatures (`civ_id: None` with nonzero
`hostile_detection_range_sq`, e.g., hornets, wyverns) pass all five checks
and can technically be designated. However, the Scout will be attacked on
approach. This is allowed by design — the player assumes the risk. In
practice, flying aggressives are also unreachable (see edge cases). Future
F-tame-aggro will add explicit aggro-on-fail mechanics for this scenario.

Then inserts into `tame_designations` and creates the `TaskKind::Tame` task.

`CancelTameDesignation` removes the designation and cancels any in-progress
taming task on that creature.

### SimEvent

```rust
/// A creature was successfully tamed and joined the player's civilization.
SimEventKind::CreatureTamed {
    creature_id: CreatureId,
    tamer_id: CreatureId,
},
```

The GDScript layer uses this to update the info panel and show a
notification.

## Task Execution

When a Scout claims and executes a `Tame` task:

1. **Move phase:** Walk to the target creature's current position. For
   multi-voxel creatures (wyvern 2x2x2, elephant 2x2x2), "adjacent"
   means within 1 voxel of the creature's AABB, same as melee range
   checks. If already adjacent, skip.
2. **Action phase:** Start a `tame_attempt_ticks` action. On completion:
   a. Look up the target's `tame_difficulty`.
   b. Read tamer's WIL, CHA, Beastcraft.
   c. Roll: `tamer_total = WIL + CHA + Beastcraft + quasi_normal(rng, 50)`.
   d. Call `try_advance_skill(tamer_id, Beastcraft, base_prob)`.
   e. If `tamer_total >= tame_difficulty`: **success**.
      - Set `creature.civ_id = Some(player_civ_id)` (obtained from the
        tamer's own `civ_id`, which is guaranteed `Some` since only
        player-civ Scouts can claim the task).
      - Remove from `tame_designations`.
      - Emit `SimEventKind::CreatureTamed`.
      - Complete the task.
   f. If failed: loop back to step 1 (re-check position, attempt again).
3. **Interruption:** If the Scout is preempted (combat, flee), the task
   returns to Available state. Any Scout can re-claim it.

The target creature has no special state — it continues its normal AI
(wandering, etc.) throughout. The Scout chases.

### Edge Cases

- **Target dies during taming:** Creature death sets `vital_status = Dead`
  (the creature row is not deleted). The taming task checks `vital_status`
  at activation time (same pattern as `AttackTarget`): if dead, the task
  completes silently and the `tame_designations` entry is removed. No
  death-handler-side task scanning is needed — the existing
  activation-time check suffices. The `tame_designations` entry itself
  is cleaned up when the task completes or when the Scout re-activates
  and finds the target dead.
- **Target changes civ (multiplayer):** The success handler re-validates
  that the target is still not in the player's civ before applying the
  `civ_id` change. If another player tamed it first, the task silently
  completes (designation removed, no event emitted).
- **Hostile creatures targeting newly-tamed creature:** Existing hostile
  `AttackTarget` tasks targeting the creature will naturally resolve —
  combat checks skip friendly targets. No special cleanup needed.
- **Target unreachable (general):** If the target creature wanders to a
  position the Scout cannot reach (across a gap, up a tree, etc.), the
  Scout will path-fail. On path failure the task returns to Available
  state so another Scout (or the same one later) can re-claim it. No
  retry limit or backoff — the creature may wander back into reach, and
  the player can always cancel the designation manually.
- **Target is a flying creature (hornet, wyvern):** Same as above — the
  Scout must reach the target's nav node. In practice, flying creatures
  are not tameable until a ground-lure mechanic is added. See the note
  in the Difficulty Values section.

## UI

### Creature Info Panel — Tame Button

A toggle button in the creature info panel's status tab, visible only for
wild creatures (`civ_id: None`) whose species has `tame_difficulty: Some(_)`.

- **Label:** "✗ Tame" (unchecked) / "✓ Tame" (checked)
- **Behavior:** Sends `SimAction::DesignateTame` or
  `SimAction::CancelTameDesignation`.
- **Visual feedback:** When designated, the button stays checked. The
  creature's status line could show "Taming designated" or similar.

The button is hidden for:
- Creatures with any `civ_id` (player-civ, hostile-civ, already tamed)
- Species with `tame_difficulty: None` (sapient, untameable)
- Dead creatures

### Notifications

On `SimEventKind::CreatureTamed`:
- Notification: "{Tamer name} tamed {creature species}!"
- Category: positive event

### Units Panel

Tamed non-elf creatures appear in the existing units panel. No new section
needed initially — they're just civ members with a different species. A
dedicated "Animals" section is a future nicety under F-civ-pets.

## Config

New fields in `GameConfig`:

```rust
/// Duration of each taming attempt in sim ticks.
/// At 1000 ticks/sec, 3000 = 3 seconds per attempt.
pub tame_attempt_ticks: u64,  // default: 3000 (~3 seconds)

/// Base probability (permille) for Beastcraft skill advancement per
/// taming attempt. Lower than construction (1000) and crafting (800)
/// because taming attempts are repeated many times per taming, but
/// comparable to combat (500) given the similar ~3s action duration.
pub tame_skill_advance_probability: u32,  // default: 50 permille (i.e., 5.0%)
```

Per-species `tame_difficulty` values live in `SpeciesData` within the
existing `config.species` map.

## Scope Boundaries

**In scope:**
- `tame_difficulty` on `SpeciesData`
- `DesignateTame` / `CancelTameDesignation` sim actions
- `TaskKind::Tame` with Scout-path restriction (hardcoded)
- Taming roll: `(WIL + CHA + Beastcraft) + quasi_normal >= tame_difficulty`
- Beastcraft skill advancement per attempt
- Creature joins player civ on success (just `civ_id` change)
- Tame toggle button on creature info panel
- `CreatureTamed` notification

**Out of scope (future features):**
- Taming thoughts/mood effects — no `ThoughtKind::TamedCreature` in this
  feature. The tamer gets no mood impact from taming. Consider adding
  positive-on-success (and negative-on-failure under F-tame-aggro) later.
- Aggro on failed taming (F-tame-aggro)
- Capability tags replacing hardcoded species checks (F-task-tags)
- Tamed creature tasks beyond wandering (F-civ-pets)
- War training, cavalry, bonds (F-war-animals, F-cavalry, F-animal-bonds)
- Labor assignment panel (F-labor-panel)
- Animal needs and husbandry (F-animal-husbandry)

## Implementation Plan

### Phase 1: Sim — Data Model
1. Add `tame_difficulty: Option<i64>` to `SpeciesData` with config defaults.
2. Add `tame_attempt_ticks` and `tame_skill_advance_probability` to `GameConfig`.
3. Add `TameDesignation` struct and `tame_designations` table to SimDb.
4. Add `TaskTameData` extension table to SimDb; wire `insert_task()` to
   decompose `TaskKind::Tame` into base task row + `TaskTameData` row.
5. Add `TaskKind::Tame` variant and `TaskKindTag::Tame`.
6. Add `TaskKindTag::Tame` match arm in `preemption_level()` (Autonomous)
   and `display_name()`.
7. Add `SimAction::DesignateTame` and `SimAction::CancelTameDesignation`.
8. Add `SimEventKind::CreatureTamed`.
9. Serde roundtrip tests for all new types.

### Phase 2: Sim — Taming Logic
1. Test: designating a tameable creature creates a tame task.
2. Test: designating an untameable creature is rejected.
3. Test: canceling a designation removes the task.
4. Implement `DesignateTame` / `CancelTameDesignation` handlers.
5. Test: taming roll succeeds when stats exceed difficulty.
6. Test: taming roll fails when stats are below difficulty.
7. Test: successful tame changes `civ_id` to player's.
8. Test: Beastcraft skill advances on taming attempts.
9. Test: only Scout-path elves can claim taming tasks (non-Scouts skip
   `TaskKindTag::Tame` in `find_available_task`).
10. Add `TaskKindTag::Tame`-specific path filter in `find_available_task`'s
    filter closure.
11. Implement tame task execution in activation.
12. Test: taming task returns to Available after Scout is preempted;
    another Scout can claim it.
13. Test: target death causes taming task to complete silently on next
    activation (activation-time `vital_status` check), removing designation.
14. Implement activation-time `vital_status == Dead` check in tame task
    execution (same pattern as `AttackTarget`).
15. Test: success re-validates target civ before applying `civ_id` change.

### Phase 3: GDExt Bridge
1. Add `SimAction::DesignateTame` and `SimAction::CancelTameDesignation` to
   the command handler and expose them through the GDExt bridge.
2. Expose `tame_difficulty` in creature info dictionary.
3. Expose `CreatureTamed` event.

### Phase 4: UI
1. Add Tame toggle button to creature info panel.
2. Wire button to sim commands.
3. Add "Taming designated" status display.
4. Add `CreatureTamed` notification.

## Changelog

### v2
- Fixed misleading claim that taming roll reuses `roll_hit_check`; clarified it is a new standalone threshold check using the same noise distribution.
- Explicitly stated that Scout-path restriction is a hardcoded check in `TaskKindTag::Tame` activation, not a generic `required_path` field on Task.
- Fixed typo: `CancelTameDesigantion` → `CancelTameDesignation`.
- Added Edge Cases section: target death cleanup, multiplayer civ re-validation on success, flying creature reachability.
- Clarified PRNG footprint: 14 total calls per attempt (12 quasi_normal + 2 try_advance_skill).
- Specified multi-voxel adjacency for taming (AABB-based, same as melee).
- Added justification for low `tame_skill_advance_probability` (50 permille) vs. other skills.
- Added implementation plan steps for target death cleanup and civ re-validation tests.

### v3
- Fixed `SimEvent::CreatureTamed` → `SimEventKind::CreatureTamed` throughout (the actual enum is `SimEventKind`, wrapped in a `SimEvent` struct).
- Added food/starvation warning: tamed non-elf creatures have food decay but no feeding behavior yet; noted as a known gap deferred to F-animal-husbandry.
- Added `civ_id: None` validation to `DesignateTame` — taming only applies to wild/unaffiliated creatures, not hostile-civ members.
- Updated difficulty table with "Spawns neutral?" column; clarified that hornets/wyverns are aggressive wild creatures and effectively untameable until a ground-lure mechanic is added.
- Added note about hostile creatures targeting newly-tamed creatures (natural resolution via combat checks).
- Added schema version bump note for `tame_designations` tabulosity table (old saves load cleanly via missing-tables-default-to-empty).
- Clarified Phase 3 step 1 wording: command handler + GDExt bridge, not just "expose as sim commands."
- Changed `tame_skill_advance_probability` type from `u64` to `u32` to match `try_advance_skill`'s parameter type.
- Clarified Scout Beastcraft cap wording: "vs. the default cap of 100 that applies to all other paths."
- Added explicit death handler wiring step to Phase 2 implementation plan.
- Updated UI visibility rules: button shown only for `civ_id: None` creatures, hidden for any creature with a civ.

### v4
- Fixed PRNG footprint section: `range_i64_inclusive` uses rejection sampling so raw draw count per call is variable (though almost always 1); dropped the incorrect "always 14" total and clarified the key invariant is input-independent call pattern.
- Specified Scout-path check happens at claim time in `find_available_task` (not activation time), preventing non-Scouts from walking to a creature only to fail.
- Added justification for separate `tame_designations` table vs. relying solely on `Task`: designations represent persistent player intent surviving task lifecycle, and provide O(1) UI lookup.
- Fixed death handler cleanup: creature death sets `vital_status = Dead` (row is not deleted); taming task checks `vital_status` at activation time (same pattern as `AttackTarget`), not via death-handler-side cleanup. Updated implementation plan steps accordingly.
- Added general "target unreachable" edge case for non-flying creatures that wander out of Scout reach (path-fail → task returns to Available).
- Explicitly documented that aggressive wild creatures (`civ_id: None` with `hostile_detection_range_sq > 0`) pass designation validation — the player assumes the risk; F-tame-aggro will add dedicated mechanics.
- Fixed `tame_attempt_ticks` default from 150 to 3000 (150 ticks = 0.15 seconds at 1000 ticks/sec, not ~3 seconds).
- Added note that tamed creatures retain species-level `EngagementStyle` and `hostile_detection_range_sq`; suppression deferred to F-war-animals.

### v5
- Acknowledged Scout-path filtering as a new filter axis in `find_available_task` (not an existing pattern); specified implementation as a `TaskKindTag::Tame`-specific branch in the filter closure.
- Added consistency invariant for `tame_designations` dual bookkeeping: orphaned entries are harmless (creature rows are never deleted), no reconciliation sweep needed.
- Specified preemption level for `TaskKind::Tame` (Autonomous, same as Craft/Haul); added `TaskKindTag::Tame` match arm steps to implementation plan.
- Added calibration sketch showing expected success rates at various stat/skill levels against each difficulty tier; noted raw linear sum is intentional (not `apply_stat_multiplier`).
- Reworded implementation plan step 11 → 12: "taming task returns to Available after preemption; another Scout can claim it" (not "resumes").
- Added thought/mood deferral note to scope boundaries: no `ThoughtKind::TamedCreature` in this feature.
- Simplified `quasi_normal` reference to "used elsewhere in the sim" instead of specifically referencing combat.
- Added note that new `SimAction` variants participate in multiplayer serialization with stable serde representations.
- Clarified `tame_skill_advance_probability` default as "50 permille (i.e., 5.0%)".
- Added military group note: tamed non-elf creatures get `military_group: None`; deferred to F-war-animals.
- Renumbered implementation plan steps after insertions (Phase 1: 8 steps, Phase 2: 15 steps).

### v6
- Added `TaskTameData` extension table spec (mirroring `TaskAttackTargetData`), with full tabulosity-annotated struct. Documented the task decomposition pattern: `TaskKind` is a creation DTO decomposed by `insert_task()` into base task row + extension table row.
- Added tabulosity `#[derive(Table)]` and `#[primary_key]` annotations to `TameDesignation` struct. Specified no FK to `creatures` — orphaned entries cleaned up lazily.
- Specified `required_species: Some(Species::Elf)` and `required_civ_id: Some(player_civ_id)` on Tame tasks (using existing field-based filters).
- Specified `TaskOrigin::PlayerDirected` for Tame tasks; `preemption_level()` maps `(TaskKindTag::Tame, _)` to `Autonomous` regardless of origin.
- Specified `player_civ_id` is obtained from the tamer's own `civ_id` (not hardcoded `CivId(0)`).
- Added note that taming attempt duration is fixed (not modified by stats/skills).
- Reworded Beastcraft cap: "Scouts cap Beastcraft at 200 (vs. the default cap of 100 for non-Scout paths)."
- Removed incorrect "schema version bump" language; tabulosity handles missing tables automatically.
- Added `TaskTameData` step to Phase 1 implementation plan (step 4); renumbered (Phase 1: 9 steps).
