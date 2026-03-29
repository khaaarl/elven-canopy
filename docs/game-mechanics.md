# Game Mechanics: Skill Checks and Ability Scores

This document defines how ability scores and skills combine with randomness to
resolve contested and uncontested actions in the simulation. The unified
`skill_check()` helper lives in `sim/skills.rs`; see also `stats.rs` for the
exponential multiplier table and `config.rs` for tuning parameters.

> **Note on specificity:** The core *formula* (how stats, skills, and noise
> combine) is a settled architectural decision. The specific *assignments* —
> which stats and skills are used for which checks, speed pairings, and
> advancement triggers — are provisional first passes that haven't been
> playtested. Expect these to change as the game matures. The tables below
> reflect what the code currently does, not necessarily what it should do long
> term.

## Core Formula

Every skill check in the game follows the same shape:

```
roll = sum(relevant_ability_scores) + skill + quasi_normal(rng, 50)
```

- **Ability scores** are the creature's innate stats (STR, AGI, DEX, CON, WIL,
  INT, PER, CHA). They are rolled at creature spawn and never change. Centered
  at 0 for humans/elves; every +100 doubles mechanical intensity via an
  exponential multiplier table (`2^(stat/100)`).
- **Skill** is a single learned proficiency (Striking, Archery, Herbalism,
  etc.). Skills start at 0 and advance probabilistically through practice.
- **quasi_normal(rng, 50)** is a bell-curve integer roll with mean 0 and
  standard deviation 50, implemented via the Central Limit Theorem (sum of
  uniform samples, scaled).

The number of ability scores summed varies by check (1–3), while every check
uses exactly one skill. The ability scores are additive, not averaged — this is
safe because they are centered at 0.

## Contested vs. Uncontested Checks

**Uncontested checks** compare the roll against a fixed threshold:

```
success = roll >= threshold
```

Examples: taming (vs. species difficulty), crafting quality (vs. tier
thresholds).

**Contested checks** compare the attacker's roll against the defender's passive
total. Only the attacker rolls; the defender does not. This is analogous to
Armor Class in D&D — the defender's stats set a bar, but luck is one-sided.

```
attacker_roll = attack_skill + stat(s) + quasi_normal(rng, 50)
defender_total = defense_skill + stat(s)
hit = attacker_roll >= defender_total
```

Note: armor in this game does *not* affect hit chance. It provides direct damage
reduction after a hit lands (see "Armor" section below).

## Current Skill Checks

> The stat/skill assignments below are what's currently implemented. They are
> reasonable first guesses but haven't been tuned or validated through gameplay.

### Melee Accuracy (Contested)

| Side     | Stats | Skill    |
|----------|-------|----------|
| Attacker | DEX   | Striking |
| Defender | AGI   | Evasion  |

A **critical hit** occurs when the attacker's roll exceeds the defender's total
by `evasion_crit_threshold` (default 100, ~2 standard deviations — roughly 2.3%
chance with equal stats).

Results: `CriticalHit | Hit | Miss`.

### Ranged Accuracy (Contested)

Ranged combat has two layers of miss chance:

**Layer 1 — Trajectory deviation** (physical accuracy): When an arrow is
launched, `apply_dex_deviation()` in `stats.rs` adds random lateral offsets to
the projectile's velocity vector, scaled inversely by the shooter's DEX via the
exponential stat table. Higher DEX reduces deviation asymptotically (every +100
DEX halves the deviation). Only the X and Z axes are perturbed; Y (vertical) is
left to gravity. This determines whether the arrow physically reaches the
target.

**Layer 2 — Evasion check** (dodge): If the arrow reaches the target, a
contested skill check determines whether the target dodges:

| Side     | Stats | Skill   |
|----------|-------|---------|
| Attacker | DEX   | Archery |
| Defender | AGI   | Evasion |

Same `roll_hit_check` function as melee, same crit threshold. If the shooter is
unknown (e.g., a trap or test fixture), the attacker roll falls back to a bare
`quasi_normal(rng, 50)` with no stat or skill contribution.

### Taming (Uncontested)

