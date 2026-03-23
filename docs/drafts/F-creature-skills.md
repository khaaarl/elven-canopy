# F-creature-skills: Creature Skill System

**Status:** Draft
**Depends on:** F-creature-stats (done)
**Related:** F-path-core, F-apprentice, F-path-residue

## Motivation

Elves currently have innate stats (Strength, Agility, etc.) but no learned
proficiency. An elf who has cooked a thousand meals performs identically to
one picking up a ladle for the first time. This removes a major axis of
differentiation and strategic depth — there's no reason to prefer one elf
over another for a given task, and no sense of growth through practice.

Skills fill the gap between **innate ability** (stats) and **role identity**
(paths). Stats are what you're born with. Paths are what you commit to. Skills
are what you've learned by doing. All three layers contribute to how well an
elf performs a task, but they're mechanically independent: a naturally clumsy
elf (low Dexterity) who practices Archery extensively can compensate, and a
gifted elf who never practices stays mediocre.

## Design Overview

### The Three Layers

```
Stats (innate)    → raw capability, rolled at birth, never changes
Skills (learned)  → proficiency from practice, grows with use
Paths (identity)  → role commitment, gates advanced applications + deeper skill growth
```

All three feed into task performance. The effective capability for any action
is a function of the relevant stat(s) and skill(s), potentially modified by
path bonuses.

### Universal Skills

All 17 skills are universally available to every elf. Any elf can practice
and improve any skill. However, **advancement is gated by paths**:

- **Without a relevant path:** An elf's skill is capped at a configurable
  default ceiling.

- **With a relevant path:** The cap is determined by the path definition.
  Deeper paths generally grant higher caps, but this is configured per
  path, not enforced by the system.

Each path defines its own skill caps. A Warrior path might grant a high
cap for Striking and Evasion but leave Cuisine at the default. A
Woodsinger path might cap Singing and Channeling high but not touch
combat skills. All of this is tuning — the system just enforces whatever
caps are configured.

Some **activities** (not skills themselves) are locked behind paths. For
example, construction woodsinging requires a Woodsinger path — it's not that
the Singing or Channeling skills are gated, but the *application* of those
skills to grow structures requires path training. An elf with high Singing
and high Channeling but no Woodsinger path simply doesn't know how to direct
those abilities into shaping wood.

### Skill List (17 Skills)

#### Combat
1. **Striking** — Melee combat proficiency. Affects attack speed, damage
   delivery, weapon handling. Higher skill means more efficient strikes and
   better exploitation of weapon reach.

2. **Archery** — Ranged combat proficiency. Affects draw speed, accuracy
   (aim deviation), reload timing. Interacts with Dexterity stat for aim
   and Strength for projectile velocity.

3. **Evasion** — Ability to avoid incoming attacks. Affects dodge chance,
   reaction to threats, positioning under pressure. Interacts with Agility
   stat for movement speed during evasion.

#### Outdoor
4. **Ranging** — Stealth, navigation, tracking, spatial awareness,
   outdoorsmanship. The ranger's toolkit: moving unseen through the canopy,
   reading terrain, finding paths, spotting threats before being spotted.
   Combines with Archery for ambush hunting, with Evasion for scouting.

5. **Herbalism** — Covers the full plant lifecycle: foraging wild fruit,
   tending greenhouses, understanding extraction and processing of plant
   materials. Interacts with the cultivation and extraction task systems.

