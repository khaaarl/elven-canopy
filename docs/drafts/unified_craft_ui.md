# Unified Crafting UI

**Tracker:** F-unified-craft-ui
**Status:** Draft v5

## Overview

Replace the per-building-type crafting UIs (kitchen cooking toggle, workshop recipe list) with a single unified, data-driven crafting panel. All buildings with crafting capabilities — kitchens, workshops, and future specialized buildings — share the same UI code backed by different recipe data. Recipes are defined as runtime catalog entries (not DB rows), identified structurally rather than by name, with per-output production targets.

**Scope note:** Greenhouses are explicitly out of scope. Their current behavior is a placeholder; future greenhouse mechanics (fruit growth schedules, fertilization, per-planter tracking) diverge significantly from the crafting model described here.

## Goals

- **One UI for all crafting.** Kitchen bread baking, workshop bow-making, and future fruit-variety operations all render through the same panel and code path.
- **Data-driven recipes.** The set of available recipes is not hardcoded in UI — it comes from Rust. Fruit-variety recipes are generated dynamically from the current world's fruit species.
- **Per-output targets.** Each active recipe has independent target quantities for each output. The recipe keeps running until ALL output targets are met.
- **Auto-logistics.** Active recipes can automatically generate logistics wants for their inputs, with configurable spare iterations.
- **Hierarchical recipe browsing.** Recipes are organized into categories. The hierarchy is generated per-building-type, with smart flattening for buildings that have few recipes.

## Recipe Identity

### The Problem

Recipes were previously identified by string IDs (e.g., `"bow"`, `"bowstring"`). This is fragile: renaming a recipe in a patch breaks saved active recipes. Recipe definitions are not DB entities — they're config-level data describing what's *possible*, like species data.

### Structural Keys

A recipe is identified by its *content* — what it does, not what it's called:

```rust
/// Structural identity for a recipe. Two recipes with identical keys
/// are the same recipe, regardless of display name changes.
///
/// Input and output Vecs MUST be sorted in canonical order (derived Ord)
/// to ensure identical recipes produce identical keys regardless of
/// definition order. The catalog builder enforces this at construction time.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
struct RecipeKey {
    verb: RecipeVerb,
    inputs: Vec<(ItemKind, MaterialFilter, u32)>,   // canonically sorted
    outputs: Vec<(ItemKind, Option<Material>, u32)>, // canonically sorted
}
```

The `RecipeVerb` disambiguates recipes that share inputs and outputs but represent fundamentally different processes (e.g., "husk fruit" vs "press fruit"):

```rust
/// STABLE ENUM: Discriminant values are persisted in save files.
/// Never reorder, never reuse a number. Append new variants at the end
/// with the next available discriminant. Comment out removed variants
/// (do not delete) to prevent accidental reuse.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u16)]
enum RecipeVerb {
    Assemble = 0,
    Brew = 1,
    Cook = 2,
    Extract = 3,
    Fletch = 4,
    Husk = 5,
    Mill = 6,
    // ... append new variants here with next sequential number
}
```

This is a curated enum, extended when new crafting methods are introduced.

### Stable Enum Policy

**All enums that participate in `RecipeKey` must be append-only with stable discriminant values.** This applies to:

- `RecipeVerb` — `#[repr(u16)]` with explicit discriminants (shown above).
- `ItemKind` — must adopt `#[repr(u16)]` with explicit discriminants. New variants append at the end with the next sequential number.
- `Material` — same policy as `ItemKind`.
- `MaterialFilter` — same policy as `ItemKind`.

The append-only policy is critical because `derive(Ord)` uses *declaration order* (not discriminant values) — inserting a variant mid-enum would change canonical sort order, causing saved `RecipeKey` values to no longer match catalog keys. The `#[repr(u16)]` with explicit discriminants serves a different purpose: it future-proofs for potential binary serialization formats and makes accidental reordering obvious in diffs.

### Serialization Format

