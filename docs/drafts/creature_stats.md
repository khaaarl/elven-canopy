# Creature Stats Design (F-creature-stats)

## Overview

Eight per-creature stats with exponential scaling, rolled at spawn from
species-specific distributions. All integer math — no floating point anywhere.
Builds on the `creature_traits` table from F-creature-biology.

## The Eight Stats

| Stat           | Abbr | Primary mechanical hooks                                 |
|----------------|------|----------------------------------------------------------|
| Strength       | STR  | Melee damage multiplier, projectile velocity              |
| Agility        | AGI  | Move speed, climb speed (blended with STR)                |
| Dexterity      | DEX  | Arrow deviation, crafting quality (future)                 |
| Constitution   | CON  | HP max multiplier                                          |
| Willpower      | WIL  | Mana pool, mana recharge rate (future)                     |
| Intelligence   | INT  | Mana recharge rate, crafting quality (future)              |
| Perception     | PER  | Hostile detection range (F-per-detection), crafting (future) |
| Charisma       | CHA  | Singing effectiveness, mana recharge rate (future)         |

Mental stats (WIL, INT, CHA) are rolled and stored at spawn but have no
mechanical effect until their systems are implemented. PER has an immediate
hook via F-per-detection (hostile detection range multiplier). WIL and INT's
first mechanical hooks (mana pool size and regen rate) are designed in
F-elf-mana-pool; spell effectiveness scaling via INT is designed in
F-war-magic (see `docs/drafts/war_magic.md`).

## Scale

Integer, centered on 0 (human baseline). Higher is better for all stats.

Each species has a **mean** and **standard deviation** per stat, configured in
`SpeciesData`. Example ranges:

| Species   | STR  | AGI  | DEX  | CON  | WIL | INT  | PER | CHA |
|-----------|------|------|------|------|-----|------|-----|-----|
| Human     |  0   |  0   |  0   |  0   |  0  |  0   |  0  |  0  |
| Elf       | -3   | +5   | +5   | -5   | +5  | +3   | +3  | +3  |
| Goblin    | -5   | +5   | +3   | -3   | -3  | -5   | +5  | -5  |
| Orc       | +10  | -3   | -5   | +10  | +3  | -8   | 0   | -5  |
| Troll     | +20  | -10  | -5   | +30  | +5  | -15  | -5  | -10 |
| Capybara  | -10  | -5   | -10  | +5   | +10 | -10  | +5  | +15 |
| Boar      | +5   | +3   | -5   | +10  | -3  | -10  | +3  | -10 |
| Deer      | -5   | +15  | +3   | -3   | 0   | -5   | +10 | 0   |
| Elephant  | +25  | -15  | -10  | +35  | +5  | +5   | +3  | +5  |
| Monkey    | -5   | +10  | +10  | -5   | 0   | +5   | +5  | +5  |
| Squirrel  | -15  | +15  | +10  | -10  | 0   | +3   | +10 | +3  |

Standard deviations might be 3-5 for most stats (species-specific, also in
`SpeciesData`). These are placeholder values — tuning happens during
playtesting.

## Exponential Scaling

**Every +10 in a stat doubles its mechanical intensity.** This is the core
design invariant.

A troll at STR +30 hits 8x as hard as a human at STR 0. A goblin at STR -10
hits 0.5x as hard. The relationship is `2^(stat/10)`.

### Integer implementation: flat precomputed lookup table

A compile-time `const` array of 131 `i64` entries maps stat values from -60 to
+70 to multipliers in 2^20 (1,048,576) fixed-point. This gives ~6 decimal
digits of precision with headroom for very negative stats.

Values at multiples of 10 are exact powers of 2:

```
stat  | multiplier (x 2^20) | effective ratio
------|--------------------|----------------
 -60  |          16384     | 1/64  (0.015625x)
 -50  |          32768     | 1/32  (0.03125x)
 -40  |          65536     | 1/16  (0.0625x)
 -30  |         131072     | 1/8   (0.125x)
 -20  |         262144     | 1/4   (0.25x)
 -10  |         524288     | 1/2   (0.5x)
   0  |        1048576     | 1x    (human baseline)
  +10 |        2097152     | 2x
  +20 |        4194304     | 4x
  +30 |        8388608     | 8x
  +40 |       16777216     | 16x
  +50 |       33554432     | 32x
  +60 |       67108864     | 64x
  +70 |      134217728     | 128x
```

Between multiples of 10, entries are linearly interpolated within each octave.
The generation formula uses Euclidean division (floor toward negative infinity)
to handle negative stats correctly:

```rust
// Generate the table at compile time (build script or const fn).
fn generate_table() -> [i64; 131] {
    let mut table = [0i64; 131];
    for i in 0..131 {
        let s = i as i64 - 60;  // stat value: -60 to +70
        let decade = s.div_euclid(10);
        let frac = s.rem_euclid(10);   // always 0..9
        let lo = if decade >= 0 {
            (1i64 << 20) << decade
        } else {
            (1i64 << 20) >> (-decade)
        };
        let hi = lo * 2;
        table[i] = lo + (hi - lo) * frac / 10;
    }
    table
}
```

**Critical:** Rust's `/` and `%` operators truncate toward zero, which gives
wrong results for negative stats (e.g., `-3 / 10 = 0` in Rust but should be
`-1` for this formula). Always use `div_euclid` and `rem_euclid`.

The linear interpolation introduces up to ~6% error at octave midpoints
compared to the exact `2^(s/10)` curve. This is acceptable — the stat system
is tunable via species distributions and the error is consistent across
platforms. If higher accuracy is ever needed, the table entries can be replaced
with precomputed exact values from a build script.

Values outside the -60 to +70 range clamp to the endpoints.

### Applying the multiplier

```rust
const STAT_TABLE: [i64; 131] = generate_stat_table();

fn stat_multiplier(stat: i64) -> i64 {
    let idx = (stat + 60).clamp(0, 130) as usize;
    STAT_TABLE[idx]
}

fn apply_stat_multiplier(base: i64, stat: i64) -> i64 {
    (base * stat_multiplier(stat)) >> 20
}
```

All arithmetic is i64. No floating point. One array index + one multiply +
one shift per lookup.

## Species Distribution at Spawn

Stats are rolled from an integer approximation of a normal distribution.
Technique: sum of 12 uniform samples centered on 0, divided by 2.

```
// Sum 12 uniform samples in [-stdev, +stdev].
// The sum has standard deviation = 2 * stdev (by CLT).
// Dividing by 2 yields the target standard deviation.
// rng.range_inclusive(-stdev, stdev) — a signed-integer range method
// that will be added to the PRNG crate for this feature.
let sum: i64 = (0..12).map(|_| rng.range_inclusive(-stdev, stdev)).sum();
let stat = species_mean + sum / 2;
```

The factor of 2 comes from: each uniform sample in `[-stdev, +stdev]` has
variance `(2*stdev)^2 / 12 = stdev^2 / 3`. The sum of 12 such samples has
variance `12 * stdev^2 / 3 = 4 * stdev^2`, so standard deviation `2 * stdev`.
Dividing by 2 normalizes to the target stdev. All integer arithmetic.

Each stat requires 12 PRNG calls (96 per creature for 8 stats). Stat rolls
happen **after** the existing BioSeed + visual trait rolling in
`roll_creature_traits` (F-creature-biology), in `TraitKind` enum order, to
preserve determinism for existing creatures.

## Mechanical Hooks (Initial)

### Strength -> Melee Damage

```
effective_damage = apply_stat_multiplier(base_melee_damage, strength)
```

A troll (STR +30, base_melee_damage 25) does `25 * 8388608 >> 20 = 200`.
An elf (STR -3, base_melee_damage 10) does `10 * 891289 >> 20 = 8`.

### Strength -> Projectile Velocity

```
effective_velocity = apply_stat_multiplier(base_arrow_velocity, strength)
```

Stronger creatures launch arrows faster -> flatter trajectory, more kinetic
energy on impact. Damage-on-impact is a future system; for now, velocity just
affects range and trajectory shape.

### Agility -> Move Speed

Higher agility = fewer ticks per voxel. The multiplier is inverted:

```
effective_ticks = base_ticks * (1 << 20) / stat_multiplier(agility)
```

AGI +10 halves the tick cost (twice as fast). AGI -10 doubles it (half speed).

Climb speed uses a blend: `(agility + strength) / 2` fed into the same
formula, applied to `climb_ticks_per_voxel`. Because the stat-to-multiplier
mapping is exponential, this averaging produces a geometric-mean blend of the
two stats' effects (intentional — climbing requires both muscle and nimbleness).

### Constitution -> HP Max

```
effective_hp = apply_stat_multiplier(base_hp_max, constitution)
```

CON +30 troll with base HP 200: `200 * 8 = 1600 HP`. Beefy.

### Perception -> Hostile Detection Range

```
effective_range_sq = apply_stat_multiplier(base_detection_range_sq, perception)
```

PER +10 doubles the detection range squared (i.e., increases effective linear
detection range by ~41%). High-PER creatures (deer PER +10, squirrel PER +10)
spot threats from further away; low-PER creatures (-5 troll) are easier to
ambush. See F-per-detection.

### Dexterity -> Arrow Angular Deviation