6. **Beastcraft** — Creature interaction, taming, animal care. Universal at
   a basic level (don't spook the deer), path-gated for advanced
   domestication and training. Future feature but the skill slot is reserved.

#### Crafting
7. **Cuisine** — Food preparation. Affects cooking speed, food quality,
   recipe complexity. Higher skill reduces waste and produces meals with
   better mood effects.

8. **Tailoring** — Textile work: weaving, sewing, dyeing cloth. Affects
   quality and speed of cloth production and clothing manufacture.

9. **Woodcraft** — Knowledge of wood, carpentry, non-magical shaping.
   Reading grain, selecting materials, understanding structural properties.
   The foundation for all wood-based crafting, including weapons and armor
   (via path specialization into warcrafter/armorer). Also relevant to
   non-magical construction and furniture.

10. **Alchemy** — Potions, salves, poultices, magical material
    transformation. Elves lean magical rather than medical — healing comes
    through alchemical preparations and enchanted substances rather than
    surgery. Combines with Herbalism for ingredient knowledge and Channeling
    for infusing mana into preparations. Future feature but the skill slot
    is reserved.

#### Mana & Performance
11. **Singing** — Musical performance and vocal mana channeling. Central to
    elven culture — every elf can sing, but skill determines effectiveness.
    Contributes to construction woodsinging (Singing + Channeling + Art,
    gated by Woodsinger path), musical composition (Singing + Literature),
    morale effects, and mana generation.

12. **Channeling** — Mana sensitivity and channeling efficiency. The ability
    to sense, draw, and direct mana. Affects mana pool utilization,
    channeling speed, and magical potency. Contributes to construction
    woodsinging, wizardry (Channeling + Literature), alchemy, and future
    magic systems.

#### Knowledge & Social
13. **Literature** — Written and verbal composition, lore, scholarly
    knowledge. Affects poetry (Literature + Art), lyrics (Singing +
    Literature), wizardry (Channeling + Literature), recipe discovery,
    and lore preservation. An elf with high Literature understands the
    theory behind what others do by instinct.

14. **Art** — Visual creation: sculpture, decoration, embellishment, dyeing,
    aesthetic judgment. Affects the beauty and mood impact of created objects.
    Contributes to construction aesthetics (Singing + Channeling + Art),
    poetry (Literature + Art), and future decoration/sculpture systems.

15. **Influence** — Persuasion, intimidation, negotiation, social
    maneuvering. The ability to change what others do. Affects trade deals,
    morale of nearby elves, conflict resolution. Combines with Singing for
    inspiring songs, with Literature for speeches.

16. **Culture** — Tradition, ceremony, ritual, storytelling, etiquette,
    hospitality. Knowledge of and participation in the fabric of elven
    society. Affects ceremony effectiveness, social cohesion, visitor
    reception. Combines with Singing for traditional songs, with Literature
    for lore transmission.

17. **Counsel** — Teaching, mentoring, guidance, advising. The ability to
    transfer knowledge and develop others. Directly supports the
    apprenticeship system (F-apprentice): an elf with high Counsel teaches
    faster and more effectively. Combines with Influence for leadership,
    with Culture for elder wisdom.

### Combinatorial Applications

Skills combine to enable activities. Some examples of multi-skill
applications, where the effective level is derived from the contributing
skills (e.g., minimum of the contributors, or weighted average — TBD):

| Application | Skills | Path Required? |
|---|---|---|
| Construction woodsinging | Singing + Channeling + Art | Woodsinger path |
| Wizardry / spellcasting | Channeling + Literature | Esoteric path (future) |
| Musical composition | Singing + Literature | Poet path |
| Poetry / lyrics | Literature + Art | Poet path |
| Inspiring speech | Influence + Culture | — |
| Mentoring effectiveness | Counsel + (relevant skill) | — |
| Herbal alchemy | Alchemy + Herbalism | Alchemist path (future) |
| Magical alchemy | Alchemy + Channeling | Alchemist path (future) |
| Ambush hunting | Ranging + Archery | — |
| Scouting / reconnaissance | Ranging + Evasion | — |
| War-crafting (weapons/armor) | Woodcraft (+ Channeling TBD) | Warcrafter path |
| Animal taming | Beastcraft + Influence | Beastmaster path (future) |

The combination formula is an open question (see below). The simplest
approach is "weakest link" — the effective level for a combination is the
minimum of the contributing skills, so a balanced profile outperforms a
lopsided one. Alternatively, a weighted average lets one strong skill
partially compensate for a weak one.

### Exponential Scaling

Skills reuse the same exponential scaling system as creature stats
(see `stats.rs`). The core mechanic:

- Every **+100 skill points doubles** the mechanical effect.
- A skill of 0 provides a 1× baseline multiplier.
- A skill of 100 provides a 2× multiplier.
- A skill of 200 provides a 4× multiplier.
- A skill of 300 provides an 8× multiplier.

This reuses the existing `apply_stat_multiplier()` / `apply_stat_divisor()`
functions and the precomputed 2^20 fixed-point lookup table. No new math
infrastructure needed.

Each path defines its own caps per skill. Deeper paths (higher commitment
tier) generally grant higher caps, but there's no system-level rule
enforcing this — it's all in the path config data. The default cap (for
skills not covered by any active path) and all per-path caps are
configurable in GameConfig. Exact values are entirely tentative and
subject to playtesting.

