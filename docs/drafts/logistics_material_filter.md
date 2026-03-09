# Logistics Material Filter (v4)

**Tracker:** F-logistics-filter
**Status:** Draft

## Problem

The logistics want system currently stores only `ItemKind` + quantity. This means:

- A storehouse can request "Fruit" but not a specific fruit species.
- Bread is always unmaterialed, but if a want says "Fruit" it matches *any* fruit — there's no way to request only unmaterialed fruit, only a specific species, or explicitly "any fruit."
- Workshop auto-wants from recipes also lack material specificity.
- The UI is hardcoded to offer only "Bread" and "Fruit" — other item kinds (Bow, Arrow, Bowstring) are missing entirely.

As fruit variety matures and processing recipes become material-aware, logistics needs to be able to request specific materials.

## Design

### MaterialFilter enum (sim)

```rust
/// Constrains which materials satisfy a logistics want.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord,
         Serialize, Deserialize)]
pub enum MaterialFilter {
    /// Any material or no material. "Give me any Fruit."
    #[default]
    Any,
    /// A specific material. "Give me Shinethúni Fruit."
    Specific(Material),
}
```

This lives in `inventory.rs` alongside `Material`. `Material` is `Copy`, so `MaterialFilter` derives `Copy` too. Derives `Default` (→ `Any`) so all `#[serde(default)]` annotations work without named default functions. Derives `Ord` so it can be used in deduplication keys with BTreeMap (required for determinism — codebase avoids HashMap).

The `matches` method takes `Option<Material>` (not `Option<&Material>`) since `Material` is `Copy`:

```rust
impl MaterialFilter {
    pub fn matches(self, material: Option<Material>) -> bool {
        match self {
            MaterialFilter::Any => true,
            MaterialFilter::Specific(m) => material == Some(m),
        }
    }
}
```

**No `None` variant.** There is no current use case for "only unmaterialed items." Bread has no material variants. Harvested fruit always has a `FruitSpecies` material. Bow/Arrow/Bowstring always get a wood material from crafting. If a future need arises, adding a `None` variant is trivial. Omitting it now keeps the UI simpler (no confusing "generic Fruit" option that matches nothing).

**Future extension point:** Additional variants like `HasProperty(FruitProperty)` or `AnyWood` can be added later without restructuring.

### Schema changes

**LogisticsWantRow** gains a new field:

```rust
#[serde(default)]
pub material_filter: MaterialFilter,
```

Default is `Any` for backward compatibility with existing saves.

**LogisticsWant DTO** (building.rs) gains the same field with `#[serde(default)]`. This is needed for backward compatibility with `GameConfig.elf_default_wants` which is deserialized from `default_config.json`.

**All existing `LogisticsWant` construction sites** in sim.rs (storehouse, kitchen, workshop furnishing defaults) and config.rs (`default_elf_default_wants()`) must add `material_filter: MaterialFilter::Any`.

**Uniqueness invariant:** A building's wants must have unique `(item_kind, material_filter)` pairs. `set_inv_wants()` deduplicates its input by merging duplicate `(kind, filter)` pairs, taking the max quantity. Max (not sum) is used because duplicates indicate a UI or serialization bug, not an intent to combine quantities — taking the max is a safe idempotent fixup rather than silently doubling. This is necessary because the tabulosity table has no compound unique index.

### Overlapping wants semantics

A building can have both `Any` and `Specific` wants for the same `ItemKind`. For example, "Any Fruit: 5" and "Shinethúni Fruit: 10". These are **additive and independent** — the building requests up to 15 fruit total (up to 5 of any kind, plus up to 10 Shinethúni specifically).

This is the simplest correct semantics. Each want has its own gap calculation and its own haul tasks. Items of species X simultaneously satisfy the "Any" want (because `Any.matches(Some(X))` is true) and the "Specific(X)" want. This means Shinethúni fruit counts toward both wants' `current` tally, which naturally results in the building being satisfied earlier when it holds enough Shinethúni.

**Concrete example:** Building wants "Any Fruit: 5" + "Shinethúni: 10", holds 12 Shinethúni + 0 other.
- Any want: current=12, gap=0 (satisfied)
- Specific want: current=12, gap=0 (satisfied)
- Result: no hauls. Building holds 12, both wants met.

