# F-bldg-dining: Dining Hall

**Status:** Implemented
**Depends on:** F-furnish (done), F-logistics (done)
**Related:** F-food-quality-mood, F-bldg-kitchen, F-food-chain, F-slow-eating

## Overview

A communal dining building where elves eat meals together. Elves prefer
dining halls over eating carried food, and get a small mood boost for doing
so. Eating outside a dining hall incurs a small new mood penalty.

The dining hall is created via the existing furnish flow: build a shell, then
issue `FurnishStructure` with `FurnishingType::DiningHall`. Food is stocked
via the existing logistics want system. Both systems are already implemented.

## Hunger Thresholds

Add `food_dining_threshold_pct` alongside the existing
`food_hunger_threshold_pct` on `SpeciesData`. Lower `food_hunger_threshold_pct`
to serve as the emergency threshold. Both are per-species, serialized in
`default_config.json`.

- **`food_dining_threshold_pct`** (new, e.g. 60): elf seeks a dining hall.
- **`food_hunger_threshold_pct`** (existing, lowered to e.g. 40): elf eats
  carried food or forages, as today.

Decision flow on heartbeat when idle:

1. `food >= dining_threshold` → not hungry, do nothing.
2. `food < dining_threshold` and `food >= hunger_threshold` → seek nearest
   valid dining hall. If none available (no free seat or no food), skip —
   elf remains idle and available for other autonomous tasks until next
   heartbeat re-evaluates.
3. `food < hunger_threshold` → eat bread if carrying any (instant, as
   today), else seek nearest edible fruit. Existing `EatBread`/`EatFruit`
   paths unchanged.

## Building & Furniture

`FurnishingType::DiningHall` already maps to `FurnitureKind::Table` with a
density of 1 table per 4 floor tiles. Each table provides a configurable
number of implicit dining seats (`dining_seats_per_table`, new field in
`GameConfig`, `u32`, default 4). Total dining capacity = tables × seats per
table. No individual chair furniture items — seats are implicit.

Food is stocked via logistics wants: the player configures a want on the
dining hall for edible items (Bread and Fruit). The want quantity caps how
much food is requested; no separate inventory capacity limit.

## Dining Task

New variant: `TaskKind::DineAtHall { structure_id: StructureId }`.

When an elf crosses the dining threshold:

1. Find the nearest dining hall with a free seat and stocked food, using
   multi-target Dijkstra (same pattern as `find_nearest_bed()`).
2. **Reserve** a seat via `TaskVoxelRole::DiningSeat` in `task_voxel_refs`
   (table position; free-seat check counts existing DiningSeat refs against
   capacity). **Reserve** a food item via `reserved_by` on `ItemStack`.
   Add a `task_structure_refs` row linking the task to the dining hall.
3. Self-assign the task. Elf paths to the table.
4. On arrival, food is consumed instantly: food item removed, hunger restored
   by `food_restore_pct` (same as current eating), `ThoughtKind::AteDining`
   mood boost applied. The `eat_action_ticks` animation plays after
   consumption; if interrupted during the animation, the meal is already
   complete — no effect.

**Preemption level:** Survival (same as EatBread/EatFruit).

## Interruption Handling

Since eating is instant (food consumed on arrival), the only meaningful
interrupt case is **before arrival** (still pathing): release seat
reservation (remove `task_voxel_refs` row), food reservation (clear
`reserved_by`), and structure ref. Food is preserved in dining hall inventory.

Interruptible slow eating with partial restoration is deferred to
F-slow-eating.

After resolving the interruption, the elf re-evaluates hunger on becoming idle
and may start a fresh dining task.

## Mood Effects

Two new `ThoughtKind` variants:

- **`AteDining`**: small flat mood boost (config-tunable). Applied on dining
  hall meal completion.
- **`AteAlone`**: small flat mood penalty (config-tunable). Applied when eating
  via EatBread/EatFruit.

Both replace the existing `AteMeal` thought for their respective paths.
`AteMeal` is kept as a deserialization alias for backward compatibility with
existing saves — maps to `AteDining` on load (preserving the positive mood
of old saves rather than retroactively penalizing).

The `AteAlone` penalty is deliberately small — colonies without dining halls
still function, just slightly less happily.

## New Fields and Variants

Summary of additions for implementation and serde:

- **`SpeciesData`**: `food_dining_threshold_pct: u32` — `#[serde(default = 60)]`.
  Existing `food_hunger_threshold_pct` default lowered to 40.
- **`GameConfig`**: `dining_seats_per_table: u32` — `#[serde(default = 4)]`.
- **`TaskKind`**: `DineAtHall { structure_id }` — new `TaskKindTag` discriminant.
- **`TaskVoxelRole`**: `DiningSeat` variant.
- **`ThoughtKind`**: `AteDining`, `AteAlone` variants. `AteMeal` kept for
  deserialization only (alias → `AteDining`).

## Deferred

- Food quality scaling mood boost (F-food-quality-mood).
- Slow eating with interruptible consumption (F-slow-eating).
- Social bonus for eating with others (requires querying concurrent diners).
- Player-placed furniture.
- Dining hall inventory capacity limits.