### Progression: Probabilistic Advancement

Skills do not use XP. Because the skill scale is granular (0–400+), each
skill point is a small increment, making direct probabilistic advancement
natural.

**On each relevant action** (melee hit, arrow fired, recipe completed, etc.),
the sim rolls against an **advancement probability**. If the roll succeeds,
the skill increases by 1. If the elf is already at their cap (determined by
path tier), no roll occurs.

More momentous actions can grant **multiple advancement rolls** or
**larger increments** on success. For example, killing a powerful enemy
might roll Striking advancement several times, or completing a particularly
complex recipe might grant +2 or +3 on success rather than +1. This is
configured per trigger — each advancement trigger specifies both the
probability and the number of rolls or gain amount.

The advancement probability can be modulated by:

- **Current skill level** — Higher skill → lower probability. Early gains
  come quickly; mastery requires many repetitions. The exact curve
  (linear decay, inverse, etc.) is configurable.
- **Path focus** — Being on a relevant path could increase the advancement
  probability, or off-path practice could reduce it, or both. This is the
  primary mechanical interaction between skills and paths — beyond cap
  gating.
- **Apprenticeship** — Working near a skilled elf with high Counsel could
  boost advancement probability (ties into F-apprentice).

This approach is simpler than XP tracking: no XP counter per skill, no XP
curve configuration, no XP-to-level conversion. The advancement probability
table is the only tuning surface. It also produces natural variation —
two elves practicing the same skill won't advance in lockstep, creating
organic differentiation.

**No skill decay.** Skills, once gained, do not decrease. The path cap
system already prevents universal mastery — an elf without a relevant path
can't exceed their cap regardless of how much they practice. This
keeps things simple and avoids the "punished for playing normally" problem
of decay systems.

### Effect Channels

Skills affect gameplay through these channels (initially implement Speed
and Quality; add others as the systems that consume them mature):

1. **Speed** — Higher skill → faster task completion. Applied as a divisor
   to task duration: `effective_ticks = apply_stat_divisor(base_ticks, skill)`.
   A skill of 100 halves the time. Universal, always relevant.

2. **Quality** — Higher skill → better output. Affects food mood bonus,
   equipment durability, crafted item properties. Implemented as quality
   tiers with skill thresholds, or as a continuous multiplier on output
   properties.

3. **Efficiency** — Higher skill → less waste. Reduced mana cost for
   woodsinging, reduced material waste in crafting, reduced arrow
   consumption (fewer misses). Deferred to later implementation.

4. **Unlocks** — Certain recipes or techniques available only above skill
   thresholds. E.g., complex meals require high Cuisine, advanced
   construction requires high Singing. Deferred to later implementation.

5. **Failure rate** — Low skill → chance of wasting materials or producing
   nothing. Punishing but realistic. Deferred; consider carefully whether
   this is fun before implementing.

### Interaction with Stats

Skills and stats are independent axes that both contribute to task
performance. The general pattern:

```
effective_capability = f(stat, skill)
task_result = apply_capability(base_value, effective_capability)
```

The simplest combination: **additive**. `effective_capability = stat + skill`,
then feed into the existing exponential lookup. This means an elf with
Strength 50 and Striking 150 has effective capability 200 (4× multiplier)
for melee damage. This is clean, deterministic, and reuses existing
infrastructure.

Alternative: **multiplicative** (apply stat multiplier, then apply skill
multiplier independently). This gives a wider dynamic range — Strength 200 +
Striking 200 would be 4× × 4× = 16× rather than additive's 4× from
combined 400. The choice affects balance significantly and should be
playtested. Additive is simpler to reason about.

### Interaction with Paths

Paths interact with skills in three ways:

1. **Cap gating** — The active path determines the maximum skill level,
   configured per path per skill. This is the primary mechanical
   interaction.

2. **Advancement focusing** — Being on a relevant path could increase the
   advancement probability for associated skills, or off-path practice
   could reduce it. The path doesn't change *what* triggers advancement
   rolls, just the probability of success.

3. **Application gating** — Some multi-skill applications require a specific
   path regardless of skill level. You can have Singing 400 and Channeling
   400, but without the Woodsinger path you can't do construction
   woodsinging. The path provides the *knowledge* of how to combine skills,
   not the skills themselves.

**Dual progression on same triggers:** F-elf-paths proposes XP-based path
leveling, while this doc proposes probabilistic skill advancement. Both
trigger on the same actions (melee hits, recipe completions, etc.). When
both systems are active, each action fires both a path XP grant and a skill
advancement roll — these are independent systems that happen to share
triggers. Path XP determines path level (which gates tier promotion and
skill caps). Skill advancement determines the skill value (which affects
task performance). They don't interfere mechanically.

Note: The existing path design (F-elf-paths.md) describes stat bonuses per
path level. With the skill system, these per-level bonuses could be
reframed as skill cap increases or advancement probability bonuses rather
than raw stat additions, or they could remain as stat bonuses alongside
independent skill progression. This is a design decision that should be
resolved when implementing F-path-core alongside F-creature-skills.

## Data Model

### Skills as Traits

Skills are stored in the existing `creature_traits` table as `TraitKind`
variants with `TraitValue::Int` values. No new table is needed. This mirrors
how creature stats (Strength, Agility, etc.) are already stored and queried
via `trait_int()`.

New `TraitKind` variants:

```rust
pub enum TraitKind {
    // ...existing stat variants (Strength, Agility, etc.)...
    // ...existing visual trait variants (HairColor, etc.)...

    // -- Skills (F-creature-skills) --
    // Integer scale starting at 0. Every +100 doubles mechanical intensity
    // via the same exponential stat multiplier table used by creature stats.
    // Advancement is probabilistic: each relevant action rolls against a
    // configurable probability; success increments the skill by 1.
    // Capped by path tier (see SkillConfig).

    /// Melee combat proficiency.
    Striking,
    /// Ranged combat proficiency.
    Archery,
    /// Dodge and threat avoidance.
    Evasion,
    /// Stealth, navigation, tracking, awareness.
    Ranging,
    /// Plant lifecycle: foraging, cultivation, extraction.
    Herbalism,
    /// Creature interaction and taming.
    Beastcraft,
    /// Food preparation.
    Cuisine,
    /// Textile work: weaving, sewing, dyeing.
    Tailoring,
    /// Wood knowledge, carpentry, non-magical shaping.
    Woodcraft,
    /// Potions, salves, magical material transformation.
    Alchemy,
    /// Musical performance and vocal mana channeling.
    Singing,
    /// Mana sensitivity and channeling efficiency.
    Channeling,
    /// Written/verbal composition, lore, scholarship.
    Literature,
    /// Visual creation: sculpture, decoration, aesthetics.
    Art,
    /// Persuasion, intimidation, social maneuvering.
    Influence,
    /// Tradition, ceremony, ritual, storytelling.
    Culture,
    /// Teaching, mentoring, guidance.
    Counsel,
}
```

