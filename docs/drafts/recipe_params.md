# F-recipe-params: Parameterized Recipe Templates

## Summary

Replace the procedurally generated `RecipeCatalog` (hundreds of concrete
`RecipeDef` entries keyed by `RecipeKey`) with a fixed `Recipe` enum.
Each variant is a recipe template that knows its parameter types, input/output
patterns, and tuning values (from `GameConfig`). A "configured recipe" is a
`Recipe` variant + parameter bindings. The catalog, key, and def types are
deleted entirely.

## Motivation

The current system generates a separate `RecipeDef` for every material variant:
10 fruit species × ~8 processing recipes = ~80 recipes, plus 5 wood types × 8
equipment recipes = 40 more, plus config recipes. Dye colors and future recipe
types would cause combinatorial explosion. Parameterization keeps the set of
recipe *templates* small and fixed, with material (and later dye color, batch
size) chosen at configuration time.

## Design

### Recipe enum

A fixed, stable enum. Each variant is one recipe template. Serialized via
serde's default JSON representation (variant name as string), so variant
**names** are the save-file contract — never rename. Discriminant values
(`#[repr(u16)]`) are for in-memory representation only. Never reorder or
reuse discriminants; never rename variants.

```rust
#[repr(u16)]
pub enum Recipe {
    // --- Fruit processing (material param: FruitSpecies) ---
    Extract         = 0,  // Fruit → components (outputs vary by species)
    Mill            = 1,  // Starchy component → Flour
    Bake            = 2,  // Flour → Bread
    Spin            = 3,  // Fine-fiber component → Thread
    Twist           = 4,  // Coarse-fiber component → Cord
    Weave           = 5,  // Thread → Cloth
    Press           = 6,  // Pigmented component → Dye

    // --- Assembly (material param: FruitSpecies) ---
    AssembleThreadBowstring  = 7,   // Thread → Bowstring
    AssembleCordBowstring    = 8,   // Cord → Bowstring

    // --- Clothing (material param: FruitSpecies) ---
    SewTunic        = 9,
    SewLeggings     = 10,
    SewBoots        = 11,
    SewHat          = 12,
    SewGloves       = 13,

    // --- Wood equipment (material param: wood type) ---
    GrowBow         = 14, // Bowstring(any) → Bow
    GrowArrow       = 15, // (no input) → Arrows
    GrowHelmet      = 16,
    GrowBreastplate = 17,
    GrowGreaves     = 18,
    GrowGauntlets   = 19,
    GrowBoots       = 20,

    // Future (F-dye-application): DyeTunic, DyeLeggings, etc.
    // Future (F-dye-mixing): MixDye
}
```

**Variant granularity:** Each variant is one *specific transformation* — not
one per verb. `SewTunic` and `SewLeggings` are separate variants even though
they share the Sew verb, because they have different inputs/outputs/work ticks.
This keeps resolution simple (no secondary "output item" parameter) and makes
it trivial to restrict which recipes are valid for a given furnishing type.

**Legacy bread recipe dropped.** The old `Cook` recipe (Fruit → Bread directly)
is removed. Bread production now goes exclusively through the component chain:
Extract → Mill → Bake. The `TaskKind::Cook` / `start_cook_action` /
`resolve_cook_action` legacy code path is deleted.

### RecipeVerb

`RecipeVerb` is currently used only as a field in `RecipeKey`. With `RecipeKey`
gone, `RecipeVerb` has no structural role. However, it's useful for UI display
(grouping/categorization in the recipe picker tree).

**Decision: Keep as a derived property.** `Recipe::verb()` returns a
`RecipeVerb`. Used for UI grouping only, not identity.

Unused `RecipeVerb` variants (`Brew`, `Fletch`, `Husk`, `Cook`) are pruned.
The remaining variants are: `Assemble`, `Extract`, `Mill`, `Bake`, `Spin`,
`Twist`, `Weave`, `Sew`, `Grow`, `Press`.

### Parameters

