# F-item-quality: Item Quality System

**Status:** Draft
**Depends on:** F-creature-skills (done), F-manufacturing (done)
**Related:** F-path-core, F-creature-skills, F-choir-build, F-sung-furniture

## Overview

Crafted items have an integer quality from -1 to +3. The existing
`quality: i32` field on `ItemStack` is used. Items stack only when quality
matches exactly (same as durability). Raw/extracted materials (fruit, fiber,
pulp) have no meaningful quality — only items produced by a skilled crafting
step get a quality roll.

## Tiers

| Quality | Label | Source | Available now? |
|---------|-------|--------|----------------|
| -1 | Crude | Low skill; hostile creature default | Yes |
| 0 | Fine | Normal crafting baseline | Yes |
| +1 | Superior | High stat+skill roll | Yes |
| +2 | Masterwork | Path + rare materials + special recipe | No (future) |
| +3 | Legendary | Deepest path + rarest materials + extreme cost | No (future) |

+2 and +3 are locked behind systems that don't exist yet (paths, advanced
recipes, rare materials). Even a max-skill elf on the deepest path is capped
at Superior (+1) through normal crafting. The ceiling is intentional —
Masterwork and Legendary items should be the stuff of story, not routine
output.

## Determination

At craft completion, roll a quasi-normal random variable (sum of several
integer ranges, stddev ~50, mean 0) and add the crafter's relevant stats
and skill. The combination is additive: `DEX + INT + PER + skill` for
most crafting verbs, `CHA + INT + PER + Singing` for construction. Stats
have mean 0 so an average elf gets no statistical bonus from them; skill
does the heavy lifting.

Stat+skill pairings per action:

| Action | Stats | Skill |
|--------|-------|-------|
| Craft (Extract/Mill/Press) | DEX + INT + PER | Herbalism |
| Craft (Spin/Twist/Weave/Sew) | DEX + INT + PER | Tailoring |
| Craft (Bake) | DEX + INT + PER | Cuisine |
| Craft (Assemble) | DEX + INT + PER | Woodcraft |
| Craft (Grow) | DEX + INT + PER | Woodcraft |
| Construction (Build/Carve) | CHA + INT + PER | Singing |

Note: stats and skills operate at different scales (stat +100 is
exceptional, skill 0–100 is the normal range), but adding three stats
together produces a range roughly comparable to one skill value. Stats
have species-specific means (elves are slightly positive on average)
but the contribution is modest — skill does the heavy lifting.

Compare the roll result against thresholds:

| Roll result | Quality |
|-------------|---------|
| < 50 | Crude (-1) |
| 50–249 | Fine (0) |
| 250+ | Superior (+1) |

Approximate outcomes at different combined levels (stats + skill +
bell curve with stddev ~50):

| Combined | Example | Crude | Fine | Superior |
|----------|---------|-------|------|----------|
| 50 | Novice elf, avg stats | ~50% | ~50% | ~0% |
| 100 | Skill ~75, decent stats | ~16% | ~84% | ~0% |
| 150 | Skill-capped, avg stats | ~3% | ~95% | ~2% |
| 200 | Skill-capped, great stats | ~0% | ~84% | ~16% |
| 250 | Pathed elf (future) | ~0% | ~50% | ~50% |
| 300 | Deep path + great stats (future) | ~0% | ~16% | ~84% |

Combined values above ~150 require either exceptional stats or path-
raised skill caps (future). Superior is genuinely rare for unpathed
elves but becomes achievable once paths raise the skill cap. Future
+2/+3 tiers would use higher thresholds on the same roll.

The thresholds (50, 250), stddev (~50), and propagation scores can be
hardcoded initially and promoted to a `QualityConfig` struct later if
tuning demands it. Uses the shared PRNG for determinism.

### Pseudocode