| Stats      | Skill     | Threshold             |
|------------|-----------|-----------------------|
| WIL + CHA  | Beastcraft| Species tame_difficulty|

Pass/fail. Current species difficulties: Capybara 100, Squirrel 100, Deer 150,
Monkey 150, Boar 200, Elephant 250, Hornet 350, Wyvern 350. Sapient species
(Elf, Goblin, Orc, Troll) cannot be tamed.

### Crafting Quality (Uncontested)

| Stats             | Skill              | Thresholds              |
|-------------------|--------------------|-------------------------|
| DEX + INT + PER   | Verb-specific skill| <50 Crude, 50–249 Fine, 250+ Superior |

Current verb-to-skill mapping (tentative):
- Extract / Mill / Press → Herbalism
- Spin / Twist / Weave / Sew → Tailoring
- Bake → Cuisine
- Assemble / Grow → Woodcraft

The choice of DEX + INT + PER for all crafting verbs is a placeholder — it's
plausible that different verbs should weight different stats (e.g., Bake might
care more about PER than DEX). These will likely be revisited.

**Input quality propagation:** If the average quality score of input items is
lower than the crafter's raw roll, the final roll is pulled toward the midpoint:
`(raw_roll + avg_input_score) / 2`. Inputs can drag quality down but never boost
it. Quality scores: Crude = 0, Fine = 150, Superior = 300.

### Social Impression (Uncontested)

| Stats | Skill                       | Result Buckets                                      |
|-------|-----------------------------|-----------------------------------------------------|
| CHA   | (see below)                 | ≥51: +2, 1–50: +1, -49–0: 0, ≤-50: -1              |

The roll is mapped to a small integer delta and applied to one of four opinion
kinds (Friendliness, Respect, Fear, Attraction). Currently only Friendliness is
actively used in bootstrap and casual social interactions. The bucket boundaries
above are arbitrary placeholders — they were chosen to be simple round numbers
around the stdev, not because they produce good gameplay.

The skill selection is tentative and uses a `SkillPicker` enum:
- **Casual interactions / bootstrap:** max(Influence, Culture) — the creature
  plays to their social strength.
- **Formal interactions (dances):** Culture only.

This split between Influence and Culture is a rough first pass. The distinction
between "social maneuvering" (Influence) and "tradition/ceremony" (Culture) felt
right thematically, but the exact boundaries — and whether both skills are even
needed — remain open questions.

**Advancement direction:** The skill check determines how impressive the
*target* creature is. But skill advancement (Influence or Culture) is rolled for
the *observer* (subject), not the target — social skills improve through
exposure, not through being impressive.

## Armor (Not a Skill Check)

Armor does not affect hit chance — it reduces damage after a hit:

```
effective_damage = max(raw_damage - total_armor, armor_min_damage)
```

Config defaults: `armor_min_damage` = 1 (damage always gets through).

**Armor degradation:** On a **penetrating hit** (raw_damage > total_armor), the
degradation amount is rolled uniformly in `[0, 2 * penetrating_damage]`, where
`penetrating_damage = max(raw_damage - total_armor, armor_min_damage) -
armor_min_damage`. If the penetrating damage works out to 0 (armor barely
fails), no degradation occurs. On a **non-penetrating hit**, there is a 1-in-N
chance (default N = `armor_non_penetrating_degrade_chance_recip` = 20) of losing
1 HP. Degradation targets a weighted-random body slot (default weights: Torso 5,
Legs 4, Head 3, Feet 2, Hands 1).

Worn items (`armor_worn_penalty`, default 1) and damaged items
(`armor_damaged_penalty`, default 2) have reduced effective armor values, floored
at 2 and 1 respectively.

## Construction (Not a Skill Check)

Construction (building/carving voxels) does not use `skill_check` — there is no
success/failure roll. Instead, construction uses skills for **speed** and
**advancement**:

- **Speed:** CHA + Singing via `skill_modified_duration`. (The thematic idea is
  that elves sing to the tree to shape it, so vocal talent and charisma
  determine how quickly the tree responds. Whether CHA is the right stat here
  is debatable.)