**If 7 Shinethúni are removed** (e.g., consumed): holds 5 Shinethúni.
- Any want: current=5, gap=0 (still satisfied)
- Specific want: current=5, gap=5 (needs 5 more Shinethúni)
- Result: 1 haul for Shinethúni. Correct.

This is a known, intentional semantic. The UI should not display per-want "current count" (which would be confusing due to overlap). Instead, the wants list shows target quantities only, and the overall inventory display shows actual held items.

### Functions requiring signature changes

Adding `MaterialFilter` affects more functions than just the matching hot path. Complete list:

| Function | Current signature (relevant params) | Change |
|----------|-------------------------------------|--------|
| `process_logistics_heartbeat()` | iterates wants | Pass `want.material_filter` to all downstream calls |
| `find_haul_source()` | `item_kind: ItemKind` | Add `filter: MaterialFilter` |
| `inv_item_count()` | `inv_id, item_kind` | Add `filter: MaterialFilter` |
| `inv_unreserved_item_count()` | `inv_id, item_kind` | Add `filter: MaterialFilter` |
| `inv_reserve_items()` | `inv_id, item_kind, qty, task_id` | Add `filter: MaterialFilter`; only reserve stacks where `filter.matches(stack.material)`; return `Option<Material>` (see below) |
| `reserve_haul_items()` | `source, item_kind, qty, task_id` | Add `filter: MaterialFilter`; delegates to `inv_reserve_items`; propagate returned material |
| `count_in_transit_items()` | `structure_id, item_kind` | Add `filter: MaterialFilter`; see "In-transit counting" |
| `inv_want_target()` | `inv_id, item_kind` | **Replace** (not extend) — see "Want target queries" |
| `compute_recipe_wants()` | (internal) | Deduplicate on `(item_kind, material_filter)` not `item_kind` alone |
| `find_acquire_source()` | `item_kind: ItemKind` | Add `filter: MaterialFilter` (creature personal wants) |

**Critical: `inv_reserve_items()` must filter by material.** Without this, a `Specific(FruitSpecies(X))` want could reserve stacks of a different species. The source-finding logic picks the right inventory, but the reservation must also match at the stack level.

**`inv_reserve_items()` return type change:** Currently returns nothing (mutates state). Must now return `Option<Material>` — the material of the stacks it reserved. When filter is `Specific(X)`, this is always `Some(X)`. When filter is `Any`, it returns the material of the first stack reserved.