`RecipeKey` is serialized using **serde JSON with deterministic field ordering** (serde's default struct serialization produces fields in declaration order, which is stable). Vec elements are pre-sorted by the catalog builder, so the JSON output is canonical for any given key. This format is human-readable in save files and debuggable.

**Stability constraint:** Serde JSON serializes enum variants by *name* (e.g., `"Assemble"`, not `0`), so the JSON stability contract is: don't rename variants, don't reorder struct fields. The `#[repr(u16)]` discriminants do not affect JSON serialization — they exist for potential future binary formats. The field declaration order of `RecipeKey` is also part of the serialization contract. Reordering fields would change the JSON representation and orphan all saved keys. Add a comment on the struct marking field order as stable.

GDScript receives `RecipeKey` as a JSON string (`key_json`) from the bridge and treats it as **opaque** — it is never parsed or constructed on the GDScript side. It is only stored and passed back to the bridge in commands.

### Key Resolution and Orphan Handling

On game load, each saved `RecipeKey` in `ActiveRecipe` rows is resolved against the current recipe catalog. Orphaned keys (recipes that no longer exist due to game updates or world changes) trigger a player notification identifying the building name and lost recipe display name (stored alongside the key for this purpose), then the orphaned `ActiveRecipe` rows are deleted (cascade removes associated `ActiveRecipeTarget` rows).

### Why Not DB Entities?

Recipes describe the rules of the game, not the state of the world. They belong alongside `GameConfig` and `SpeciesData`. Putting them in the DB would mean:
- Needing migration logic when recipe definitions change between versions.
- Duplicating recipe data across save files.
- Complicating the fruit-variety pipeline (fruit species generate recipes at world creation; these are deterministic from the seed, not mutable state).

## Recipe Catalog

The **recipe catalog** is a runtime structure built at game startup from config recipes + dynamically generated fruit-variety recipes. It is rebuilt on load (deterministically from seed + config). The catalog is stored on `SimState` (alongside `GameConfig`) and is immutable after construction. Internally it uses a `BTreeMap<RecipeKey, RecipeDef>` for O(log n) lookups.

The catalog is built in a deterministic order: hardcoded config recipes first (in config Vec order), then dynamically generated fruit-variety recipes (ordered by `FruitSpeciesId`, then by verb). This ordering is the canonical iteration order used everywhere. The catalog builder validates that all recipe outputs have `quantity >= 1` (zero-quantity outputs are a config error that would cause division by zero in `runs_needed` computation).

```rust
struct RecipeDef {
    key: RecipeKey,
    display_name: String,
    category: Vec<String>,               // e.g., ["Brewing", "Cordials"]; empty vec = root level (no nesting)
    furnishing_types: Vec<FurnishingType>, // which buildings can use this
    inputs: Vec<RecipeInput>,
    outputs: Vec<RecipeOutput>,
    work_ticks: u64,
    subcomponent_records: Vec<RecipeSubcomponentRecord>,
    required_species: Option<Species>,    // None = any species can craft; Some = only that species
}
```

Note: `inputs` and `outputs` on `RecipeDef` are the full structs (with all metadata like quality), while `RecipeKey` contains only the identity-relevant subset. The `RecipeDef` fields are the authoritative source for crafting logic; the key is only for identity/lookup.

**`RecipeInput` changes:** The existing `RecipeInput` struct gains a `material_filter: MaterialFilter` field (defaulting to `Any`), matching what `RecipeKey` expects. This enables material-specific recipe inputs (e.g., "requires Yew wood specifically").

### Per-Building Hierarchy Generation

Each recipe has a `category: Vec<String>` path (e.g., `["Brewing", "Cordials"]`). The category tree shown to the player is generated per-building-type:

1. Filter the catalog to recipes available for the building's `FurnishingType`.
2. Build a tree from the category paths of matching recipes.
3. Apply smart flattening:
   - If a category node has only one child category, collapse them (e.g., `"Brewing" > "Cordials"` becomes `"Brewing > Cordials"` if there's nothing else under Brewing for this building).
   - If the total recipe count for the building is small (e.g., < 8), skip categories entirely and show a flat list.

This means a generic workshop might show a deep hierarchy, while a specialized mill shows a flat list of its handful of recipes — same code, different data.

### Bridge API

The bridge exposes the recipe catalog to GDScript per-building:

```
get_recipe_catalog_for_building(structure_id) -> Dictionary:
    {
        "tree": [
            {
                "label": "Brewing",
                "children": [
                    {
                        "label": "Cordials",
                        "children": [],
                        "recipes": [
                            {
                                "key_json": "...",          // serialized RecipeKey (opaque to GDScript)
                                "display_name": "Thirasuni Cordial",
                                "inputs": [...],
                                "outputs": [
                                    {"item_kind": "Cordial", "material": "Thirasuni", "quantity": 1}
                                ],
                                "work_ticks": 12000
                            }
                        ]
                    }
                ],
                "recipes": []
            },
            ...
        ],
        "flat_recipes": [...]   // same recipes, flat list (for when hierarchy is skipped)
    }
```

GDScript renders whichever representation is appropriate based on recipe count. If the structure is unfurnished, `get_recipe_catalog_for_building` returns an empty tree and empty flat list.

## DB Schema

### ActiveRecipe Table

Replaces `workshop_recipe_ids`, `workshop_recipe_targets`, `cooking_bread_target`, and `cooking_enabled` on `CompletedStructure`.

```rust
#[derive(Table)]
struct ActiveRecipe {
    #[primary_key(auto_increment)]
    id: ActiveRecipeId,

    #[indexed]
    structure_id: StructureId,           // FK cascade on CompletedStructure

    recipe_key: RecipeKey,               // structural identity (serialized)
    recipe_display_name: String,         // cached for orphan notification messages
    enabled: bool,                       // can be toggled without removing

    #[indexed(unique)]
    sort_order: u32,                     // priority ordering (lower = higher priority); unique across all recipes globally

    auto_logistics: bool,                // default: true
    spare_iterations: u32,               // default: 0
}
```

**Compound index:** `ActiveRecipe` needs an `#[index(structure_id, sort_order)]` compound index so that queries for a single structure's recipes return results in priority order without an in-memory sort.

**Sort order:** `sort_order` is globally unique (enforced by unique index), which guarantees deterministic iteration order with no tiebreaker needed. New recipes are inserted with `sort_order = max(all existing sort_orders) + 1`. Reorder commands swap `sort_order` values between adjacent recipes within the same structure. "Up" means lower `sort_order` (higher priority).

**No duplicate recipes per building:** `AddActiveRecipe` rejects adding a recipe whose `recipe_key` already exists as an active recipe on the same structure. This prevents confusing duplicate entries.

### ActiveRecipeTarget Table

Per-output target quantities for an active recipe.

```rust
#[derive(Table)]
struct ActiveRecipeTarget {
    #[primary_key(auto_increment)]
    id: ActiveRecipeTargetId,

    #[indexed]
    active_recipe_id: ActiveRecipeId,    // FK cascade on ActiveRecipe

    output_item_kind: ItemKind,
    output_material: Option<Material>,   // None = match any material (uses MaterialFilter::Any semantics)
    target_quantity: u32,                // default: 0 (don't care about this output)
}
```

When a recipe is added, one `ActiveRecipeTarget` row is created per recipe output, all with `target_quantity = 0`. The user must set at least one target to a non-zero value for the recipe to ever run. This is intentional: the user explicitly chooses what they want produced rather than getting unexpected crafting activity.

**Stock counting** uses `MaterialFilter` semantics (mirroring existing logistics want matching):
- `output_material: None` → uses `MaterialFilter::Any`, counts ALL items of the matching `output_item_kind` regardless of material. This is an intentional semantic choice: material-less outputs (like bread) should count all instances, not just untagged ones.
- `output_material: Some(m)` → uses `MaterialFilter::Specific(m)`, counts only items with that exact material.

Only unreserved items count toward stock — items reserved by an in-progress craft task are excluded to prevent double-counting.

### TaskCraftData Changes

`TaskCraftData` gains an `active_recipe_id: ActiveRecipeId` FK (cascade on `ActiveRecipe`) so that `RemoveActiveRecipe` can find and interrupt any in-progress craft task for the removed recipe. The command handler uses this FK to look up the task and calls `interrupt_task()` (the unified interruption entry point), which handles all cleanup: nav invalidation, input unreservation, and creature state reset regardless of whether the creature is mid-walk or mid-craft.

### CompletedStructure Changes

Remove from `CompletedStructure`:
- `workshop_enabled: bool`
- `workshop_recipe_ids: Vec<String>`
- `workshop_recipe_targets: BTreeMap<String, u32>`
- `cooking_enabled: bool`
- `cooking_bread_target: u32`

Add to `CompletedStructure`:
- `crafting_enabled: bool` — single toggle replacing both `workshop_enabled` and `cooking_enabled`.

**Default on furnishing:** Furnishing a building auto-adds all available recipes for the building's furnishing type (via `add_default_active_recipes`). Output targets default to 0 (no automatic production). For kitchens, the bread recipe's target is set to `kitchen_default_bread_target` (50). `crafting_enabled` defaults to `false` on new buildings — the player must explicitly enable crafting and set targets.

**Save compatibility:** Old saves will deserialize with the new `ActiveRecipe` / `ActiveRecipeTarget` tables defaulting to empty (standard tabulosity missing-table behavior). The removed fields will be silently dropped by serde. This means existing workshop and kitchen configurations are lost on upgrade — acceptable during early development.

## Commands

Granular commands replacing `SetWorkshopConfig` and `SetCookingConfig`:

```rust
enum SimAction {
    SetCraftingEnabled {
        structure_id: StructureId,
        enabled: bool,
    },

    AddActiveRecipe {
        structure_id: StructureId,
        recipe_key: RecipeKey,
        // Output targets are initialized to 0 for all outputs; user sets them after adding.
    },

    RemoveActiveRecipe {
        active_recipe_id: ActiveRecipeId,
    },

    SetRecipeOutputTarget {
        active_recipe_target_id: ActiveRecipeTargetId,  // identifies the specific target row
        target_quantity: u32,
    },

    SetRecipeAutoLogistics {
        active_recipe_id: ActiveRecipeId,
        auto_logistics: bool,
        spare_iterations: u32,
    },

    SetRecipeEnabled {
        active_recipe_id: ActiveRecipeId,
        enabled: bool,
    },

    MoveActiveRecipeUp {
        active_recipe_id: ActiveRecipeId,
    },

    MoveActiveRecipeDown {
        active_recipe_id: ActiveRecipeId,
    },
}
```

**Command validation:**

- `SetCraftingEnabled`: Validates structure exists and has a furnishing type. No-op for unfurnished structures. Setting `enabled: false` lets any in-progress craft task finish normally; it only suppresses new task creation (matching current workshop behavior).
- `AddActiveRecipe`: Validates recipe_key exists in catalog, building's FurnishingType is in recipe's furnishing_types, and recipe is not already active on this structure. Invalid adds are silently dropped.
- `RemoveActiveRecipe`: Validates the active recipe exists. If a craft task is in progress for this recipe, it is interrupted and inputs are unreserved.
- `SetRecipeOutputTarget`: Identifies the target row by `ActiveRecipeTargetId` (primary key), avoiding ambiguity. Validates the row exists.
- `MoveActiveRecipeUp` / `MoveActiveRecipeDown`: Swap `sort_order` values between the target recipe and its neighbor *within the same structure* (found via the `(structure_id, sort_order)` compound index, not by global `sort_order` adjacency). Moving the top recipe up or the bottom recipe down is a no-op.

## Crafting Monitor

The existing `process_workshop_monitor()` generalizes to all crafting buildings.

### Tick Logic

On each logistics heartbeat, for each building with `crafting_enabled = true` and at least one active recipe:

1. Skip if there's already an in-progress Craft task for this building.
2. Iterate active recipes in `sort_order` ascending (filtering to this structure's recipes):
   a. Skip if `enabled = false`.
   b. Compute `runs_needed`:
      - For each output target: `shortfall_i = max(0, target_i - stock_i)`, then `runs_for_output_i = ceil(shortfall_i / output_quantity_i)`.
      - `runs_needed = max(runs_for_output_i across all output targets)`.
      - If `runs_needed == 0`, this recipe is satisfied — skip.
   c. Check if ALL inputs are available (unreserved) in the building's inventory.
   d. If inputs available: create a Craft task (with `required_species` from `RecipeDef`, or any species if `None`), reserve inputs, break.
3. If no recipe needs work, building is idle.

**Stock scope is per-building.** Stock is counted only in the building's own inventory. A workshop targeting 10 arrows will keep crafting even if a storehouse elsewhere has 100 arrows. This is intentional: targets represent what this specific building should produce and maintain in its own inventory.

**Multi-output overproduction:** When `runs_needed = max(runs_for_output_i)`, some outputs may be overproduced to satisfy others. For example, a recipe producing 1 bread + 1 crumb with a target of 10 bread and 0 crumbs will produce 10 unwanted crumbs. This is inherent to multi-output recipes and is expected/acceptable behavior.

**All-zero targets:** A recipe where every output has `target_quantity = 0` always has `runs_needed = 0` and is permanently satisfied. It never generates craft tasks or auto-logistics wants. This is the expected state for a newly added recipe before the user configures targets.

**Task location:** The craft task's location is the nav node inside (or adjacent to) the building, same as the current workshop task location logic.

### Stock Counting

Stock for a given output target uses `MaterialFilter` semantics (see ActiveRecipeTarget section above). Only unreserved items in the building's inventory count.

## Auto-Logistics

### Semantic Note

Auto-logistics wants and explicit (user-configured) logistics wants are **summed**, not maxed. This means explicit wants on a crafting building represent "extra stock I want beyond what active recipes need" — a buffer or general-purpose stockpile. This is a deliberate semantic difference from storehouses (where explicit wants are the sole driver): on crafting buildings, explicit wants are additive with auto-wants.

### Want Computation

During the logistics heartbeat, for each building:

1. Start with the building's explicit `LogisticsWant` rows (user-configured via the logistics panel).
2. For each `ActiveRecipe` with `auto_logistics = true` and `enabled = true`:
   a. **Skip if `crafting_enabled = false` on the building.** Auto-logistics does not pre-stage materials when crafting is paused.
   b. Compute `runs_needed` (same formula as crafting monitor above).
   c. If `runs_needed == 0` and `spare_iterations == 0`, this recipe contributes no auto-wants — skip.
   d. Compute `total_runs = runs_needed + spare_iterations`. (When `runs_needed == 0` but `spare_iterations > 0`, this stockpiles inputs for future production even though the recipe is currently satisfied.)
   e. For each recipe input: `auto_want_quantity = input.quantity * total_runs`.
   f. Sum these auto-wants by `(ItemKind, MaterialFilter)`.
3. Merge: for each `(ItemKind, MaterialFilter)`, the final want is `explicit_want + auto_want`.
4. The merged wants drive hauling decisions — logistics hauls if current inventory < want.

**Recipes with no inputs:** Some recipes may have no inputs (e.g., the current arrow recipe). These generate no auto-logistics wants regardless of settings. The crafting monitor still creates tasks for them normally.

### Example

- Recipe: 1 fletching + 1 shaft + 1 arrowhead → 1 arrow
- Target: 10 arrows, current stock: 7
- Spare iterations: 2
- `runs_needed = 3`, `total_runs = 5`
- Auto-wants: 5 fletchings, 5 shafts, 5 arrowheads
- Explicit wants on building: 2 arrowheads
- Final merged wants: 5 fletchings, 5 shafts, 7 arrowheads

**No auto-ejection:** Disabling a recipe or reducing targets does not trigger removal of materials already delivered to the building. Surplus materials remain in the building's inventory until manually hauled elsewhere or consumed by another recipe. This is consistent with existing logistics behavior (wants drive hauls, not ejections).

## UI Layout

### Panel Structure

The crafting UI follows the logistics panel pattern: a small summary on the building's info panel (left side) with a "Details..." button, and a detail panel in the middle of the screen. Only one detail panel (crafting or logistics) is visible at a time — opening one closes the other.

The entire detail panel is wrapped in a single `ScrollContainer` (with programmatic height matching the viewport, as in the military panel). Everything — the add button, recipe picker, and active recipe list — scrolls together as one vertical flow.

### Detail Panel Contents (top to bottom)

1. **Crafting Enabled** checkbox.
2. **"Add Recipe" button.** Clicking expands the recipe picker inline (below the button, inside the scroll).
3. **Recipe picker** (when expanded): a hierarchical, recursively-nested list of categories and recipes. Clicking a category expands/collapses its children (indented, with a disclosure triangle or similar). Clicking a recipe leaf adds it as an active recipe (with all output targets at 0), collapses the picker, and scrolls to the new entry. Recipes already active on this building are grayed out / unclickable in the picker.
   - Arbitrary nesting depth — GDScript renders recursively with increasing left margin per depth level.
   - Smart flattening is applied on the Rust side before sending to GDScript, so UI code doesn't need threshold logic.
4. **Active recipes list.** Each active recipe is a visual section:
   - **Header row:** Recipe name (bold), up/down reorder buttons, remove button (X).
   - **Enabled** toggle (per-recipe, independent of the building-wide toggle).
   - **Per-output target rows:** For each output, a row showing: output name (with material if applicable), current stock count, and target quantity field with +/- buttons. Each row is identified by `ActiveRecipeTargetId` for command targeting.
   - **Auto-logistics section:** Toggle switch. When on, shows spare iterations field with +/- buttons.
   - Visual separator between recipes.

### Summary (on building info panel)

A compact line like: `"Crafting: 3 active recipes (2 satisfied)"` with the "Details..." button.

## Kitchen Migration

The kitchen's current hardcoded bread recipe (`cooking_enabled`, `cooking_bread_target`, `cook_work_ticks`) becomes a standard `RecipeDef` in the catalog:

- `verb: RecipeVerb::Cook`
- `inputs`: whatever the bread recipe currently consumes (fruit)
- `outputs`: `[(ItemKind::Bread, None, 1)]`
- `work_ticks`: value from `config.cook_bread_work_ticks` (renamed from `cook_work_ticks`)
- `furnishing_types`: `[FurnishingType::Kitchen]`
- `category`: `[]` (top-level, no hierarchy needed for a kitchen with few recipes)
- `required_species`: `Some(Species::Elf)`

The `cook_work_ticks` field is renamed to `cook_bread_work_ticks` in `GameConfig` (with a serde alias for old saves). The bread `RecipeDef` reads its `work_ticks` from this config field at catalog build time. The legacy `start_cook_action` reads duration from the recipe catalog.

## Migration Path

### Phase 1: Unified Data Model

1. Add stable `#[repr(u16)]` discriminants to `ItemKind`, `Material`, and `MaterialFilter` enums (same policy as `RecipeVerb`).
2. Add `material_filter: MaterialFilter` field to `RecipeInput` (defaulting to `Any`).
3. Add `RecipeVerb` enum, `RecipeKey` struct, `RecipeDef`, and recipe catalog builder to the sim.
4. Add `ActiveRecipe` and `ActiveRecipeTarget` tables to `SimDb`.
5. Add new `SimAction` commands (including reorder commands).
6. Generalize `process_workshop_monitor()` to iterate `ActiveRecipe` by `sort_order`, use per-output target checking, and pull `required_species` from `RecipeDef`. Update `TaskCraftData` to reference `ActiveRecipeId`.
7. Generalize `compute_recipe_wants()` for auto-logistics with spare iterations and sum-based merging.
8. Migrate existing recipes (bowstring, bow, arrow, bread) to `RecipeDef` entries in the catalog. All four get `required_species: Some(Species::Elf)`, matching the current hardcoded behavior in `process_workshop_monitor()`.
9. Remove old workshop/kitchen fields from `CompletedStructure` and old commands (`SetWorkshopConfig`, `SetCookingConfig`).
10. Handle save compatibility: old saves deserialize with `ActiveRecipe`/`ActiveRecipeTarget` tables defaulting to empty. Existing workshop/kitchen configs are lost — acceptable during early development.

### Phase 2: Unified UI

1. Build the new crafting detail panel in GDScript (shared for all building types).
2. Add bridge methods for recipe catalog queries and active recipe management.
3. Wire the new panel to the new commands.
4. Remove old workshop recipe rows, kitchen controls, and associated signals from `structure_info_panel.gd`.

### Phase 3: Fruit-Variety Recipes (F-fruit-extraction)

1. Extend the catalog builder to generate recipes from fruit species data.
2. Add new `RecipeVerb` variants as needed (Extract, Press, etc.).
3. UI automatically handles the new recipes via the existing data-driven pipeline.

## Open Questions

- **Multiple concurrent craft tasks.** Currently limited to one active craft task per building. Future buildings might support parallel crafting (multiple elves working). Out of scope for this feature.