Querying a skill value uses the existing `trait_int()` function. A creature
with no row for a given skill trait has an implicit value of 0 (the baseline).

### Config (GameConfig)

```rust
/// Added to GameConfig as `skills: SkillConfig` with #[serde(default)].
struct SkillConfig {
    /// Default skill cap. Applies when no path is active, or when the
    /// active path doesn't specify a cap for a given skill. Default: 100.
    default_skill_cap: i64,

    /// Per-skill advancement probability configuration.
    /// Maps (action trigger) → (skill, base probability permille).
    advancement_triggers: BTreeMap<AdvancementTrigger, Vec<SkillAdvancement>>,
}

struct SkillAdvancement {
    skill: TraitKind,  // must be a skill variant
    /// Base probability of advancement on this trigger (permille).
    /// Modulated by current skill level and path focus.
    base_probability_permille: u32,
    /// Number of independent advancement rolls per trigger. Default 1.
    /// Momentous actions (e.g., killing a powerful enemy) can grant multiple.
    rolls: u32,
    /// Skill points gained per successful roll. Default 1.
    /// Complex or difficult actions can grant larger increments.
    gain: i64,
}

/// What triggers a skill advancement roll.
enum AdvancementTrigger {
    MeleeHitLanded,
    ArrowFired,
    // DodgeSuccessful,  // future, once dodge mechanic exists
    CraftCompleted { verb: RecipeVerb },
    HarvestCompleted,
    BuildProgressMade,
    GrowActionCompleted,
    // etc. — extensible as new task types are added
}

/// Extension to PathDef (in PathConfig, from F-path-core):
struct PathDef {
    // ...existing fields from F-elf-paths.md...

    /// Skill caps granted by this path. Only skills with non-default
    /// caps need to be listed — unlisted skills use default_skill_cap.
    /// This means adding new skills or forgetting one in a path
    /// definition is safe; they just get the default cap.
    skill_caps: BTreeMap<TraitKind, i64>,

    /// Skills that receive full advancement probability on this path.
    /// Other skills may receive reduced probability (configurable).
    focused_skills: Vec<TraitKind>,
}
```

### Standalone Behavior (Without F-path-core)

F-creature-skills can be implemented before F-path-core. Without the path
system:

- All skills use the `default_skill_cap` — no path-tier caps apply.
- All advancement rolls use the base probability — no path focus modifier.
- Application gating (path-locked activities) is not enforced.

This allows skill progression and its gameplay effects (speed, quality) to
be tested independently before the path system adds cap differentiation.

### Effective Skill Query

```rust
impl SimState {
    /// Returns the effective skill level for a creature, clamped to their
    /// current path cap (or default cap if no path system exists yet).
    fn effective_skill(&self, creature_id: CreatureId, skill: TraitKind) -> i64 {
        let raw_level = self.trait_int(creature_id, skill, 0);
        let cap = self.skill_cap(creature_id, skill);
        raw_level.min(cap)
    }

    /// Returns the skill cap for a creature based on their current path.
    /// Falls back to default cap if no path is active or the path doesn't
    /// specify a cap for this skill.
    fn skill_cap(&self, creature_id: CreatureId, skill: TraitKind) -> i64 {
        // Once F-path-core exists: look up PathAssignment, then
        // PathDef.skill_caps.get(&skill), falling back to default.
        self.config.skills.default_skill_cap
    }
}
```

### Integration with Existing Stat Pipeline

Skills feed into the same exponential multiplier system as stats:

```rust
// Example: melee damage with both stat and skill
let stat_value = sim.trait_int(creature_id, TraitKind::Strength, 0);
let skill_value = sim.effective_skill(creature_id, TraitKind::Striking);
let effective = stat_value + skill_value;  // additive combination
let damage = apply_stat_multiplier(base_damage, effective);

// Example: task speed with skill only (Craft task with Bake verb)
let skill_value = sim.effective_skill(creature_id, TraitKind::Cuisine);
let cook_ticks = apply_stat_divisor(base_cook_ticks, skill_value);
```

### Advancement Roll

```rust
impl SimState {
    /// Called after a relevant action. Rolls for skill advancement.
    /// Uses the same insert_trait / modify_unchecked pattern as creature
    /// stat storage in creature.rs.
    fn try_advance_skill(
        &mut self,
        creature_id: CreatureId,
        skill: TraitKind,
        base_probability_permille: u32,
        gain: i64,  // typically 1; momentous actions may grant more
    ) {
        let current = self.trait_int(creature_id, skill, 0);

        let cap = self.skill_cap(creature_id, skill);
        if current >= cap {
            return; // already at cap, no roll
        }

        // Modulate probability by current level (higher → harder).
        // Example formula (illustrative, not final):
        //   adjusted = base_prob * 100 / (100 + current)
        // At skill   0: 100% of base probability
        // At skill  50: 67% of base probability
        // At skill 100: 50% of base probability
        // At skill 200: 33% of base probability
        // At skill 400: 20% of base probability
        let adjusted_prob = base_probability_permille as u64 * 100
            / (100 + current.max(0) as u64);

        let roll = self.rng.next_u64() % 1000;
        if roll < adjusted_prob {
            let new_value = (current + gain).min(cap);
            if self.db.creature_traits.get(&(creature_id, skill)).is_some() {
                let _ = self.db.creature_traits.modify_unchecked(
                    &(creature_id, skill),
                    |row| row.value = TraitValue::Int(new_value),
                );
            } else {
                self.insert_trait(creature_id, skill, TraitValue::Int(new_value));
            }
        }
    }
}
```

## Advancement Triggers

Each action type maps to one or more skills for advancement rolls.
Configured in GameConfig. Complete mapping of existing task/action types:

Complete mapping of all current `RecipeVerb` variants (Assemble, Extract,
Mill, Spin, Twist, Bake, Weave, Sew, Grow, Press) and other action types:

| Action / Task | Skills | Notes |
|---|---|---|
| Melee hit landed | Striking | Per-hit, not per-task |
| Arrow fired | Archery | Per-shot |
| Harvest task completed | Herbalism | One task per fruit voxel |
| Craft (Extract) | Herbalism | Breaking down fruit |
| Craft (Mill) | Herbalism | Grinding plant matter |
| Craft (Press) | Herbalism | Pressing fruit into dye |
| Craft (Spin) | Tailoring | Fiber → thread |
| Craft (Twist) | Tailoring | Fiber → cord |
| Craft (Weave) | Tailoring | Thread → cloth |
| Craft (Sew) | Tailoring | Cloth → garment |
| Craft (Bake) | Cuisine | Cooking |
| Craft (Assemble) | Woodcraft | General assembly (e.g., bowstrings) |
| Craft (Grow) | Woodcraft | Growing equipment from wood |
| Build progress (non-magical) | Woodcraft | Blueprint voxel placement |
| Grow action (construction) | Singing, Channeling | Woodsinging construction |

Future triggers (not in initial implementation):

| Action / Task | Skills | Notes |
|---|---|---|
| GoTo (in wilderness) | Ranging | Only in untamed areas; requires area tagging |

Tasks that don't produce skill advancement: EatBread, EatFruit, Sleep,
Mope, AcquireItem, AcquireMilitaryEquipment, Haul. Furnish is excluded
because it's simple item placement, not skilled work (may revisit if
furnishing gains quality mechanics). AttackTarget and AttackMove trigger
advancement via their sub-actions (melee hits, arrows fired), not on task
completion.

Note: If future recipe verbs are added (e.g., a dedicated dye-application
verb), the trigger mapping should be extended accordingly.

