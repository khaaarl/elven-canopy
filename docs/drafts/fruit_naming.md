# Fruit Naming Overhaul v4

**Tracker:** F-fruit-naming
**Status:** Draft v4 (revised from v3 critique)

## Problem

The current fruit naming system (`fruit.rs` lines 992-1059) picks 1-2 Vaelith morphemes based on a fruit's most notable property and shape. This produces frequent collisions because:

- The **gloss-matching funnel is too narrow**, not the pool size. The lexicon has 48 botanical entries, which partition into ~40 property/color/descriptive roots and ~8 shape roots (pod, berry, nut, gourd, blossom, cluster, husk, fruit). But the current code maps each `PartProperty` to 1-3 English glosses and does exact string matching against the lexicon — many botanical entries have glosses that no property maps to (e.g., "vine," "thorn," "root" are unreachable).
- The "most notable property" selection is deterministic and tier-based — all luminescent fruits compete for "glow"/"light" roots.
- Collision resolution appends numbers ("Vela0", "Vela1"), which is ugly and uninformative.

With 20-40+ fruit species per world, collisions are common. Names don't convey meaningful information about the fruit, and numbered suffixes break immersion.

## Goals

- **Zero repeats.** Every fruit species gets a unique name with no number suffixes.
- **Meaningful names.** Names should tell you something — the reddest fruit might be "Bloodberry," a luminescent gourd might be "Stargourd," an unremarkable pod might be "Elfandriel's Pod."
- **Varied naming strategies.** Not every fruit is named by its properties. Some are named after (fictional) historical figures, implying worldgen history.
- **Natural feel.** The mix of property-names and cultural-names should feel like a real botanical tradition, not a procedural generator.
- **Per-world variation.** The naming shouldn't be deterministic from properties alone — the same fruit in two different worlds might get different names based on what other fruits exist and PRNG rolls.

## Design

### Affinity data: static Rust table (no lexicon JSON changes)

The affinity data lives in `fruit.rs` as a static table mapping root glosses to `(trait, weight)` pairs. This avoids expanding the lexicon JSON schema with fruit-specific concerns — the lexicon format stays as-is, and crates that parse it (music, lang) are unaffected.

```rust
/// Affinity between a botanical root (by gloss) and fruit traits.
/// Weights are u8; higher = stronger association.
struct RootAffinity {
    gloss: &'static str,
    affinities: &'static [(AffinityTrait, u8)],
}

/// Trait dimensions for root-fruit affinity scoring.
///
/// Property variants wrap PartProperty directly for compile-time sync.
/// Pigment variants cover the 5 primary DyeColor values that appear on
/// fruit parts; Orange/Green/Violet are mixing-only secondaries and are
/// excluded here.
enum AffinityTrait {
    // Properties — wraps PartProperty for compile-time sync.
    // If a PartProperty variant is added or renamed, this breaks at compile
    // time, preventing silent desync.
    Property(PartProperty),
    // Pigment colors (5 primaries only — Orange/Green/Violet are
    // mixing-only secondaries that never appear on fruit parts)
    Pigment(DyeColor),
    // Shapes
    Shape(FruitShape),
    // Habitat/character (for roots like "deep," "wild," "ancient")
    Habitat(HabitatTrait),
}

/// Small enum for habitat/character affinities.
enum HabitatTrait {
    HighCanopy,
    LowTrunk,
    Wild,
    Ancient,
}
```

Example entries:

```rust
const ROOT_AFFINITIES: &[RootAffinity] = &[
    RootAffinity { gloss: "blood", affinities: &[
        (AffinityTrait::Pigment(DyeColor::Red), 8),
        (AffinityTrait::Property(PartProperty::Medicinal), 4),
    ]},
    RootAffinity { gloss: "fire", affinities: &[
        (AffinityTrait::Pigment(DyeColor::Red), 5),
        (AffinityTrait::Property(PartProperty::Stimulant), 4),
        (AffinityTrait::Property(PartProperty::Luminescent), 3),
    ]},
    RootAffinity { gloss: "star", affinities: &[
        (AffinityTrait::Property(PartProperty::Luminescent), 8),
        (AffinityTrait::Habitat(HabitatTrait::HighCanopy), 3),
    ]},
    RootAffinity { gloss: "berry", affinities: &[
        (AffinityTrait::Shape(FruitShape::Clustered), 6),
        (AffinityTrait::Shape(FruitShape::Round), 3),
    ]},
    // ... one entry per botanical root in the lexicon
];
```

