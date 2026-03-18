# F-armor: Wearable Armor System

## Damage Reduction

- Total armor = sum of `effective_armor_value()` across all equipped pieces
- Subtract total armor from incoming damage (melee and projectile)
- Minimum 1 incoming damage always gets through

## effective_armor_value(kind, material, current_hp, max_hp, ArmorParams) -> i32

Free function in `inventory.rs` that encapsulates all armor logic.
Takes individual fields rather than `ItemStack` directly (avoids circular
dependency with `db.rs`). `ArmorParams` bundles config thresholds/penalties.

- Calls `base_armor_value()` (renamed from current `armor_value()`) for the
  raw value from item kind + material
- Applies condition penalty based on current_hp / max_hp thresholds:
  - "Worn" (HP% <= worn threshold, default 70%): -1 penalty, floor 2
  - "Damaged" (HP% <= damaged threshold, default 40%): -2 penalty, floor 1
  - Normal condition: no penalty
- `base_armor_value()` marked as internal helper

## Durability Degradation on Hit

When a creature takes damage, after computing armor reduction:

1. **Pick a body location** via weighted random: Torso=5, Legs=4, Head=3,
   Feet=2, Hands=1. These weights are configurable in GameConfig.
2. **Check what's equipped** at that location:
   - Nothing: no degradation
   - Armor or clothing: degrade it
3. **Compute degradation amount:**
   - **Penetrating hit** (raw damage > total armor): roll uniformly in
     `[0, 2 * penetrating_damage]` where penetrating_damage = final_damage - 1
     (the amount that got through beyond the minimum). Mean = penetrating_damage.
   - **Non-penetrating hit** (raw damage <= total armor, so minimum 1 applied):
     1/20 chance of losing 1 HP. Otherwise no degradation.
4. Apply degradation via existing `inv_damage_item()` infrastructure. If item
   reaches 0 HP, existing `ItemBroken` event fires — no special combat logic.

## No Special Break Logic

Armor breaking mid-combat just removes it. Subsequent hits in the same tick
see the updated equipped state naturally.

## Config Knobs (GameConfig)

- `armor_min_damage: i64` — floor on incoming damage after reduction (default 1)
- `armor_non_penetrating_degrade_chance_recip: i32` — 1-in-N chance (default 20)
- `armor_worn_penalty: i32` — penalty when worn condition (default 1)
- `armor_damaged_penalty: i32` — penalty when damaged condition (default 2)
- `armor_degrade_location_weights: [i32; 5]` — weights for [Torso, Legs, Head, Feet, Hands] (defaults [5,4,3,2,1])

All integer math — no floating point for cross-platform determinism.

## Integration Points

- `combat.rs`: `try_melee_strike()` and `resolve_projectile_creature_hit()`
  need to query equipped armor and apply reduction before `apply_damage()`
- `inventory.rs`: rename `armor_value()` -> `base_armor_value()`, add
  `effective_armor_value()` free function + `ArmorParams` config struct
- Existing `inv_damage_item()` handles durability; existing `ItemBroken` event
  handles item destruction
- No UI changes needed (inventory already shows worn/damaged labels;
  equipment sprites are a separate tracker item)
