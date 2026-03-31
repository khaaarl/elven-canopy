# Creature Genetics Design (F-genetics)

## Overview

Every creature carries an immutable genome — a compact bitfield encoding
genetic predispositions for ability scores, personality, pigmentation, and
morphology. The genome determines initial trait values at spawn, but
expressed traits are stored separately in the trait table and can diverge
over time (injury, aging, dye, magic). When two creatures reproduce, the
offspring's genome is assembled per-bit from both parents with a small
mutation rate, producing natural inheritance patterns without explicit
dominance or linkage modeling.

The design uses an additive SNP-like model: each SNP region is a section
of bits whose values are combined to produce a trait score. For ability
scores, each bit has a distinct large prime weight — the trait's raw
value is the sum of the weights of all 1-bits. Using large primes spread across a bounded range (e.g.,
[1,000,000 .. 5,000,000]) dramatically reduces sum collisions compared
to simple bit-counting, yielding far more distinct trait values.
The distribution is still approximately normal by the central limit
theorem (sum of independent weighted Bernoullis). The different weights
also create natural "major SNP / minor SNP" texture — some bits matter
more than others, mimicking real genetics where certain SNPs have larger
effect sizes. For smaller trait regions (personality, pigmentation),
simple bit-counting suffices since fine granularity is less important.
Categorical traits (hair hue, antler style) use a max-bit-count
selection across competing categories.

## Genome Structure

Two bitfields per creature:

1. **Generic genome** — shared by all species. Contains ability scores
   (8 × 32 bits = 256 bits) and Big Five personality axes
   (5 × 8 bits = 40 bits). Future additions appended here.
   ~296+ bits (~37+ bytes).

2. **Species-specific genome** — layout varies per species. Contains
   pigmentation axes, morphological traits, and species-unique features.
   Typical size 50–200 bits depending on species complexity.

Both are stored as a newtype `Genome { bytes: Vec<u8>, bit_len: u32 }`
with accessor methods that enforce bit-ordering invariants. Bit ordering
is LSB-first within each byte. SNP regions are packed without padding
and may straddle byte boundaries — the `Genome` accessor handles the bit
extraction. Bit-widths per SNP region vary by how much granularity the
trait needs. The `bit_len` field tracks the logical bit count for serde
forward compatibility (distinguishes trailing padding from real data).

**Why not bitvec?** We evaluated the `bitvec` crate (the leading Rust
bit-vector library) and rejected it: the crate is unmaintained (last
commit April 2023, 69 open issues), its serde implementation uses
`std::any::type_name()` which is unstable across compiler versions
(breaking save files on toolchain upgrade), and it has an open soundness
bug (#283). The bit manipulation we need (extract a region, count ones,
weighted sum) is simple enough to implement directly on `Vec<u8>` —
roughly 30–40 lines of utility methods.

### Bit-Width Guidelines

| Trait type               | Typical bits | Rationale                                    |
|--------------------------|--------------|----------------------------------------------|
| Ability score            | 32           | Weighted-sum with prime weights; many distinct values |
| Personality axis         | 8            | Bit-count; ~9 distinguishable levels is plenty |
| Pigmentation axis        | 4–8          | Bit-count; mapped to color gradient            |
| Categorical selector     | 4–8 per opt  | Bit-count competition; 4 bits = 0–4 range      |

### Serde and Forward Compatibility

**Append-only convention:** new SNP regions are ONLY added at the end of
each bitfield. Never reorder or remove existing regions. Each stored
genome includes its bit-length. On deserialization, if the stored length
is shorter than the current layout, trailing bits are filled
deterministically from a SplitMix64 seeded by the creature's ID
(128-bit CreatureId XOR-folded to u64). This means old saves gain
plausible genetics
for newly-added SNP regions without breaking determinism.

### Memory and Save-Size Budget

Generic genome: ~296 bits (~37 bytes). Species-specific genome: 50–200
bits (~7–25 bytes). Total: ~44–62 bytes per creature. With 1,000
creatures, that's ~44–62 KB — negligible compared to the rest of the
sim state. Genomes are stored in saves alongside (not instead of)
expressed traits, since traits can diverge from genetic values. On load,
genomes are read as-is; expressed traits are not re-derived.

**Tabulosity storage:** The existing `creature_traits` table stores
`TraitValue` (Int or Text), which cannot hold raw genome bytes. Genomes
need their own tabulosity table — a `creature_genomes` table keyed by
`creature_id` with columns for `generic_genome: Genome` and
`species_genome: Genome` (each containing `bytes` + `bit_len`). This is
a simple 1:1 table alongside the existing creature row.