This table will be ~100-200 lines of static data. Roots with no `RootAffinity` entry (i.e., glosses missing from the table) get implicit zero affinity across all traits — they are still available for world-naming but never chosen by the property-scoring path. No explicit fallback entry is needed.

**Root classification:** a root is classified as a **shape root** if its highest-weight affinity is a `Shape(*)` variant; otherwise it is a **property root**. This is determined once at startup by inspecting each root's `RootAffinity` entry.

**Shape root / FruitShape alignment:** Shape roots in the affinity table do not correspond 1:1 to `FruitShape` variants. A single shape root can have affinities to multiple `FruitShape` variants (e.g., "berry" maps to `Shape(Clustered)` with weight 6 and `Shape(Round)` with weight 3). Conversely, roots with shape-like English meanings (e.g., "blossom", "fruit") may have weak or zero shape affinities — in that case, the classification rule (highest-weight affinity determines category) treats them as property roots despite their English gloss suggesting a shape. The `FruitShape` enum has 6 variants (`Round`, `Oblong`, `Clustered`, `Pod`, `Nut`, `Gourd`); the lexicon has ~8 shape-adjacent glosses. The many-to-many mapping between glosses and enum variants is handled naturally by the affinity weights.

At startup, the naming algorithm loads the botanical pool from the lexicon (filtered by `NameTag::Botanical`) and joins it with the affinity table by gloss.

The 48 existing botanical roots partition into ~40 property/color/descriptive roots and ~8 shape roots. With ~40 property roots and ~8 shape roots forming property+shape pairs, the combinatorial space is ~320 pairs. This exceeds the 40-fruit maximum, but with a thinner margin than a naive 48-choose-2 calculation would suggest — the roots are not interchangeable across categories. The margin is sufficient for typical worlds (20-40 fruits), and fruits that can't get a unique property+shape pair fall through to world-naming, so the algorithm degrades gracefully rather than failing.

### Property intensity via yield_percent

`FruitPart.properties` is a `BTreeSet<PartProperty>` (boolean per-part) and `pigment` is `Option<DyeColor>`. However, intensity CAN be derived from `yield_percent: u8` on each part — the percentage of the fruit's mass that part represents.

**Intensity formula:** For a given property P across the entire fruit:

```
intensity(fruit, P) = sum of yield_percent for all parts that have property P
```

For pigment colors:

```
intensity(fruit, color) = sum of yield_percent for all parts where pigment == Some(color)
```

Examples:
- A fruit with Flesh (yield 60%, Sweet) and Rind (yield 25%, Sweet) has Sweet intensity = 85.
- A fruit with Sap (yield 5%, Luminescent) has Luminescent intensity = 5.
- A fruit with Flesh (yield 50%, pigment Red) and Rind (yield 30%, pigment Red) has PigmentRed intensity = 80.

This gives a natural 0-100 scale where bulky, dominant parts contribute more than trace components. The fruit whose red pigment comes from 80% of its mass is "bloodier" than one where only a 5% sap layer is red.

**Note:** F-fruit-yields will reexamine the yield model in the future. For now, `yield_percent` is the intensity signal.

For shape and habitat traits, intensity is binary (1 or 0) — a fruit either has shape Round or it doesn't.

### Multi-pass temperature-weighted assignment

The naming algorithm runs after all fruit species are generated, operating on the full set at once. It assigns roots to fruits in multiple passes using integer-weighted random selection.

#### Phase 1: Score all (fruit, root) pairs

For each fruit and each root in the botanical pool, compute an **affinity score**:

```
score(fruit, root) = sum over root's affinities:
    if fruit has trait T: affinity_weight(root, T) * intensity(fruit, T)
```

All values are integers. `affinity_weight` comes from the static table (`u8`, max 255). `intensity` comes from the `yield_percent` formula above (0-100 scale for properties/pigments, 0 or 1 for shapes/habitats). The product of a single `(weight, intensity)` pair fits in `u32` (max 255 * 100 = 25,500). The sum across all affinities for one root also fits in `u32` (a root with 5 affinities at max values: 5 * 25,500 = 127,500). Scores are stored as `u32`.

Shape affinities use a bonus for matching shape but with lower weights than property affinities, since many fruits share shapes.

#### Phase 2: Iterative root assignment (fruit-first)

Hardcoded 10 passes, all executed (no early stopping — 10 passes over 40 fruits and 48 roots is trivially cheap):