- **Advancement:** Three skills advance per construction action (per voxel
  placed/carved), not at project completion:
  - Singing: 1000 permille (100% base)
  - Channeling: 1000 permille (100% base)
  - Woodcraft: 500 permille (50% base)

## Skill Advancement

Skill advancement is **separate from skill checks** and uses its own probability
system. The helper `try_advance_skill()` in `skills.rs` handles the roll. In the
future, a higher-level helper that combines the skill check with advancement
(and possibly speed calculation) may be worth building to reduce the boilerplate
at each call site. For now, callers wire these up individually. When a
creature performs a relevant action, it calls this helper with a per-action base
probability:

```
adjusted = base_probability * decay_base / (decay_base + current_skill)
adjusted = adjusted * 2^(INT/100)
adjusted = min(adjusted, 1000)  // cap at 100%
```

- `decay_base` (default 100) controls the learning curve — higher skill means
  slower advancement.
- INT scaling doubles learning speed per +100 INT.
- Path bonuses (Warrior, Scout) grant an extra advancement roll for associated
  skills, and raise the skill cap from 100 to 200.

### Advancement Triggers

> The base probabilities and trigger points below are initial values. They
> haven't been balanced against each other or tested for feel.

| Action | Skill(s) | Base Prob (‰) | Trigger Point |
|--------|----------|---------------|---------------|
| Melee strike | Striking | 500 | Every strike attempt |
| Melee dodge | Evasion | 500 (config) | On successful dodge |
| Ranged attack | Archery | 500 | Every shot attempt |
| Ranged dodge | Evasion | 500 (config) | On successful evasion |
| Construction | Singing | 1000 | Per action tick |
| Construction | Channeling | 1000 | Per action tick |
| Construction | Woodcraft | 500 | Per action tick |
| Grow (crafting) | Woodcraft | 1000 | Per action tick |
| Grow (crafting) | Singing | 500 | Per action tick |
| Grow (crafting) | Channeling | 500 | Per action tick |
| Recipe completion | Verb-specific | 800 | On completion |
| Harvesting | Herbalism | 800 | On completion |
| Taming attempt | Beastcraft | 50 (config) | Every attempt |
| Social interaction | Influence or Culture | 50 (config) | Per interaction |

Notes:
- **Grow-verb recipes** advance skills per action tick (during the mana drain
  loop), not at completion — they share the construction pattern because they
  involve singing to the tree. Non-Grow recipes advance once at completion.
- **Evasion** advances from both melee and ranged dodges (same config field:
  `evasion_dodge_advance_permille`).
- **Taming** and **social** have much lower base probabilities (50‰ = 5%) than
  combat and crafting, reflecting their lower-intensity interactions.
- **Social advancement** is on the *observer* (the creature forming an opinion),
  not the target making the impression.
- **Unwired skills:** 5 of the 17 skill TraitKinds — Ranging, Alchemy,
  Literature, Art, and Counsel — have no advancement triggers yet. They exist
  in the type system but aren't connected to any actions.

## Task Speed

Skills also reduce task duration via additive combination with stats:

```
effective_ticks = apply_stat_divisor(base_ticks, stat + skill)
```

This uses the same exponential table: combined stat+skill of +100 halves the
duration, +200 quarters it, etc. The skill cap limits *learning*, not the speed
benefit — a creature with skill 200 at cap 100 still gets full speed benefit.
Result is floored at 1 tick.

### Speed Pairings

> These pairings are first guesses — they feel thematically reasonable but
> haven't been evaluated for balance. Some may change as gameplay develops.

| Activity | Stat | Skill |
|----------|------|-------|
| Melee strike interval | AGI | Striking |
| Ranged shoot interval | DEX | Archery |
| Construction (build/carve) | CHA | Singing |
| Furnishing | DEX | Woodcraft |
| Crafting (all verbs) | DEX | Verb-specific skill |
| Harvesting | DEX | Herbalism |

Grow-verb crafting speed applies per action tick (during the mana drain loop),
not once per recipe.