Comment convention in code:
```rust
// GENOME LAYOUT (generic) — append only, never reorder!
// [0..32)    STR
// [32..64)   AGI
// [64..96)   DEX
// [96..128)  CON
// [128..160) WIL
// [160..192) INT
// [192..224) PER
// [224..256) CHA
// [256..264) Openness
// [264..272) Conscientiousness
// [272..280) Extraversion
// [280..288) Agreeableness
// [288..296) Neuroticism
// --- add new regions here ---
```

## Trait Expression

### Ability Scores (Weighted-Sum Model)

Each bit in a stat's SNP region has a distinct prime weight chosen from
a bounded range (e.g., 32 primes spread across [1,000,000 .. 5,000,000]).
The raw value is the sum of weights for all 1-bits. Using large primes
in a bounded range dramatically reduces sum collisions compared to simple
bit-counting (which produces only 33 distinct values for 32 bits).
Collisions still exist (2^32 patterns map into a range of ~128M possible
sums), but the number of distinct values is orders of magnitude higher
than bit-counting, giving much finer trait granularity. The ~5× ratio between the smallest and largest weights provides
moderate "major SNP / minor SNP" texture without any single bit
dominating the result.

The weights are defined once as a compile-time constant array. The
theoretical distribution parameters are also precomputed constants:

```
expected_mean = sum(w_i) / 2            // mean of sum of Bernoulli(0.5) * w_i
theoretical_stdev = sqrt(sum(w_i^2)) / 2  // stdev of weighted Bernoulli sum
```

Map to the species stat scale via integer linear scaling:

```
raw = weighted_sum(bits, PRIME_WEIGHTS)  // 0..max_sum
trait_value = (raw - expected_mean) * species_stdev / theoretical_stdev + species_mean
```

All arithmetic uses integer math with symmetric round-to-nearest (not
truncation-toward-zero) to match the existing `quasi_normal` behavior.
The `theoretical_stdev` is stored as a pre-rounded integer constant.
No floating-point anywhere.

This produces values on the existing integer scale where +100 ≈ 2× power
(exponential scaling in stats.rs, range -600..+700, `2^(s/100)`). The
`species_mean` and `species_stdev` values come directly from the existing
`StatDistribution { mean, stdev }` fields in `SpeciesData` — no changes
to species config values are needed.

### Smaller Continuous Traits (Personality, Pigmentation Axes)

For traits with fewer bits (4–8), simple bit-counting (number of 1-bits)
suffices — the granularity is adequate and the simpler model avoids
needing per-width weight tables. An 8-bit region gives 9 distinct values
(0–8), which maps to ~9 distinguishable trait levels. This is coarse
compared to ability scores but appropriate for personality and
pigmentation where broad strokes matter more than fine gradation.
Convert bit-count to the species stat scale:

Use `2*k - n` instead of `k - n/2` to avoid integer division asymmetry
when n is odd (though recommended bit-widths are all even, this is
defensive):

```
k = count_ones(bits)      // 0..n
trait_value = (2*k - n) * species_stdev / (2 * theoretical_stdev) + species_mean
```

The `theoretical_stdev` for B(n, 0.5) is `sqrt(n/4)`. For small n this
is not an integer (e.g., n=8 → stdev = sqrt(2) ≈ 1.414). To avoid
floating-point, precompute a fixed-point scaling factor per bit-width:
store `SCALE / (2 * theoretical_stdev_scaled)` as an integer constant,
where SCALE is a power-of-two precision factor (e.g., 2^20). The trait
expression becomes:

```
trait_value = (2*k - n) * species_stdev * STDEV_SCALE / SCALE + species_mean
```

The `species_stdev` remains a runtime term (varies per species per axis);
only the `STDEV_SCALE` factor is precomputed per bit-width.

This gives exact integer results with no runtime floating-point.
Division uses symmetric round-to-nearest.

### Categorical Traits (Hue, Antler Style, etc.)

Each candidate category gets its own small bit section (4–8 bits).
Each bit within a category has a distinct prime weight (from a
per-bit-width weight table, same principle as ability score weights but
smaller primes suited to 4–8 bit regions). The category's score is the
sum of weights of its 1-bits — this gives far more distinct scores than
simple bit-counting, making ties rare and blend weights smooth. The
category with the highest weighted sum wins. Ties (exact equal sums)
are broken by PRNG (seeded deterministically from creature ID + trait ID).

Example: elf hair hue with 7 categories × 6 bits each = 42 bits total.

```
[gold:6][copper:6][rose:6][violet:6][blue:6][teal:6][green:6]
weighted sums: 1823, 2941, 1205, 2938, 502, 1647, 2103
winner: copper (2941)
```