In the initial implementation, all recipes require a specific material —
`material` is always `Some(m)`. "Any" material support (where the crafting
action propagates the input's material to the output) is deferred to
F-recipe-any-mat.

```rust
/// Parameter bindings for a configured recipe instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeParams {
    /// Material selection. Always `Some(m)` in the initial implementation.
    /// F-recipe-any-mat adds `None` = "any" support.
    pub material: Option<Material>,

    // Future (F-dye-palette): pub dye_color: Option<DyePaletteId>,
    // Future (F-batch-craft): pub batch_size: Option<u32>,
}
```

Each `Recipe` variant declares which parameters it supports and what materials
are valid:

```rust
impl Recipe {
    /// Whether this recipe has a material parameter.
    /// All current recipes return true.
    pub fn has_material_param(&self) -> bool { ... }

    /// Valid materials for this recipe given the current world state.
    /// Fruit-processing recipes: all known FruitSpecies materials (filtered
    /// by whether the species has the required part property).
    /// Wood recipes: Material::WOOD_TYPES.
    pub fn valid_materials(&self, fruit_species: &[FruitSpecies]) -> Vec<Material> { ... }

    /// Which species can perform this recipe. All current recipes are elf-only.
    pub fn required_species(&self) -> Option<Species> {
        Some(Species::Elf) // all recipes require elves for now
    }
}
```

### Resolution: Recipe + Params → concrete inputs/outputs

A `Recipe` variant + `RecipeParams` + world data (fruit species, game config)
resolves to concrete inputs and outputs. This replaces what the catalog builder
does today, but happens on-demand instead of at startup.

```rust
pub struct ResolvedRecipe {
    pub inputs: Vec<RecipeInput>,
    pub outputs: Vec<RecipeOutput>,
    pub work_ticks: u64,
    pub subcomponent_records: Vec<RecipeSubcomponentRecord>,
}

impl Recipe {
    /// Resolve this recipe template with the given parameters into concrete
    /// inputs and outputs.
    ///
    /// For most recipes this is straightforward substitution: replace the
    /// material placeholder with the bound material. For Extract, the outputs
    /// depend on the fruit species' part composition (looked up from
    /// `fruit_species`).
    ///
    /// Returns `None` if the params are invalid for this recipe (e.g., wrong
    /// material category, species lacks required property).
    pub fn resolve(
        &self,
        params: &RecipeParams,
        config: &GameConfig,
        fruit_species: &[FruitSpecies],
    ) -> Option<ResolvedRecipe> { ... }
}
```

**Extract special case:** Extract outputs (which component items, what
quantities) depend on the species' part composition. `resolve()` looks up the
fruit species data and generates outputs accordingly — same logic as the
current `build_extraction_recipe()`, just done at resolve time.

**Press special case:** Only species with pigmented parts are valid.
`valid_materials()` filters accordingly. The resolved output carries the
pigment's `dye_color`.

**Bowstring recipes:** Input/output quantities and work ticks are hardcoded
on the enum variant (not configurable via GameConfig). These are simple
recipes (thread/cord → bowstring) that don't need tuning knobs.

### Display metadata

```rust
impl Recipe {
    /// Human-readable name, incorporating material if bound.
    /// e.g., Recipe::Mill with material=Shinethúni → "Mill Shinethúni Pulp"
    /// e.g., Recipe::GrowBow with material=Oak → "Grow Oak Bow"
    pub fn display_name(&self, params: &RecipeParams, fruit_species: &[FruitSpecies]) -> String;

    /// Category path for UI hierarchy.
    /// e.g., Recipe::Mill → ["Processing", "Milling"]
    pub fn category(&self) -> Vec<&'static str>;

    /// Which furnishing types can use this recipe.
    pub fn furnishing_types(&self) -> Vec<FurnishingType>;
}
```

### ActiveRecipe table changes

Current schema:
```
active_recipes:
    id: ActiveRecipeId (PK)
    structure_id: StructureId
    recipe_key_json: String       ← deleted
    recipe_display_name: String   ← deleted (derived on the fly)
    enabled: bool
    sort_order: u32
    auto_logistics: bool
    spare_iterations: u32
```

New schema:
```
active_recipes:
    id: ActiveRecipeId (PK)
    structure_id: StructureId
    recipe: Recipe                ← enum variant
    material: Option<Material>    ← material binding
    enabled: bool
    sort_order: u32
    auto_logistics: bool
    spare_iterations: u32
```

`recipe_key_json` (opaque JSON string) is replaced by the `Recipe` enum +
`material` field. This is smaller, type-safe, and doesn't require JSON
round-tripping. The `recipe_display_name` cached field is dropped — the display
name is cheap to derive from `Recipe + material + fruit species`.

**Duplicate detection:** Currently compares `recipe_key_json` strings. New
system compares `(recipe, material)` tuples — structurally identical, simpler.

**ActiveRecipeTarget** is unchanged — it still stores per-output target
quantities.

### TaskCraftData and TaskKind changes

The current `TaskKind::Craft` stores a `recipe_id: String` (legacy ID derived
from `recipe_key_to_legacy_id()`). `TaskCraftData` stores `recipe_id: String`
and `active_recipe_id: Option<ActiveRecipeId>`.

New `TaskCraftData`:
```rust
pub struct TaskCraftData {
    pub recipe: Recipe,
    pub material: Option<Material>,
    pub active_recipe_id: ActiveRecipeId,
    // ... existing fields: reserved inputs, output targets, etc.
}
```

Both `Recipe + Material` (for resolution via `recipe.resolve()`) and
`active_recipe_id` (for interruption cleanup / target updates) are stored.
The recipe+material are needed because `resolve_craft_action()` must know
concrete inputs/outputs to consume/produce items, and the ActiveRecipe row
may have been deleted by interruption before resolution completes.

- `TaskKind::Craft` stores `active_recipe_id: ActiveRecipeId` (the monitor
  passes the rest via `TaskCraftData`).
- `recipe_key_to_legacy_id()` is deleted.
- `start_craft_action()` and `resolve_craft_action()` use `recipe.resolve()`
  to get concrete inputs/outputs instead of catalog lookup.

The legacy `TaskKind::Cook` variant and its `start_cook_action()` /
`resolve_cook_action()` code paths are deleted (legacy bread recipe removed).

### Cook removal blast radius

Deleting `TaskKind::Cook` touches several files beyond crafting.rs:
- `db.rs`: `TaskKindTag::Cook` variant and `TaskStructureRole::CookAt` — both
  deleted. `ActionKind::Cook = 4` — deleted, discriminant slot reserved.
- `preemption.rs`: `TaskKindTag::Cook` in priority mapping and tests — removed.
- `sim/activation.rs`: `TaskKindTag::Cook` dispatch arm — removed.
- `sim/creature.rs`: `TaskKindTag::Cook` cleanup dispatch — removed.
- `sim/crafting.rs`: Monitor's active-task guard currently checks for both
  `CookAt` and `CraftAt` roles — simplified to just `CraftAt`.

### SimAction command changes

`SimAction::AddActiveRecipe` currently takes `recipe_key: RecipeKey`. Updated
to `recipe: Recipe, material: Option<Material>`. Other recipe-related
`SimAction` variants (`RemoveActiveRecipe`, `SetRecipeEnabled`, etc.) are
unchanged — they operate on `ActiveRecipeId`.

### Save format migration

Old saves have `recipe_key_json` strings in `active_recipes` rows and legacy
`TaskKind::Cook`/`Craft { recipe_id: String }` variants in task data. On load,
`rebuild_transient_state()` drops all `ActiveRecipe` rows that use the old
schema (they won't deserialize as the new schema). No attempt to map old keys
to new Recipe+Material — recipes are easy for the player to re-add. In-flight
Cook/Craft tasks with old-format data are similarly dropped.

The orphan notification system (`cleanup_orphaned_active_recipes`) is removed.
It was needed when the catalog could change between saves (species
added/removed); with the fixed enum, recipes don't become orphans — they
either deserialize or they don't (schema migration).

### Crafting monitor changes

`process_unified_crafting_monitor()` currently:
1. Gets active recipes for a building
2. Looks up `RecipeDef` in catalog via `recipe_key_json`
3. Uses the def's inputs/outputs for reservation and execution

New flow:
1. Gets active recipes for a building
2. Calls `recipe.resolve(params, config, fruit_species)` to get concrete
   inputs/outputs
3. Uses the resolved recipe for reservation and execution

The resolve call is cheap (pattern match + config lookup + optional species
lookup). No caching needed.

**Auto-logistics** (`compute_effective_wants()`) follows the same pattern —
it currently does `RecipeKey::from_json()` + `recipe_catalog.get()` to
resolve input requirements. Updated to use `recipe.resolve()` instead.

### Auto-add on furnish

**Removed.** The current auto-add behavior (adding certain recipes when a
building is furnished) was a testing convenience, not a design requirement.
With parameterized recipes requiring material selection, auto-add would need
to either pick a material or leave it as "any" — neither is a good default.
Players manually add all recipes.

The `auto_add_on_furnish` field on `RecipeDef` and
`default_recipes_for_furnishing()` on `RecipeCatalog` are deleted.
`construction.rs`'s furnishing code that calls this is simplified.

### GDScript bridge changes

**Catalog retrieval** — currently `get_recipe_catalog_for_building()` returns
an array of recipe dicts from the catalog. New approach: the bridge returns a
list of `Recipe` enum variants valid for the building's furnishing type, with
display metadata. Material options are fetched separately per recipe.

```
get_available_recipes(structure_id) → [
    { recipe: 0, display_name: "Extract", category: [...], has_material_param: true },
    { recipe: 1, display_name: "Mill", category: [...], has_material_param: true },
    ...
]

get_valid_materials(recipe: int) → [
    { material_json: "...", display_name: "Shinethúni" },
    { material_json: "...", display_name: "Vaelora" },
    ...
]
```

**Adding a recipe** — currently `add_active_recipe(structure_id, recipe_key_json)`.
New: `add_active_recipe(structure_id, recipe_variant: int, material_json: String)`.

**Structure info** — `get_structure_info()` returns active recipes with the
`Recipe` variant int instead of `recipe_key_json`. Display name is computed
by the bridge.

### GDScript UI changes

The recipe picker flow changes from:
1. Show hierarchical catalog of all concrete recipes
2. Player clicks to add

To:
1. Show hierarchical list of recipe templates (much smaller list)
2. Player clicks a template
3. If template has material param: show material picker (dropdown or list)
   - Lists all valid specific materials from `get_valid_materials()`
4. Player selects material → recipe added as active

The active recipe row display remains similar but shows the resolved display
name (recipe + material).

### What gets deleted

- `RecipeKey` struct and all its methods
- `RecipeDef` struct
- `RecipeCatalog` struct and `build_catalog()` + all builder helpers
- `SimState::recipe_catalog` field
- `recipe_key_json` / `recipe_display_name` fields on `ActiveRecipe`
- `recipe_key_to_legacy_id()` and the legacy `recipe_id: String` in task data
- `TaskKind::Cook` and `start_cook_action()` / `resolve_cook_action()`
- `build_bread_recipe()` (legacy Fruit → Bread)
- Config `Recipe` struct and `GameConfig::recipes` Vec (bowstring config)
- `GameConfig::cook_fruit_input` and `GameConfig::cook_bread_output` fields
  (and their entries in `data/config.json`)
- `auto_add_on_furnish` field and `default_recipes_for_furnishing()`
- `cleanup_orphaned_active_recipes()` and its notifications
- `TaskKindTag::Cook`, `TaskStructureRole::CookAt`, `ActionKind::Cook`
  (reserve discriminant slots on ActionKind)
- Cook-related arms in `preemption.rs`, `activation.rs`, `creature.rs`
- All GDScript code that handles `key_json` as opaque strings
- Unused `RecipeVerb` variants (`Brew`, `Fletch`, `Husk`, `Cook`)
- `RecipeKey` references in `inventory.rs` doc comments (update to reference
  `Recipe` enum)

### What stays

- `RecipeInput`, `RecipeOutput`, `RecipeSubcomponentRecord` in config.rs —
  still used for resolution and config tuning values
- `ComponentRecipeConfig`, `GrowRecipeConfig` in config.rs — tuning values
  for work ticks, input/output quantities
- `RecipeVerb` — pruned to active variants, used for UI grouping via
  `Recipe::verb()`
- `ActiveRecipeTarget` table — unchanged
- The crafting monitor's core loop — structurally similar, just resolves
  differently

### Test impact

Existing recipe-related tests in `sim/tests.rs` (duplicate detection,
furnishing-type filtering, auto-add, recipe removal with task interruption,
~10+ tests) all use `RecipeKey` and `RecipeCatalog` and will need rewriting
against the new `Recipe` enum + `resolve()` API.

GDScript tests in `structure_info_panel` tests that interact with recipe
key_json will need updating for the new two-step template+material picker flow.

## Resolved Questions

1. **"Any" material** — Deferred to F-recipe-any-mat. Initial implementation
   requires specific material on all recipes.

2. **Extract output targets** — Not a problem: Extract always requires a
   specific species, so outputs are known when the recipe is added and
   `ActiveRecipeTarget` rows can be created normally.

3. **Flat enum vs verb+sub-parameter** — Flat enum. The variant count is
   manageable (~21 now, ~35 with future dye recipes). Keeps resolution simple.

4. **Config recipe removal** — `GameConfig::recipes` is deleted. Bowstring
   assembly recipes have hardcoded quantities on the enum variants. Other
   recipe tuning stays in `ComponentRecipeConfig` and `GrowRecipeConfig`.