1. **For each fruit that is not yet name-ready** (in `FruitSpeciesId` order, deterministic via `BTreeMap`):
   - Compute temperature-scaled weights for each root. Temperature controls how peaked the distribution is:
     - **Integer-only temperature sampling:** raise each score to an integer power equal to the temperature. Weights are computed as `u64` to avoid overflow: `weight(root) = (score(fruit, root) as u64).pow(temperature)` where temperature is a `u32`. With max score ~4,000 and max temperature 4, the max exponentiated value is 4,000^4 = 256,000,000,000,000, which fits comfortably in `u64` (max ~1.8 * 10^19).
     - Temperature=1 means linear (proportional to raw score). Temperature=2 means quadratic (strongly favors high scorers). Temperature=3 means cubic (very peaked).
     - **Property roots** use temperature = `naming_temperature * 2` (more peaked, more distinctive assignments).
     - **Shape roots** use temperature = `naming_temperature` (base temperature, flatter distribution, more sharing).
   - **Sample one root** using integer-weighted selection: compute cumulative `u64` weight sums, draw via `GameRng::range_u64(0, total_weight)`, find the root whose cumulative range contains the draw. This avoids modulo bias (unlike `next_u64() % total_weight`), matching codebase conventions — `range_u64` uses rejection sampling internally. If all weights are zero for this fruit, skip it this pass.
   - The sampled root is added to this fruit's assigned root set. A root can be assigned to multiple fruits across passes (like "berry" appearing in multiple fruit names), but the *combination* of roots must be unique per fruit. Shape roots are freely shared; property roots are shared less often due to the higher temperature.

2. **After each pass**, check each fruit:
   - Tally assigned roots. Categorize as **property roots** or **shape roots** per the classification rule above.
   - A fruit is **name-ready** if it has at least 1 property root and 1 shape root (or at least 2 property roots), AND its root combination is unique among all name-ready fruits.
   - **Note on root-set uniqueness:** this per-pass check is an optimization that avoids wasted assignment passes for fruits that already have a viable root set. It does not guarantee final name uniqueness — two different root sets could theoretically produce identical composed strings. String-level uniqueness is enforced separately in Phase 4.

3. **After all 10 passes:** any fruit that still lacks a unique root combination is assigned to the world-naming path. This guarantees termination and zero collisions regardless of PRNG rolls or fruit count.

**Bland fruits:** Fruits with the `Bland` property and no other distinctive traits score near-zero on all property roots — their affinity weights are negligible across the board. These fruits naturally fall through to world-naming after 10 passes. This is intentional: unremarkable fruits get named after discoverers, which is thematically appropriate. Real-world botany works the same way — the most boring plants are the ones named after people.

#### Phase 3: Name composition

**For name-ready fruits (property-derived names):**
- Order: property root(s) first, shape root last.
- Concatenate Vaelith root texts directly (same approach as `names.rs` — raw concatenation, no epenthesis or consonant assimilation). Root-root compounding has no existing phonotactic joining rules in the lang crate, and adding them is out of scope for this feature.
- Capitalize the first letter.
- Display name appends the English shape noun: Vaelith compound = "Tharvelë", display = "Tharvelë Fruit".
- English gloss built from root glosses: "blood-light fruit."

**For fruits with 2+ property roots but no shape root:** the display name uses `FruitShape::item_noun()` (the existing method returning "Fruit", "Pod", "Nut", "Cluster", or "Gourd") as the English suffix. The Vaelith compound is the concatenated property roots with no shape root appended. Example: a luminescent, medicinal fruit with shape `Round` and property roots "thár" (blood) + "léshi" (light) produces display name "Thárléshi Fruit" (blood-light fruit), where "Fruit" comes from `FruitShape::Round.item_noun()`. This follows the same display pattern as shape-rooted names; the only difference is the English noun comes from the shape enum directly instead of a shape root's gloss.