Arrow aim deviation is expressed as **parts-per-million of distance** (micro-
voxels of offset per voxel of range). Applied as a perpendicular offset to the
aim velocity vector at launch time.

The deviation decreases asymptotically with dexterity — high DEX reduces
deviation but never eliminates it entirely:

```
// dex_mult = stat_multiplier(dexterity), in 2^20 fixed-point.
// base_deviation = config param (default: 50000 ppm = 5% of range).
// 1 << 20 = the multiplier value at DEX 0 (human baseline).
deviation_ppm = base_deviation * (1 << 20) / dex_mult
```

This uses the inverse of the exponential multiplier. Higher DEX means a larger
`dex_mult` denominator, which shrinks the deviation.

Worked examples with `base_deviation = 50000` (5% of range):

| DEX  | dex_mult      | deviation_ppm | at 10 voxels | at 30 voxels |
|------|---------------|---------------|--------------|--------------|
| -10  |       524288  |        100000 | +/- 1.0 voxel | +/- 3.0 voxels |
|   0  |      1048576  |         50000 | +/- 0.5 voxel | +/- 1.5 voxels |
| +10  |      2097152  |         25000 | +/- 0.25 voxel | +/- 0.75 voxels |
| +20  |      4194304  |         12500 | +/- 0.125 voxel | +/- 0.375 voxels |

At launch, sample a random perpendicular offset from
`[-deviation_ppm, +deviation_ppm]` on each lateral axis (X and Z for a
horizontal shot), scale by the sub-voxel distance to target, then add to the
aim velocity. All computed in SubVoxelCoord integer math (2^30 units per
voxel — plenty of precision for micro-voxel offsets).

The deviation is applied to the velocity direction, not the target position.
This means at close range (3 voxels) even poor dexterity rarely misses, while
at long range (30 voxels) low-DEX creatures spray arrows everywhere and
high-DEX archers remain surgical.

Potential integer overflow concern: `base_deviation * (1 << 20)` at max
deviation (100000 * 1048576) = ~105 billion, which fits in i64 (max ~9.2e18).
Safe even for very negative DEX values — at DEX -60 the multiplier is 16384,
giving `50000 * 1048576 / 16384 = 3200000 ppm` (3.2 voxels per voxel of
range). Extreme but won't overflow.

## Save/Load Compatibility

Existing saves from before F-creature-stats will have creatures with no stat
traits. The `trait_int` helper returns a default value (0) for missing traits.
With stat 0, all multipliers equal 1x (human baseline), so existing creatures
behave identically to before. No migration logic needed.

## Size (Future Visual Hook)

Size is derived from a combination of species base size and stats (primarily
Constitution, possibly a dedicated Growth Factor trait in the future). It
affects sprite scaling — a +/-15% variation in sprite dimensions makes crowds
look alive.

This is deferred to a follow-up since it touches the render path (sprite
scaling in GDScript/gdext).

## TraitKind Variants

New variants added to the `TraitKind` enum:

```rust
// Creature stats (F-creature-stats)
Strength,
Agility,
Dexterity,
Constitution,
Willpower,
Intelligence,
Perception,
Charisma,
```

Stored as `TraitValue::Int` in the `creature_traits` table, same as visual
traits from F-creature-biology.

## SpeciesData Config Additions

```rust
/// Per-stat distribution parameters for this species.
/// Key: stat TraitKind, Value: (mean, stdev).
pub stat_distributions: BTreeMap<TraitKind, StatDistribution>,
```

```rust
pub struct StatDistribution {
    pub mean: i32,
    pub stdev: i32,
}
```

Stats not present in the map default to `(0, 5)` (human baseline with moderate
variation).

## References

- **Design doc:** Section 18 (Personality Axes) describes the deferred
  personality system. Creature stats are orthogonal — physical/mental
  capabilities rather than personality multipliers. The two systems coexist:
  personality affects behavior choices, stats affect capability magnitudes.
- **Tracker:** F-creature-biology (done — provides TraitKind, creature_traits
  table), F-per-detection (todo — PER -> detection range), F-creature-stats
  (this feature).

## Future Interactions (Not in Scope)

- **Heredity:** Offspring stats derived from parent stats + variation.
- **Training:** Effective stat = innate + training bonus. Innate is stored
  trait, training bonus is a separate system.
- **Buffs/debuffs:** Temporary modifiers from potions, injuries, moods.
- **Aging:** Stats change over creature lifetime.
- **Crafting quality:** DEX + INT + PER -> quality modifier on crafted items.
- **Mana system:** WIL -> mana pool, WIL + INT + CHA -> recharge rate.
- **Singing:** CHA -> choir effectiveness, mana generation during construction.
- **Carry capacity:** STR -> max carry weight.