```
fn determine_quality(sim, creature_id, skill, stats) -> i32 {
    let bell = quasi_normal(&mut sim.rng, 50);
    let skill_val = sim.trait_int(creature_id, skill, 0);
    let stat_total = stats.iter().map(|s| sim.trait_int(creature_id, s, 0)).sum();
    let mut roll = bell + stat_total + skill_val;

    // Input quality drag-down (if recipe has inputs).
    if let Some(avg_input_score) = avg_input_quality_score(inputs) {
        if avg_input_score < roll {
            roll = (roll + avg_input_score) / 2;
        }
    }

    match roll {
        ..50 => -1,     // Crude
        50..250 => 0,   // Fine
        250.. => 1,     // Superior (capped at +1 for now)
    }
}
```

## Quality Propagation

Input quality drags down output quality but cannot boost it. Each quality
tier maps to a representative score on the roll scale: Crude=0, Fine=150,
Superior=300. (These don't align exactly with threshold midpoints — they're
tuning values that control how strongly inputs drag down the roll.)
Average the input quality scores. If the average is less than the crafter's
roll, reduce the roll to the midpoint of the roll and the average input
score. If the average is >= the roll, no adjustment — good materials
don't compensate for a bad roll.

Example: all-crude cloth inputs (score 0), crafter rolls 200. Adjusted
roll = (200 + 0) / 2 = 100 → Fine. The crude materials dragged a
potentially-superior result down to fine.

Example: all-superior cloth inputs (score 300), crafter rolls 100. No
adjustment — roll stays 100 → Fine. Good materials can't carry a
mediocre crafter.

Initial implementation may skip propagation (use crafter skill alone) and
add the input quality drag-down later. The item schema already carries
quality on intermediate goods (thread, cloth, cord) so the data is there.

## Code Cleanup

- **`RecipeOutput.quality`:** Unused scaffolding (all recipes default to 0).
  Remove — the quality roll replaces it entirely. Quality is always
  determined by the crafter's skill and stats at craft time, never
  hardcoded per recipe.
- **Subcomponent quality:** `crafting.rs` currently hardcodes subcomponent
  quality to 0 in `record_subcomponents`. Update to propagate the parent
  item's rolled quality to its subcomponent records.

## Starting and Hostile Equipment

Elves' starting gear (clothes, bread, bows, arrows) is Crude (-1). This
creates an early progression arc — replacing your crude starting tunic
with a freshly crafted fine one feels like a tangible step up. Most
barbaric hostiles (orcs, goblins, trolls) default to Crude equipment,
but this is a species-level default, not a hard rule — individual
raiders or more advanced attackers could carry anything.

## Display

All items display their quality label as a prefix: "Crude Oak Bow", "Fine
Bread", "Superior Tunic". Every tier is visible — seeing "Fine" on your
newly crafted tunic feels like a step up from the "Crude" one you started
with. The label is derived at display time from the `quality: i32` field,
not stored as a string.

## Effects (Tentative)

Each quality tier applies a simple modifier. Direction, not final numbers:

- **Food:** Quality affects mood bonus. Crude food gives reduced (or no)
  mood boost; Superior food gives extra.
- **Equipment:** Quality could affect durability max HP, armor value, or
  weapon damage. +1 per quality tier on the relevant property.
- **Construction/furniture:** Quality could affect room mood bonus. Deferred
  until room mood exists.

## What Doesn't Get Quality

Fruit, extracted parts (fiber, pulp, juice), and other raw materials.
Quality enters at the crafting step where skill determines outcome.
Thread, cloth, and other intermediate crafted goods *do* get quality
since they involve skilled work.

## Backward Compatibility

Existing saves have `quality: 0` on all items (the serde default). These
are Fine under the new tier system — no migration needed.

## Testing Strategy

- Verify the quasi-normal distribution has the expected stddev (~50) over
  many samples.
- Verify threshold boundaries: roll exactly 49 → Crude, 50 → Fine,
  249 → Fine, 250 → Superior.
- Verify quality propagation drag-down: crude inputs reduce a high roll,
  superior inputs don't boost a low roll.
- Verify starting elf gear spawns at quality -1.
- Verify display names include the correct quality prefix.
- Verify items with different quality values don't stack together.
- Statistical test: at combined stat+skill 100, crude rate should be
  ~16% ± margin over many trials.