**For remaining fruits (world-named):**
- Generate a Vaelith proper name using a **single name part** from the existing `names.rs` generator, then combine with a shape root in genitive form. `generate_name_part` is currently private (`fn` in `names.rs`); it will be made `pub` with an expanded return type (see below). Only a single name part is used (not a full given+surname), since fruit names should be shorter than elf names.
- **`generate_name_part` signature change:** modify the return type from `(String, String)` to `(String, String, VowelClass)` — returning the name text, the English gloss, and the vowel class of the last root used. This is a minimal change: the function already has access to the last `LexEntry` chosen, which carries a `vowel_class` field. The function is made `pub` at the same time. The existing call site in `generate_name` destructures the third element with `_` so no other code changes are needed.
- The possessive uses Vaelith **genitive case** from `phonotactics.rs`: suffix `-li` (front vowel class) or `-lu` (back vowel class). The vowel class comes from the third element of the `generate_name_part` return tuple.
- Example: a front-class name "Thira" becomes "Thirali Pod" (star-GEN pod). The English display form is "Thirali Pod" (not "Thira's Pod") — the genitive suffix IS the possessive marker in Vaelith.
- Lean toward shape roots for the noun portion, since these fruits lack distinctive property roots.
- The person names are fictional — they don't correspond to actual in-game elves or locations. They imply a history that exists in the world's background.

**Place-style names are out of scope.** The lexicon has place-related glosses (forest, canopy, garden, meadow, etc.) but none are tagged `botanical`, so they are not in the botanical root pool. Adding place-style naming would require either expanding the botanical tag to non-botanical roots or building a separate generation path from the `given`/`surname` pools. This adds complexity for marginal benefit — person-style genitive names already provide sufficient variety for world-named fruits. Place-style naming could be revisited if the lexicon grows.

**World-naming path inherits float math from `names.rs`.** The `generate_name_part` function uses `rng.next_f64()` for the 70/30 root count roll. This is acceptable — `GameRng::next_f64()` is deterministic from seed, and the world-naming path runs during worldgen where reproducibility (same seed = same output) is the requirement. The scoring and sampling phases (Phases 1-2) use integer-only math for scoring and sampling; the world-naming path (Phase 3) uses existing `names.rs` which includes deterministic float calls.

#### Phase 4: Uniqueness enforcement

After all names are composed:
- Check for **string-level collisions** on the final composed display names. Phase 2's root-set uniqueness check is an optimization (avoiding wasted passes), but two different root sets could theoretically produce identical composed strings — this phase catches that case.
- For any collision, the lower-priority fruit gets reassigned to world-naming. Priority is defined as: **sum of affinity scores across all assigned roots** for that fruit. Lower total score = lower priority = reassigned. This is concrete and deterministic.
- If a world-name collides (extremely unlikely with generated proper names), regenerate the proper name with a different PRNG draw.
- **No number suffixes ever.** The combined space of property-named compounds (~320 property+shape pairs) plus world-named fruits (effectively infinite proper name space from the name generator) guarantees enough entropy for any realistic fruit count.

### Integration with existing systems

**`generate_fruit_species()` changes:** Currently names each fruit inline during generation. The new algorithm runs *after* all species are generated, as a separate `assign_fruit_names()` pass in worldgen. This is necessary because naming is relative to the cohort — you need to know all fruits before deciding which one gets "blood."

**`FruitSpecies` struct:** `vaelith_name` and `english_name` fields remain, just populated by the new algorithm instead of the old one.

**Encyclopedia / display:** No changes needed — they already read `vaelith_name` and `english_name` from the species.

**Save/load compatibility:** Names are persisted in save files on `FruitSpecies` and never regenerated at load time. Existing saves keep their old names; only new worlds use the new algorithm.

**Determinism:** All scoring and sampling uses `GameRng` with integer arithmetic (`u32` scores, `u64` exponentiated weights, `range_u64` for selection). Root iteration uses deterministic `BTreeMap` order (by `FruitSpeciesId`). Fruit iteration is also `BTreeMap`-ordered. The world-naming path uses `names.rs` which includes deterministic float calls via `GameRng::next_f64()`.

### Configuration

Single new field in `FruitConfig`:

```rust
/// Base temperature for root assignment (integer power applied to scores).
/// Property roots use 2x this value; shape roots use 1x.
/// Higher = more peaked distribution (best-match fruit strongly favored).
/// Lower = flatter distribution (more randomness in assignment).
/// Default: 2 (property roots use temp=4, shape roots use temp=2).
pub naming_temperature: u32,
```

Pass count is hardcoded at 10 (sufficient for up to 40 fruits; not worth exposing as config). Assignment probability is removed — the temperature-weighted sampling already controls selectivity.

## Scope