### Hue Blending for Adjacent Categories

Species define a hue wheel ordering for their color categories. When the
top two candidates are adjacent on the hue wheel, the expressed hue is a
blend of the two rather than a winner-take-all pick. The blend position
is determined by the ratio of their weighted sums — if copper scores 2941
and rose scores 2938, the result is almost exactly halfway between them;
if copper scores 2941 and gold scores 1823, the result leans strongly
toward copper. This produces a continuous spectrum of intermediate colors
through inheritance — e.g., blue parent × teal parent → aqua child, with
the exact shade varying smoothly based on the genome.

When the top two candidates are NOT adjacent on the hue wheel, the
highest-scoring category wins outright (no blending across distant hues).

The blend logic is species-specific and lives in the sprite/color mapping
layer, not in the genome code itself.

## Pigmentation Model

Pigmentation uses a Value-Saturation-Hue model with species-specific
palettes. The genome encodes biological parameters; the sprite renderer
maps them to RGB.

### Axes

| Axis       | Bits | Meaning                                         |
|------------|------|-------------------------------------------------|
| Value      | 4–8  | dark ↔ light                                    |
| Saturation | 4–8  | muted/desaturated ↔ vivid/intense               |
| Hue        | categorical | Which color family (species-specific options) |

**Elf hair:** Many hue options (warm/copper, cool/blue, violet, green,
gold/amber, pink/magenta, neutral/brown). High saturation = anime-vivid;
low saturation = muted/natural. High value + low saturation = silver/ash;
low value + high saturation = deep jewel tones; high value + high
saturation = vivid pastels. Hue blending enabled for adjacent categories.

**Human hair (forward-looking — Human is not yet a sim species):** Fewer
hue options (blonde, brown, black, red). Narrower saturation range —
naturalistic palette.

**Elf/human skin (human forward-looking):** Three axes — melanin
(light↔dark), ruddiness (pale↔rosy), warmth (cool/olive↔warm/golden).
Warmth reserved for future implementation; include bits in genome now.

**Elf/human eyes (human forward-looking):** Value (light↔dark) + hue
(categorical: amber, violet, green, blue, grey, plus species-specific
fantasy options).

**Non-humanoid skin** (goblins, orcs, trolls): Same VSH structure but
species-specific hue categories. Troll: {stone-grey, moss-green,
muddy-brown, pale-blue}. Goblin: {sickly-green, jaundice-yellow,
grey-brown, pale-chartreuse}. Different species, different palettes,
same genome structure.

## Personality (Big Five)

Five axes, 8 bits each in the generic genome:

| Axis              | Low end              | High end             | Gameplay hooks (future)                    |
|-------------------|----------------------|----------------------|--------------------------------------------|
| Openness          | Routine-seeking      | Curious, exploratory | Task variety preference, exploration bias   |
| Conscientiousness | Distractible, lax    | Focused, disciplined | Work speed, task completion, tidiness       |
| Extraversion      | Solitary, reserved   | Social, talkative    | Social need strength, group activity pref   |
| Agreeableness     | Competitive, blunt   | Cooperative, kind    | Conflict avoidance, opinion bias, diplomacy |
| Neuroticism       | Emotionally stable   | Volatile, anxious    | Mood swings, stress response, fear threshold|

Species-specific distributions use the same `StatDistribution { mean, stdev }`
config pattern as ability scores.

Personality values use the same integer scale as ability scores (centered
on 0). Each species has a per-axis mean and stdev, configured via the
same `StatDistribution { mean, stdev }` used for ability scores. The
8-bit count (0–8) is mapped to this scale via the same linear formula
as other small continuous traits.

**Example species personalities (rough sketch):**

| Species | O (mean/sd) | C (mean/sd) | E (mean/sd) | A (mean/sd) | N (mean/sd) |
|---------|-------------|-------------|-------------|-------------|-------------|
| Human   | 0 / 50      | 0 / 50      | 0 / 50      | 0 / 50      | 0 / 50      |
| Elf     | 0 / 60      | +30 / 60    | 0 / 60      | 0 / 60      | +30 / 60    |
| Goblin  | +40 / 50    | -40 / 50    | +10 / 50    | -40 / 50    | +10 / 50    |

(O=Openness, C=Conscientiousness, E=Extraversion, A=Agreeableness,
N=Neuroticism. These are rough starting points — tuning happens during
playtesting. Per-axis stdevs can differ within a species if needed.)

## Inheritance

When two creatures produce offspring:

1. **Per-bit crossover:** For each bit position in both genomes, select
   uniformly at random from parent A or parent B (50/50).

2. **Mutation:** Each bit has a small configurable probability (e.g.,
   1/500) of being flipped regardless of parental value. At 1/500 and
   a 32-bit stat region, this flips ~0.064 bits per stat on average —
   very subtle drift.

3. **Species mismatch:** Cross-species reproduction (if ever supported)
   would need special rules. Out of scope for now; the generic genome
   crosses cleanly, the species-specific genome would need a policy
   (pick one parent's species layout, hybridize, etc.).

No linkage (bits near each other don't co-inherit as blocks), no
dominance/recessiveness — purely additive. These could be added later
if desired.

**Variance note:** Because ability score SNPs have unequal weights,
per-bit crossover can produce higher offspring variance than uniform-weight
models. Two parents with identical stat values but different underlying
bit patterns can produce children with a wider spread of values. This is
a feature, not a bug — it mirrors real genetics where identical phenotypes
can have different genotypes, leading to surprising offspring.

### Inheritance of Categorical Traits

Because each category has its own bit section, crossover naturally
produces interesting results:

- Same-hue parents: offspring very likely gets the same hue (both
  parents contribute high-scoring bits to that category).
- Different-hue parents: offspring is roughly a coin flip between the
  two hues, with occasional third-option surprises when the bit mix
  boosts an unexpected category.
- Adjacent-hue parents with blending: offspring whose top two categories
  are adjacent get a blended intermediate color (blue × teal → aqua),
  with the exact shade depending on the inherited weighted sums.

## Integration with Existing Systems

### Replacing Current Stat Rolling

Currently: `mean + quasi_normal(rng, stdev)` per stat (creature.rs).
After genetics: roll genome bits from PRNG, then derive stats via the
weighted-sum → species scaling formula. The `quasi_normal` call is
replaced entirely. End result is the same distribution shape
(approximately normal by CLT) but now backed by heritable genetic data.

**Non-offspring creatures** (wild spawns, starting elves, summoned
creatures) generate their genome bits independently at random (each bit
50/50) from the sim PRNG, exactly as if they had no parents. Only
offspring produced through reproduction use the crossover + mutation
path.

### Genome-Derived Visual Traits

Pigmentation SNPs drive the color values stored in the trait table,
and the sprite system reads from trait values directly. BioSeed has been
removed — all visual traits are now genome-derived.

### DNA vs Current State

The genome is immutable after creation. Expressed traits are written to the
`creature_traits` table at spawn and can be modified by game events:

- Hair dye changes current hair color (DNA unchanged)
- Antler injury changes current antler style
- Aging might shift skin/hair values
- Magical effects could alter any expressed trait

The creature info panel could eventually show both genetic and current
values, or just current values with a "genetics" tab for breeding-focused
players.

## Config

**SpeciesData** gains new fields alongside the existing `stat_distributions`:

```rust
// On SpeciesData (existing struct in species.rs):
/// Per-personality-axis distribution (mean, stdev), same pattern as
/// stat_distributions. Axes not present default to (0, 50).
pub personality_distributions: BTreeMap<PersonalityAxis, StatDistribution>,
/// Genome-structural config (bit widths, species-specific SNP layout).
pub genome_config: SpeciesGenomeConfig,
```

**SpeciesGenomeConfig** is a new struct for genome-structural config:

```rust
pub struct SpeciesGenomeConfig {
    /// Bit-width for each ability score SNP region (default 32).
    pub stat_bits: u32,
    /// Bit-width for each personality axis (default 8).
    pub personality_bits: u32,
    /// Species-specific genome layout definition.
    /// IMPORTANT: ordering defines physical bit layout. Never reorder entries;
    /// only append new ones at the end.
    pub species_snps: Vec<SnpRegion>,
}

pub struct SnpRegion {
    pub name: String,         // e.g., "hair_value", "hair_hue_warm"
    pub bits: u32,            // bit-width
    pub kind: SnpKind,
}

pub enum SnpKind {
    /// Weighted-sum (ability scores) or bit-count (personality, pigmentation)
    /// scaled to species mean/stdev. The scaling model is determined by
    /// context: ability scores in the generic genome use weighted-sum;
    /// all other continuous SNP regions use bit-count.
    Continuous,
    /// Competes with other entries in the same group via max weighted sum.
    /// For hue categories with a defined wheel ordering, the top two
    /// adjacent candidates blend by their score ratio instead of
    /// winner-take-all.
    Categorical { group: String },
}
```

## Implementation Phases

### Phase A: Genome infrastructure
- Define genome bitfield types (generic + species-specific)
- Implement weighted-sum → species scaling (ability scores)
- Implement bit-count → species scaling (personality, pigmentation)
- Implement categorical max-bit-count selection with PRNG tiebreak
- Serde with append-only forward compatibility
- Unit tests for distribution properties, serde roundtrip

### Phase B: Wire up ability scores
- Replace `quasi_normal` stat rolling with genome-derived stats
- Verify stat distributions match existing species configs
- All existing stat-dependent systems (combat, movement, HP, etc.)
  continue working unchanged

### Phase C: Personality
- Add Big Five to generic genome
- Species personality distributions in config
- Store as traits; no behavioral hooks yet (future work)
- Info panel display

### Phase D: Pigmentation and visual traits
- Species-specific genome layouts for pigmentation (VSH model)
- Elf hair/eye color with full hue palette + blending
- Elf/human skin melanin + ruddiness (warmth bits reserved)
- Wire into sprite generation (genome-derived colors)
- Non-humanoid species pigmentation (goblins, trolls, etc.)
- All 12 species need genome backing for their visual traits: elves
  (HairColor, EyeColor, SkinTone, HairStyle), capybara (BodyColor,
  Accessory), boar (BodyColor, TuskSize), deer (BodyColor, AntlerStyle,
  SpotPattern), monkey (FurColor, FaceMarking), squirrel (FurColor,
  TailType), etc.
  Estimate the total bit budget per species during implementation.

### Phase E: Inheritance
- Per-bit crossover + mutation
- Requires a reproduction system (out of scope for this feature;
  this phase implements the genetic mechanics, not the breeding trigger)
- Blocked by F-creature-sex (creature sex/gender field) and
  F-animal-breeding (breed tamed animals) for actual in-game use
- Tests for inheritance distribution properties

### Phase F: Species morphology
- Categorical traits for species-specific features (antler style,
  tusk size, wing pattern, etc.)
- Wire into sprite generation

## Implementation Notes

Guidance for the implementer — decisions made during design that aren't
obvious from the spec alone.

**Prime weight selection:** Pick the first 32 primes that fall within
[1,000,000 .. 5,000,000]. The specific primes don't matter much — what
matters is that they're all distinct, all within the bounded range, and
all prime (to minimize accidental sum collisions). Compute
`EXPECTED_MEAN` and `THEORETICAL_STDEV` from the chosen set at compile
time. Verify with a unit test that the resulting distribution, when
scaled to a species with mean=0 stdev=50, covers approximately the
range [-250, +250] at the tails (5 sigma).

**Bit-widths should be even.** The `2*k - n` formula is symmetric for
even n. While the formula works for odd n too, prefer even bit-widths
(4, 6, 8, 16, 32) to avoid any asymmetry edge cases.

**Don't use bitvec.** The `bitvec` crate was evaluated and rejected
(see "Why not bitvec?" in the Genome Structure section). Implement the
~30 lines of bit extraction, count_ones, and weighted_sum directly.

**The `species.rs` doc comment on `stat_distributions`** previously said
the default was `(0, 5)` — this was a pre-existing bug, already fixed
to `(0, 50)` in this branch.

**Categorical tiebreak seed:** Use creature ID (XOR-folded to u64) +
trait ID as seed, not "birth seed" (which doesn't exist as a concept
in the codebase).

**`SnpKind::Continuous` scaling model:** The enum doesn't distinguish
weighted-sum from bit-count — the scaling model is determined by context.
Ability score regions in the generic genome use weighted-sum with
`PRIME_WEIGHTS`; all other continuous regions (personality, pigmentation)
use simple bit-count. This is implicit from the genome layout, not
stored in the enum. If this ever becomes ambiguous, split `Continuous`
into `WeightedSum` and `BitCount` variants.

**Phase D vs Phase F boundary:** Phase D covers pigmentation (color
axes — value, saturation, hue). Phase F covers non-color morphology
(antler style, tusk size, wing pattern). Some traits straddle the
boundary (e.g., fur pattern could be either). Use judgment — if it
maps to a color, it's Phase D; if it maps to a shape/structure, it's
Phase F.

## Future Extensions (Out of Scope)

- **Linkage / crossover points:** Block-based crossover instead of
  per-bit, creating linked SNP clusters
- **Dominance / recessiveness:** Some alleles masking others
- **Founding population correlation:** Initial elves share partial
  genome similarity to simulate common ancestry
- **Epigenetics:** Environmental factors affecting trait expression
- **Genetic diseases / rare mutations:** Special phenotypes from
  specific bit patterns
- **Selective breeding UI:** Player tools for planning pairings