**Single-material reservation under `Any`:** When `inv_reserve_items` is called with `MaterialFilter::Any`, it picks a single material and only reserves stacks of that material. This avoids the problem of mixed-material hauls: a pile with 3 Shinethúni + 5 Révatórun, with a request for 6 `Any` fruit, reserves 5 Révatórun (the first material with sufficient quantity, or if none has enough, the first material's full count). The remaining demand (1) will be filled by a subsequent heartbeat. This keeps `hauled_material` unambiguous and haul tasks simple.

Implementation: `inv_reserve_items` iterates stacks matching `(kind, filter)`. On the first matching stack, it locks in that stack's material. It then only reserves further stacks with the same material. Returns `Some(locked_material)` or `None` if nothing was reserved.

### Gap calculation in process_logistics_heartbeat

For each want `(item_kind, material_filter, target_quantity)` on a building:

```
current = inv_item_count(inv_id, item_kind, material_filter)
in_transit = count_in_transit_items(building_id, item_kind, material_filter)
gap = max(0, target_quantity - current - in_transit)
```

When `material_filter` is `Any`, `inv_item_count` counts all items of that kind regardless of material. When `Specific(X)`, it counts only items with material X. This is correct: an "Any Fruit: 5" want is satisfied by any 5 fruit, while a "Shinethúni Fruit: 10" want counts only Shinethúni.

### find_haul_source: three-phase walk-through

All three phases thread the material filter:

**Phase 1: Ground piles.** `inv_unreserved_item_count(pile_inv, item_kind, filter)` — only counts stacks matching the filter. A pile with 3 Shinethúni Fruit and 5 Révatórun Pod: a `Specific(Shinethúni)` request sees 3 available; `Any` sees 8.

**Phase 2: Lower-priority buildings.** Same `inv_unreserved_item_count` call with filter. A lower-priority storehouse holding mixed fruit: only matching stacks count.

**Phase 3: Surplus buildings.** Surplus = `held - wanted`, where:
- `held = inv_unreserved_item_count(source_inv, item_kind, filter)` — how many items matching the *caller's* filter this building has unreserved.
- `wanted = inv_want_target_total(source_inv, item_kind)` — total of *all* wants for this kind at the source building (regardless of filter). This prevents taking items that the source building itself wants, even if the wants are for different materials.

This is conservative: a source building wanting "Shinethúni Fruit: 10" and holding 15 Shinethúni + 8 Révatórun sees `wanted=10` for both caller filters. A `Specific(Révatórun)` caller sees `held=8, wanted=10, surplus=0` — the source's total want exceeds the Révatórun held, even though the source doesn't want Révatórun specifically. This is over-conservative in pathological cases but prevents accidentally stripping items from buildings that have complex multi-material wants. Acceptable for v1; can be refined to per-material want accounting later if it causes real gameplay problems.

### Want target queries

The existing `inv_want_target(inv_id, item_kind)` uses `.find()` (returns the first match), which is incorrect if multiple wants for the same kind exist. This function is **replaced** (not extended) with two new functions:

1. **`inv_want_target(inv_id, item_kind, filter)`** — returns the target for a specific `(kind, filter)` pair, or 0 if no such want exists. Used when checking gap for a specific want.
2. **`inv_want_target_total(inv_id, item_kind)`** — sums targets of all wants for a kind regardless of filter. Used by `find_haul_source()` surplus calculation (Phase 3).

### Haul task changes

`TaskHaulData` gains two fields:

```rust
/// The material filter from the want that triggered this haul.
#[serde(default)]
pub material_filter: MaterialFilter,

/// The actual material of the items being hauled. Set when the haul is
/// created based on what `inv_reserve_items` selected. `None` for
/// unmaterialed items (e.g., Bread).
#[serde(default)]
pub hauled_material: Option<Material>,
```

`material_filter` records the want's filter for debugging/display. `hauled_material` records what was actually reserved — this is what in-transit counting uses.

### In-transit counting

`count_in_transit_items(structure_id, item_kind, filter)` uses `hauled_material` (not the haul's `material_filter`) for matching:

```rust
// For each active haul targeting this structure with matching item_kind:
if filter.matches(haul_data.hauled_material) {
    total += haul_data.quantity;
}
```

This is precise: a `Specific(X)` request only counts hauls actually carrying X, not hauls with `Any` filter that might be carrying Y. An `Any` request counts all hauls regardless of what they're carrying.

### Process harvest tasks

Harvest demand calculation remains **material-unaware**. `process_harvest_tasks()` computes global fruit demand/supply to decide whether to create harvest tasks. Harvested fruit species depends on which tree voxel has fruit, not on logistics wants — the sim can't direct elves to harvest a specific species (the species is determined by the voxel). Logistics routes the harvested fruit afterward.

The existing `inv_item_count` and `count_in_transit_items` calls in `process_harvest_tasks` use `MaterialFilter::Any` — total fruit demand vs total fruit supply. This is correct: harvest creates supply, logistics distributes it.

### Creature personal wants

Creature personal wants (`elf_default_wants` in config, `AcquireItem` tasks) use the same `LogisticsWant` type and gain the `material_filter` field. Default wants from config use `MaterialFilter::Any`. There is no per-creature UI for setting material filters — personal wants are config-driven.

`find_acquire_source()` gains a `filter: MaterialFilter` parameter, threading it through `inv_unreserved_item_count` and `inv_reserve_items` the same way `find_haul_source` does. For now, all creature wants use `Any`, but the plumbing is in place.

### Bridge serialization

**`set_logistics_wants()`** — the bridge currently hand-parses JSON (not serde). This continues with material filter support. JSON format changes from:
```json
[{"kind": "Bread", "quantity": 10}]
```
to:
```json
[{"kind": "Bread", "material_filter": "Any", "quantity": 10}]
```

Material filter encoding (matches serde's default externally-tagged enum representation):
- `"Any"` → `MaterialFilter::Any`
- `{"Specific": "Oak"}` → `MaterialFilter::Specific(Material::Oak)`
- `{"Specific": {"FruitSpecies": 42}}` → `MaterialFilter::Specific(Material::FruitSpecies(FruitSpeciesId(42)))`

(`FruitSpeciesId` is a transparent newtype, so serde serializes it as the bare integer.)

The bridge hand-parses these JSON structures. The nesting is modest (at most 2 levels for `Specific` with `FruitSpecies`), which is manageable with hand-parsing. If the parsing becomes unwieldy during implementation, `serde_json` can be used in the gdext crate (which is allowed external dependencies, unlike the sim crate).

**`get_structure_info()`** returns the filter and a display label in the want dict. The UI displays target quantities only (not per-want current counts, since overlapping wants make per-want counts confusing — see "Overlapping wants semantics"). The overall inventory section already shows actual held items.

### SimCommand serialization (multiplayer)

`SimAction::SetLogisticsWants` carries `Vec<LogisticsWant>`. Adding `material_filter` changes the serde format. Since multiplayer is not live yet, this is a clean break — no backward compatibility needed for the command wire format. The `#[serde(default)]` on `LogisticsWant.material_filter` ensures old save files deserialize correctly (defaulting to `Any`).

### Bridge: available items query

Two-step query to keep the response small and match the natural two-step UI flow:

1. **`get_logistics_item_kinds() -> VarArray`** — returns all `ItemKind` variants with labels:
   ```
   [{"kind": "Bread", "label": "Bread"}, {"kind": "Fruit", "label": "Fruit"}, ...]
   ```

2. **`get_logistics_material_options(kind: String) -> VarArray`** — returns material filter options for a given kind:
   ```
   [
     {"filter": "Any", "label": "Any Fruit"},
     {"filter": {"Specific": {"FruitSpecies": 1}}, "label": "Shinethúni Fruit"},
     {"filter": {"Specific": {"FruitSpecies": 2}}, "label": "Révatórun Pod"},
     ...
   ]
   ```

For each `ItemKind`:
- Always include an "Any" option.
- Include `Specific(m)` for each material that makes sense for that kind:
  - Fruit → each `FruitSpecies` in the DB. Labels use species Vaelith name + shape noun, looked up from the `fruit_species` table via the same logic as `SimState::item_display_name()`. (`Material::display_name()` alone is insufficient — it returns generic "Fruit" for all species.)
  - Bow/Arrow/Bowstring → each wood `Material` (Oak, Birch, Willow, Ash, Yew)
  - Bread → only "Any" (no material variants)
- If the only option is "Any" (e.g., Bread), the UI can skip the material step.

### UI changes (structure_info_panel.gd)

Replace the hardcoded `["Bread", "Fruit"]` picker with a two-step flow:

1. Pick item kind (populated from `bridge.get_logistics_item_kinds()`).
2. Pick material filter (populated from `bridge.get_logistics_material_options(kind)`).
   - If only "Any" is available, skip step 2 and auto-select it.

Display in the wants list shows the label and target quantity (e.g., "Shinethúni Fruit: 10" or "Any Fruit: 5"). No per-want "current" count — the overall inventory section shows what's held.

### Workshop auto-wants interaction

Workshops fully own their wants — `set_inv_wants()` does a full replace on every `SetWorkshopRecipes` command. Manual editing of workshop wants is overwritten on the next recipe change. This is existing behavior and remains unchanged.

Auto-wants from recipes use `MaterialFilter::Any` since recipes don't yet specify materials. `compute_recipe_wants()` deduplication updates to match on `(item_kind, material_filter)` pairs. Since all auto-wants use `Any`, this is equivalent to the current behavior — but correct when recipes become material-aware in the future.

Non-workshop buildings (storehouses) are not affected by auto-wants — their wants are set only by manual UI interaction.

## Scope

### In scope
- `MaterialFilter` enum with `Any` / `Specific(Material)` (derives `Default`, `Ord`)
- `LogisticsWantRow` and `LogisticsWant` schema extension (with `#[serde(default)]`)
- All existing `LogisticsWant` construction sites updated with `material_filter: MaterialFilter::Any`
- Material-aware matching in all inventory counting and reservation functions
- Single-material reservation under `Any` filter (lock in first material found)
- `inv_reserve_items` returns `Option<Material>` (the reserved material)
- `TaskHaulData` gains `material_filter` and `hauled_material` for precise in-transit counting
- Explicit gap calculation formula using material filter
- `find_haul_source` three-phase surplus logic threaded with filter
- Overlapping wants are additive and independent (documented semantic)
- `inv_want_target` replaced with per-filter and total variants
- `process_harvest_tasks()` remains material-unaware (uses `Any`)
- `find_acquire_source()` gains filter parameter (creature personal wants)
- `set_inv_wants()` deduplicates input by `(kind, filter)` pairs (max quantity)
- `compute_recipe_wants()` deduplication on `(item_kind, material_filter)`
- Bridge two-step query (`get_logistics_item_kinds`, `get_logistics_material_options`)
- Bridge serialization (set/get) with hand-parsed JSON matching serde enum format
- Dynamic two-step UI picker replacing hardcoded list
- All existing item kinds available in picker (not just Bread/Fruit)
- Serde backward compat (default to `Any`) for saves, commands, and config
- `SimCommand` format is a clean break (multiplayer not live)
- UI shows target quantities only per want (no per-want current counts)

### Out of scope (future)
- `None` filter variant (add when a use case arises)
- Property-based filters (e.g., "any starchy fruit")
- Recipe material constraints
- Material-aware workshop auto-wants
- Per-creature material filter UI
- Per-material surplus accounting (refine Phase 3 conservatism)

## Test plan

1. **MaterialFilter::matches()** — unit tests for `Any` and `Specific` against `None` and `Some(material)` values.
2. **MaterialFilter Ord** — verify deterministic ordering (Any < Specific variants).
3. **Serde roundtrip** — both filter variants serialize/deserialize correctly; old saves without the field default to `Any`.
4. **Logistics delivery with Specific filter** — set up wants with `Specific(FruitSpecies(X))`, verify only species X is hauled. Verify `inv_reserve_items` reserves only matching stacks when a ground pile has mixed species.
5. **Logistics delivery with Any** — verify any material satisfies the want.
6. **Single-material reservation under Any** — pile has 3 Shinethúni + 5 Révatórun, `Any` request for 6: verify only one material is reserved per haul, `hauled_material` is set correctly.
7. **Gap calculation accuracy** — verify `inv_item_count`, `inv_unreserved_item_count`, and `count_in_transit_items` all respect the material filter.
8. **Overlapping wants (additive semantics)** — building wants "Any Fruit: 5" + "Shinethúni Fruit: 10". Hold 12 Shinethúni: both satisfied, no hauls. Remove 7: Any still satisfied (5 held), Specific has gap 5. Verify exactly 1 Shinethúni haul created.
9. **Surplus calculation** — `inv_want_target_total()` correctly sums all fruit wants when determining if a building has surplus to give away. Phase 3 surplus uses caller's filter for `held`, total wants for `wanted`.
10. **Surplus conservatism** — source wants "Shinethúni: 10", holds 8 Révatórun. Caller wants `Specific(Révatórun)`. Verify surplus=0 (conservative: total wanted exceeds held-for-filter).
11. **Harvest demand** — `process_harvest_tasks()` uses `MaterialFilter::Any` for global fruit demand, unaffected by specific-material wants.
12. **Workshop auto-wants** — recipe wants use `Any` filter; deduplication works on `(kind, filter)` pairs.
13. **Creature personal wants** — `find_acquire_source()` respects `MaterialFilter::Any` from default elf wants.
14. **get_logistics_item_kinds / get_logistics_material_options** — returns correct options including all fruit species with proper display names (Vaelith name + shape noun, not generic "Fruit").
15. **Uniqueness** — `set_inv_wants()` deduplicates duplicate `(kind, filter)` pairs by merging (max quantity).
16. **hauled_material tracking** — haul task records actual material from reserved stacks; `count_in_transit_items` uses `hauled_material` for matching.
17. **Config backward compat** — `default_config.json` without `material_filter` in `elf_default_wants` deserializes correctly.
18. **inv_want_target replacement** — old `.find()` semantics replaced; per-filter and total variants both return correct values with multiple wants for same kind.