### In scope
- Static affinity table in `fruit.rs` mapping botanical root glosses to trait weights (~100-200 lines)
- `AffinityTrait` as a wrapper enum: `Property(PartProperty)`, `Pigment(DyeColor)`, `Shape(FruitShape)`, `Habitat(HabitatTrait)`
- `RootAffinity` with `u8` weights (sufficient range for the 1-8 scale used in practice; enforces the max=255 assumption at the type level)
- Multi-pass fruit-first temperature-weighted root assignment algorithm (integer scoring, `u64` exponentiated weights, `range_u64` sampling)
- Property intensity derived from `yield_percent` sums
- World-naming fallback with Vaelith genitive case for possessives (person-style only)
- Changing `generate_name_part` in `names.rs` to `pub` with return type `(String, String, VowelClass)` — name text, English gloss, and last root's vowel class
- Uniqueness enforcement with zero number suffixes (root-set uniqueness in Phase 2 as optimization, string-level uniqueness in Phase 4 as guarantee)
- Post-generation naming pass in worldgen
- Single config parameter (`naming_temperature`)

### Out of scope
- Expanding the Vaelith lexicon JSON format (affinities live in Rust, not JSON)
- New lexicon entries (the existing 48 botanical roots are sufficient with affinity-based matching)
- Place-style names (insufficient lexicon support — no place-related glosses tagged `botanical`)
- Vaelith grammar beyond root concatenation and genitive case
- Root-root phonotactic joining rules (uses raw concatenation like `names.rs`)
- Player-visible naming history ("this fruit was named after Elfandriel because...")
- Player renaming of fruits
- Dynamic re-naming when new fruits are discovered mid-game
- Reworking yield_percent values (deferred to F-fruit-yields)

## Test plan

1. **Zero collisions** — generate 100 worlds with max fruit count, verify no two species share a name in any world.
2. **Determinism** — same seed produces identical names across runs.
3. **Variety across seeds** — different seeds produce meaningfully different name distributions (not all worlds have the same "most red fruit = bloodberry" pattern).
4. **World-name generation** — verify world-named fruits use correct Vaelith genitive case (`-li`/`-lu` suffix matching last root's vowel class).
5. **Property-intensity scoring** — a fruit with Red pigment on 80% yield_percent parts should outscore one with Red on 5% yield_percent parts for "blood"/"fire" roots.
6. **Shape root sharing** — multiple fruits can share a shape root; verify this happens naturally.
7. **Guaranteed termination** — verify the algorithm completes within 10 passes for all tested seeds, with remaining fruits falling through to world-naming.
8. **Serde roundtrip** — `FruitSpecies` with new name fields roundtrips correctly. Lexicon JSON is unchanged and still loads without modification.
9. **Consonant cluster check** — verify no generated name contains 4 or more consecutive consonants. Define a programmatic check: scan the composed name for runs of non-vowel characters (where vowels = `{a, e, i, o, u, á, é, í, ó, ú, à, è, ì, ò, ù, â, ê, î, ô, û, ä, ë, ï, ö, ü, ě, ǎ}` and their uppercase equivalents); assert zero occurrences of 4+ consonant runs across all names in 100 generated worlds. The expectation is that Vaelith roots are short (1-2 syllables each) and always contain vowels, so concatenating 2-3 roots should never produce 4+ consonant runs. If this test ever fails, the fix is to add an epenthetic vowel at the join point (future work, not in scope for this feature).
10. **Bland fruit fallthrough** — verify that fruits whose only property is `Bland` consistently fall through to world-naming (their affinity scores should be near-zero for all property roots).
11. **Edge cases** — test with `min_species_per_world = 1` (trivial, always property-named), `max_species_per_world` at high values (more fruits than distinct root combinations forces world-naming fallback), and worlds where many fruits share properties (e.g., all luminescent).
12. **Existing test updates** — the existing naming tests in `fruit.rs` (lines 1270+) must be updated or replaced to match the new algorithm's output format.
13. **Overflow safety** — verify that the maximum possible exponentiated weight (`4000^4 = 2.56 * 10^14`) fits in `u64` and that cumulative sums across 48 roots (`48 * 2.56 * 10^14 = 1.23 * 10^16`) also fit in `u64`. Note: the max score per root is now bounded more tightly because affinity weights are `u8` (max 255) rather than unbounded `u32`, giving a per-root max of 5 * 255 * 100 = 127,500 and max exponentiated value of 127,500^4 = 2.64 * 10^20 — this does NOT fit in `u64`. In practice, real weights are 1-8 and real scores peak around ~4,000, so 4,000^4 = 2.56 * 10^14 is the realistic max. The test should verify with both realistic and theoretical maximums and confirm the temperature cap prevents overflow (e.g., temperature must be capped at 4 for the realistic score range, or at 3 for worst-case `u8` weights).