## Testing Strategy

- **Advancement**: verify skill increments on relevant actions; verify
  probability modulation by current level; verify cap enforcement
- **Cap without path**: verify default skill cap applies to all elves
  when path system is absent
- **Exponential scaling**: verify skill integrates correctly with existing
  stat multiplier pipeline (additive combination)
- **Speed effect**: verify higher skill reduces task completion time via
  `apply_stat_divisor()`
- **Combinatorial applications**: verify multi-skill effective level
  calculation (min or weighted average)
- **Application gating**: verify path-locked activities reject capable but
  pathless elves (once paths exist)
- **Serde roundtrip**: new TraitKind variants must round-trip through
  save/load — add serde tests matching sibling variants. Adding new
  `TraitKind` variants is backward-compatible: old saves simply won't
  contain skill trait rows, and `trait_int()` returns the default (0)
  for missing traits. `SkillConfig` in `GameConfig` uses
  `#[serde(default)]` so old config files load without error.
- **Determinism**: all skill math uses integer arithmetic via existing
  fixed-point pipeline; advancement rolls use the shared PRNG
- **No decay**: verify skill levels are stable across ticks with no
  practice

## Open Questions

1. **Stat + skill combination formula.** Additive (stat + skill fed into one
   exponential lookup) or multiplicative (separate lookups, multiply results)?
   Additive is simpler and recommended as the starting point; multiplicative
   can be explored if the dynamic range feels too compressed.

2. **Multi-skill combination formula.** For combinatorial applications (e.g.,
   construction woodsinging = Singing + Channeling + Art), how are the
   contributing skills combined? Options:
   - **Minimum** ("weakest link") — effective level = min(contributors).
     Rewards balanced development.
   - **Weighted average** — allows one strong skill to partially compensate.
     More forgiving but harder to reason about.
   - **Sum / N** — simple average. Similar to weighted but uniform.
   The minimum approach is simplest and creates the clearest incentive to
   develop all contributing skills.

3. **Advancement probability modulation.** The pseudocode uses an
   illustrative formula `base * 100 / (100 + level)` which gives:
   skill 0 → 100% of base, skill 50 → 67%, skill 100 → 50%,
   skill 200 → 33%, skill 400 → 20%. This may be too gentle at high
   levels or too aggressive at low levels — the exact curve needs
   playtesting. Alternatives include steeper falloff
   (`base * 50 / (50 + level)`) or step functions with fixed
   probabilities per skill range.

4. **Path focus effect on advancement.** Does being on a relevant path
   increase probability, does being off-path decrease it, or both?
   Simplest: off-path has a flat multiplier (e.g., 0.25×) on advancement
   probability.

5. **Initial skill values.** Do elves start with all skills at 0, or do they
   have some initial spread based on stats or background? Starting at 0 is
   simplest. A stat-derived initial value (e.g., starting Striking =
   Strength / 10) would create more varied starting populations but adds
   complexity.

6. **Skill interaction with F-path-core stat bonuses.** The existing path
   design gives flat stat bonuses per level (e.g., Warrior gets +Strength
   / +Constitution per level). With skills, these could be reframed as
   skill cap increases or advancement probability bonuses rather than raw
   stat additions. Or both systems could coexist — paths give stat bonuses
   AND skill cap increases. Needs design alignment with F-path-core before
   implementation.

7. **Non-elf creatures.** Hostile creatures (trolls, orcs) could have fixed
   skill levels set in `SpeciesData` (loaded from JSON config) as initial
   trait values rolled at spawn, without advancement rolls. Animals likely
   don't use the skill system at all. The trait-based storage is
   creature-generic, but advancement triggers would be elf-only.

8. **UI display.** How are skills surfaced to the player? A skill panel per
   elf, aggregated village skill overview, skill-based task assignment
   recommendations? Deferred to F-path-ui or a dedicated F-skills-ui item.
