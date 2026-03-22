# F-elf-paths: Elf Path System (Way → Calling → Attunement)

**Status:** Draft
**Depends on:** F-creature-stats (done), F-emotions-basic (done)
**Related:** F-apprentice, F-emotions, F-buff-system, F-war-magic, F-combat-singing

**Note on personality axes:** This design references personality axes
(Ambition, Conscientiousness, Temperament, etc.) from §18 of the design doc.
These axes are **not yet implemented** — F-emotions-basic only provides mood
score from thought weights, not personality. Features that depend on
personality axes (Way → Calling personality gate, self-assignment personality
compatibility, personality drift) are deferred until the personality system
exists (F-emotions or a dedicated F-personality tracker item). The core
mechanics (XP, levels, stat bonuses, tier transitions, Attunement lock) work
without personality and should be implemented first.

## Motivation

Elves currently have no skill progression or specialization. Every elf is
interchangeable — same capabilities, same potential. This flattens both the
strategic depth (army composition doesn't matter) and the narrative richness
(elves don't develop identities through their work).

Inspired by the Asuryani Path system from Warhammer 40k, this feature gives
elves a progression system where they walk **paths** — disciplines they commit
to with escalating depth. Unlike typical RPG class systems, the interesting
tension is in *commitment*: the deeper an elf goes, the better they get, but
the harder it is to change direction. At the deepest level (Attunement), the
commitment becomes permanent — a feature of elven psychology, not a limitation
of the game system.

## Design Overview

### Core Concepts

Each elf has a **current path** (or none — the default starting state). Paths
are organized into a hierarchy:

```
Category (Combat / Civil)
  └─ Base Path (e.g., Warrior, Archer, Cook, Woodsinger)
       └─ Specialization (e.g., Blademaster, Sharpshooter, Spoon Carver)
```

(Esoteric/magic paths are a future consideration — see Open Questions.)

### Commitment Tiers

An elf's relationship to their current path escalates through three tiers as
they accumulate experience:

1. **Way** — The elf has begun walking this path. They're learning, flexible,
   can be reassigned freely. "Thaelindra walks the Way of the Bow."

2. **Calling** — The elf has invested significant time and (once personality
   axes exist) the path suits their personality. XP gain is faster (comfort
   bonus). Reassignment is possible but incurs a mood penalty and temporary XP
   drought. "The Bow has become Thaelindra's Calling."

3. **Attunement** — The elf has fully merged their identity with this path.
   They cannot leave this *category* — only specialize further within it. An
   Attuned Warrior can become a Blademaster but can never become a Cook.
   "Thaelindra is Attuned to the Way of War."

Tier thresholds are configurable per path in `GameConfig`. The transition from
Way to Calling is initially based on time-on-path and level thresholds. Once
personality axes are implemented, a personality compatibility gate can be added:
an elf who hates their assigned path may never reach Calling even with enough
XP.

### Assignment Model

**Combat paths** are always **player-assigned**. The player explicitly sets an
elf's combat path via the UI. This is essential for RTS-style army composition
— you need to be able to say "you four are archers now" without negotiation.

**Civil paths** can be **player-assigned** or **self-assigned**. If an elf
spends enough time doing a particular kind of work (cooking, crafting,
harvesting), they may organically adopt that path on their own. A notification
informs the player: "Vaelith has embraced the Way of the Cook." (Once
personality axes exist, self-assignment will also check personality
compatibility.)

**Self-assignment trigger mechanism:** Each task kind maps to a path category
(e.g., Cook tasks → Cook path, Craft tasks → Artisan path). When an unpathed
elf completes a task, the sim increments an affinity counter in the
`PathAffinity` table for that path. When the counter exceeds a configurable
threshold (`self_assign_task_count` in `PathDef`), the elf self-assigns to
that path. Affinity counters decay over time (configurable
`affinity_decay_per_heartbeat`) so that sporadic tasks don't accumulate
indefinitely.

**Attunement warning**: When any elf (combat or civil) is approaching the
Attunement threshold, the player receives an advance warning notification with
a window to intervene. "Thaelindra is becoming Attuned to the Way of the Bow.
Reassign her within N ticks or this becomes permanent." This creates a
meaningful decision point that emerges from gameplay.

### XP and Leveling

Each path has an integer XP counter. XP is gained by performing actions
relevant to the path:

- **Warrior**: landing melee hits, taking damage in melee, killing enemies
- **Archer**: landing ranged hits, killing at range
- **Cook**: completing cook tasks, higher XP for complex recipes
- **Woodsinger**: completing grow/shape construction tasks
- **Harvester**: completing harvest/forage tasks
- **Artisan**: completing craft tasks at workshops
- (etc.)

XP thresholds for each level are configured per path in `xp_per_level: Vec<u32>`.
The maximum level for a path equals the length of this vector. Once an elf
reaches the max level, no further XP is gained. Leveling up grants **fixed stat
bonuses** defined by the path — no per-elf perk choices. Every Archer at level 6
gets the same bonuses. This keeps units predictable and RTS-friendly: a group of
Sharpshooters all behave the same way.

The Calling tier provides a **comfort bonus** — XP gain is multiplied (e.g.,
1.5× via `calling_xp_multiplier_permille` in config) when the elf has reached
Calling tier.

### Stat Modifier Architecture

Path bonuses integrate with the existing creature stat system (F-creature-stats)
via a **stat modifier layer**. Currently, stats are `TraitKind` variants (e.g.,
`TraitKind::Strength`) stored as `TraitValue::Int(i64)` in the `creature_traits`
table. The `trait_int()` function queries a creature's base stat value, and
`apply_stat_multiplier()` / `apply_stat_divisor()` in `stats.rs` convert it to
a gameplay effect using the 2^20 fixed-point exponential table.

Path bonuses add a modifier layer between the base trait and the multiplier
lookup:

```
effective_stat = base_trait_int + path_bonus + residue_bonus
gameplay_effect = apply_stat_multiplier(base_value, effective_stat)
```

A new function `effective_stat(sim, creature_id, trait_kind) -> i64` replaces
direct `trait_int()` calls in combat/task code. It sums:

1. **Base stat** — the immutable `TraitValue::Int` from `creature_traits`
   (rolled at spawn, never mutated by paths)
2. **Active path bonus** — looked up from `PathAssignment.level` ×
   `PathDef.stat_bonuses_per_level` for the creature's current path
3. **Residue bonuses** — summed from `PathHistory` entries using each path's
   `residue_fraction_permille`

All components are integer. No floating-point math.

This architecture is designed to be compatible with F-buff-system (timed stat
modifiers). When buffs are implemented, `effective_stat` gains a fourth term:

```
effective_stat = base + path_bonus + residue + buff_total
```

The buff system will need its own modifier storage, but the query function is
the shared integration point.

**Civil path bonuses** don't use stat modifiers — they're task-specific
multipliers checked during task execution (e.g., craft speed permille bonus,
recipe quality tier). These are queried directly from the path level, not
through the stat pipeline.

### Stat Bonuses

Path levels provide these bonuses:

- **Warrior path**: +STR, +CON per level (flows through stat multiplier →
  melee damage, HP)
- **Archer path**: +DEX, +PER per level (flows through stat multiplier →
  aim accuracy, detection range)
- **Cook path**: craft speed permille bonus, recipe quality tier unlock
- **Woodsinger path**: construction speed bonus, mana cost reduction permille
- **Poet path**: mana generation bonus permille (applied to the elf's personal
  mana contribution; spatial mood auras are a future system, deferred)
- (etc.)

### Specialization Branching

At a configurable level threshold within a base path, the elf can
**specialize**. Specialization is a one-way fork — you pick a narrower
discipline within your base path. For combat paths, the player picks the
specialization. For civil paths, it can be player-chosen or organic (an elf
who only ever crafts spoons will specialize in spoons if left alone long
enough — tracked via the same affinity counter mechanism as self-assignment).

Specializations have **prerequisites** beyond just level. Some examples:

- **Champion** (Warrior specialization, leadership aura): requires Warrior
  level N *and* some time spent on a civil path. You can't lead if you've
  never done anything but fight.
- **Healer** (Woodsinger specialization): requires Woodsinger level N *and*
  Harvester level M. Understanding living things requires both disciplines.

Prerequisites encourage the player to route elves through varied experience
before funneling them into advanced roles. "I want this elf to eventually
become a Champion, so I need to park her on Cook for a while first."

**Specialization narrows ability.** An Artisan who specializes in Spoon Carving
*refuses* to craft chairs — this is a philosophical commitment, not amnesia.
The elf has chosen to pursue mastery in their narrow domain. The player must
either respect this or force a reassignment (mood penalty, and impossible at
Attunement level).

**Task eligibility filtering:** Specialization adds a per-creature task filter.
The task assignment loop (`find_available_task` in `activation.rs`) already
supports `required_species` filtering on tasks. Path specialization adds an
analogous check: if the creature has a specialization, tasks outside that
specialization's allowed set are skipped. If no eligible creature exists for a
task, the task remains unclaimed — the player sees it sitting in the queue and
can either reassign an elf or create a non-specialized elf to handle it. A
notification fires when tasks go unclaimed for a configurable duration due to
specialization restrictions.

### Skill Residue

When an elf leaves a path, they don't lose everything. A fraction of their
accumulated level is retained as a **passive stat residue** — an ex-Warrior
archer is slightly tougher than a pure archer. The residue is:

- A configurable fraction (e.g., 200‰ = 20%, via `residue_fraction_permille`
  per path) of the old path's stat bonuses, applied as a permanent passive
  modifier via the `effective_stat()` pipeline.
- Reduced XP cost to re-enter a previously walked path. Thaelindra can return
  to Warrior faster than someone who never walked it. But if she's Attuned to
  Archer, the Warrior door is closed (Attunement locks the category).

Residue does *not* grant abilities or active bonuses from the old path — just
a stat echo.

### Path Flexibility by Category

Not all path categories work the same way:

- **Combat paths** are self-contained. Switch to Warrior, you get Warrior
  bonuses. Switch away, you lose them (except residue). Clean and predictable.
- **Craft paths** work similarly — you're a Carver or you're not.
- **Woodsinging** sits in between — part craft, part magic. Basic shaping
  ability is retained once learned (higher residue fraction), but advanced
  techniques require active commitment.

This asymmetry is expressed through per-path configuration (residue fraction,
XP curves) rather than different mechanical systems, keeping the core system
uniform while allowing each path category to have its own personality.

### Personality Interaction (Deferred — requires F-emotions / F-personality)

Once personality axes from §18 of the design doc are implemented, they will
influence path progression:

- **Ambition** affects XP gain rate and how quickly they push toward
  specialization.
- **Conscientiousness** affects whether they stick with an assigned path or
  drift toward self-assignment.
- **Temperament** affects how they handle path transitions — dramatic elves
  have bigger mood swings on reassignment.

The Way → Calling transition will gain a personality gate: an elf won't reach
Calling on a path they're poorly suited for, even with enough XP. A gregarious
elf forced into solitary harvesting may stay at Way forever.

Until personality is implemented, tier transitions are purely threshold-based
(time + level).

### Deep Commitment: Personality Drift and Refusal (Deferred — requires F-emotions / F-personality)

Once personality axes exist, elves deeply committed to a path will undergo
**personality drift** — their axes shift to match stereotypes of their
discipline:

- Long-time Warriors become more aggressive, impatient (temperament shifts
  toward dramatic, ambition increases).
- Long-time Artisans become perfectionist, methodical (conscientiousness
  increases, sociability may decrease).
- Long-time Poets become more sensitive, expressive.

The Attunement refusal mechanic does **not** require personality — it is a hard
mechanical constraint. At Attunement level, an elf **cannot** be reassigned out
of their path category. "Thaelindra refuses to leave the Way of the Bow." The
player cannot force an Attuned elf out of their category. They can only be
directed further along their path (specialization).

### Military Group Interaction

Combat paths and military groups (F-military-groups) serve complementary roles:

- **Military group** determines tactical behavior: `EngagementStyle` (weapon
  preference, ammo exhaustion fallback, initiative, disengage threshold) and
  equipment wants.
- **Combat path** determines skill progression: stat bonuses, XP gain, level.

An elf can be in a military group without a combat path (they fight with
default stats) and can have a combat path without being in a military group
(they have the skill but aren't deployed). Assigning a combat path does
**not** auto-assign to a military group — these are independent systems.

The path system does not override `EngagementStyle`. A Warrior-path elf in a
group with `PreferRanged` weapon preference will still prefer ranged weapons
per group config — the path just makes them better at melee when they do engage
in melee. Path bonuses are passive stat modifiers, not behavioral overrides.

## Path Catalog (Initial Set)

### Combat Paths

| Base Path | Specializations | Notes |
|-----------|----------------|-------|
| **Warrior** | Sentinel (defensive), Blademaster (melee DPS), Champion (leadership aura) | Champion requires civil path experience |
| **Archer** | Sharpshooter (single-target damage), Skirmisher (mobile/kiting) | |
| **Guard** | — | Defensive; no specializations initially. Guard mechanics (e.g., patrol routes, location anchoring) need separate detailed design. |

### Civil Paths

| Base Path | Specializations | Notes |
|-----------|----------------|-------|
| **Cook** | (future: Brewer, Feast-master) | Affects recipe quality and speed |
| **Harvester** | Forager (wild food), Cultivator (farming) | |
| **Artisan** | Carver (wood items), Weaver (textiles), Fletcher (ammo) | Organic specialization via repetition |
| **Woodsinger** | Shaper (construction), Grower (fruit/tree growth) | Higher residue fraction than other craft paths |
| **Poet** | (future: Lorekeep, Cantor) | Passive mana generation bonus, scales with CHA |

## Data Model

### New Tables (SimDb)

```rust
/// Current path assignment for a creature.
/// A creature has at most one active path.
#[derive(Table)]
struct PathAssignment {
    #[primary_key]
    creature_id: CreatureId,
    path_id: PathId,
    tier: PathTier,           // Way, Calling, Attunement
    level: u16,
    xp: u32,
    ticks_on_path: u64,       // for tier promotion thresholds
    assigned_by: PathOrigin,  // Player or Self
}

/// Historical record of past paths walked.
/// Used for residue calculation and prerequisite checking.
#[derive(Table)]
struct PathHistory {
    #[primary_key]
    id: PathHistoryId,
    #[indexed]
    creature_id: CreatureId,
    path_id: PathId,
    max_level_reached: u16,
    max_tier_reached: PathTier,
    total_xp_earned: u32,
}

/// Tracks task-type affinity for unpathed elves (self-assignment trigger).
/// Rows are created lazily when an unpathed elf completes a relevant task.
#[derive(Table)]
struct PathAffinity {
    #[primary_key]
    id: PathAffinityId,
    #[indexed]
    creature_id: CreatureId,
    path_id: PathId,
    task_count: u32,          // completed tasks mapping to this path
    last_task_tick: u64,      // for decay calculation
}

#[derive(Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
enum PathTier {
    Way,
    Calling,
    Attunement,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathOrigin {
    Player,
    Self_,  // organic self-assignment
}
```

### Config (GameConfig)

```rust
/// Added to GameConfig as `path: PathConfig` with #[serde(default)].
struct PathConfig {
    paths: BTreeMap<PathId, PathDef>,
}

struct PathDef {
    name: String,
    category: PathCategory,       // Combat, Civil
    base_path: Option<PathId>,    // None for base paths, Some for specializations
    xp_per_level: Vec<u32>,       // XP thresholds; max level = len()
    calling_threshold_ticks: u64, // min ticks before Way → Calling possible
    calling_threshold_level: u16, // min level before Way → Calling possible
    calling_xp_multiplier_permille: u32, // e.g., 1500 = 1.5×
    attunement_threshold_level: u16,
    attunement_warning_ticks: u64, // how long player has to intervene
    stat_bonuses_per_level: BTreeMap<TraitKind, i32>,
    task_speed_bonus_permille_per_level: i32,  // civil path: craft/task speed
    residue_fraction_permille: u32, // e.g., 200 = 20% retained on leaving
    prerequisites: Vec<PathPrereq>,
    allows_self_assignment: bool, // false for combat, true for civil
    self_assign_task_count: u32,  // tasks needed to trigger self-assignment
    affinity_decay_per_heartbeat: u32, // affinity counter decay rate
    specialization_level: Option<u16>, // level at which specialization unlocks
    allowed_task_kinds: Option<Vec<TaskCategory>>, // if set, restricts task eligibility
}

enum PathPrereq {
    /// Must have reached at least this level on this path (checked via PathHistory)
    PathLevel { path_id: PathId, min_level: u16 },
    /// Must have spent time on any path in this category
    CategoryExperience { category: PathCategory, min_ticks: u64 },
}
```

### SimActions

New variants for `SimAction` (the action enum inside the `SimCommand` struct):

```rust
enum SimAction {
    // ...existing variants...
    AssignPath { creature_id: CreatureId, path_id: PathId },
    Specialize { creature_id: CreatureId, specialization_id: PathId },
    AcceptAttunement { creature_id: CreatureId }, // player acknowledges, allows Attunement
}
```

`AcceptAttunement` explicitly allows the pending Attunement to proceed. Without
it, the elf remains at Calling tier indefinitely after the warning window
expires — Attunement requires either player acceptance or expiry of the warning
window without reassignment (configurable behavior via `attunement_on_expiry`
in config: auto-attune or stay at Calling).

### SimEvents

New variants for `SimEventKind` (the kind enum inside the `SimEvent` struct):

```rust
enum SimEventKind {
    // ...existing variants...
    PathTierReached { creature_id: CreatureId, path_id: PathId, tier: PathTier },
    PathLevelUp { creature_id: CreatureId, path_id: PathId, level: u16 },
    PathSelfAssigned { creature_id: CreatureId, path_id: PathId },
    AttunementWarning { creature_id: CreatureId, path_id: PathId, ticks_remaining: u64 },
    PathAssignmentRefused { creature_id: CreatureId, reason: RefusalReason },
}

enum RefusalReason {
    /// Elf is Attuned and cannot leave their category
    AttunedToCategory { current_category: PathCategory },
    /// Prerequisite not met for requested specialization
    PrerequisiteNotMet { prereq: PathPrereq },
}
```

## Testing Strategy

- **XP gain**: verify XP increments correctly for each action type
- **Level-up**: verify stat bonuses apply at correct thresholds via
  `effective_stat()` pipeline
- **Max level cap**: verify no XP gain past `xp_per_level.len()`
- **Tier transitions**: Way → Calling (time + level threshold), Calling →
  Attunement (with warning window)
- **Specialization prerequisites**: verify prerequisite checks (path level,
  category experience) via PathHistory
- **Attunement lock**: verify Attuned elf cannot be reassigned out of category;
  verify `RefusalReason::AttunedToCategory` event fires
- **Skill residue**: verify fraction of old bonuses retained after path change
  via `effective_stat()` sum
- **Self-assignment**: verify PathAffinity counter increments on task
  completion, triggers self-assignment at threshold, decays over time
- **Task eligibility**: verify specialized elf skips non-matching tasks in
  `find_available_task()`; verify unclaimed task notification
- **Mood penalty**: verify reassignment from Calling tier produces mood thought
- **Military group independence**: verify path assignment doesn't alter military
  group membership or EngagementStyle
- **Serde roundtrip**: PathAssignment, PathHistory, PathAffinity, PathTier,
  PathOrigin, RefusalReason
- **Determinism**: all XP/level/residue calculations use integer math only

## Integration Points

- **Task system**: XP granted on task completion; path level affects task
  speed/quality; specialization filters task eligibility in
  `find_available_task()`
- **Combat**: path stat bonuses flow through `effective_stat()` →
  `apply_stat_multiplier()` pipeline in `stats.rs`
- **Military groups**: independent systems — path gives stats, group gives
  tactics; no auto-assignment between them
- **Mood system**: reassignment mood penalties (ThoughtKind variants), Calling
  comfort bonus
- **Notifications**: tier transitions, Attunement warnings, self-assignments,
  unclaimed specialized tasks
- **UI**: path info in creature panel, assignment controls, specialization
  picker
- **Save/load**: new tables in SimDb, serde for all new types; `PathConfig`
  added to `GameConfig` with `#[serde(default)]` for backward compatibility

## Open Questions

1. **How many levels per path?** Needs playtesting. Initial guess: 10 levels
   for base paths, 5 more for specializations.
2. **Exact XP values per action?** Defer to tuning phase — make everything
   configurable.
3. **Esoteric/magic paths**: Combat and Civil paths are well-defined, but a
   third "Esoteric" category (Ritualist, Seer, etc.) depends on the spell
   system (F-war-magic, F-spell-system) which doesn't exist yet. Esoteric
   paths may need fundamentally different mechanics (cumulative spell learning,
   cross-path knowledge retention). Defer until magic system design is
   underway. The current two-category system (Combat / Civil) is sufficient
   for initial implementation. If/when Esoteric is added as a third category,
   the Attunement category-lock rule may need revisiting.
4. **Multi-path elves**: Can an elf walk two paths simultaneously (e.g., Warrior
   + Poet)? Current design says no — one active path at a time. Revisit if
   gameplay feels too restrictive.
5. **Attunement warning duration**: How long does the player have to intervene?
   Needs to be long enough to notice but short enough to feel urgent.
   Configurable via `attunement_warning_ticks`.
6. **Attunement on warning expiry**: Does the elf auto-attune when the warning
   window expires, or stay at Calling until the player explicitly accepts?
   Both options have merit — auto-attune creates urgency, stay-at-Calling is
   safer. Make configurable.
7. **Guard path mechanics**: The Guard path is listed but "location-bound"
   behavior (patrol routes, area anchoring) needs its own detailed design.
   Defer Guard specialization until the core path system is working.
