# Item Schema & Manufacturing System

Design document for the expanded item schema, recipe system, and workshop
manufacturing pipeline.

## Item Schema

### Core Fields

Every `ItemStack` row (in `db.rs`) has:

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `kind` | `ItemKind` | — | What the item is (Bread, Fruit, Bow, Arrow, Bowstring) |
| `quantity` | `u32` | — | Stack count |
| `material` | `Option<Material>` | `None` | Optional material variant (Oak, Birch, etc.) |
| `quality` | `i32` | `0` | Quality score (0 = default, positive = better) |
| `enchantment_id` | `Option<EnchantmentId>` | `None` | FK to shared enchantment instance |
| `owner` | `Option<CreatureId>` | `None` | Creature who owns this stack |
| `reserved_by` | `Option<TaskId>` | `None` | Task that has reserved this stack |

### Stackability

Two item stacks merge if and only if **all** of these match:
- `kind`
- `material`
- `quality`
- `enchantment_id`
- `owner`
- `reserved_by`

This means a quality-3 Oak Bow stacks separately from a quality-0 Oak Bow.

### Material Enum

```rust
enum Material { Oak, Birch, Willow, Ash, Yew }
```

Materials are optional — Bread, Fruit, Bowstring, and Arrow have `material: None`.
Future items (furniture, tools) will use materials for visual/stat variation.

### Enchantment Hierarchy

Three-tier schema for future magic items:

1. **ItemStack** → `enchantment_id: Option<EnchantmentId>` (FK to `ItemEnchantment`)
2. **ItemEnchantment** — a shared enchantment instance (stubbed for now)
3. **EnchantmentEffect** → `enchantment_id` FK (cascade) — individual effects

Multiple stacks can share one enchantment (e.g., a batch of arrows all getting
the same fire enchantment). Effects are cascaded when the enchantment is deleted.

`EffectKind` is stubbed as `Placeholder` for now.

### Subcomponents

`ItemSubcomponent` records what went into crafting an item:

| Field | Type | Purpose |
|-------|------|---------|
| `item_stack_id` | FK → `item_stacks` (cascade) | Parent item |
| `component_kind` | `ItemKind` | What was used |
| `material` | `Option<Material>` | Material of the component |
| `quality` | `i32` | Quality of the component |
| `quantity_per_item` | `u32` | How many per parent item |

When a Bow is crafted with a Bowstring, the Bow's `ItemStack` gets an
`ItemSubcomponent` row recording the Bowstring. This enables future UI
(inspect item → see components) and quality inheritance.

## Recipe System

Recipes are data-driven via `GameConfig`:

```rust
struct Recipe {
    id: String,
    display_name: String,
    inputs: Vec<RecipeInput>,
    outputs: Vec<RecipeOutput>,
    work_ticks: u64,
    subcomponent_records: Vec<RecipeSubcomponentRecord>,
}
```

### Default Recipes

| Recipe | Inputs | Outputs | Work Ticks | Subcomponents |
|--------|--------|---------|------------|---------------|
| `bowstring` | 1 Fruit | 20 Bowstring | 5000 | — |
| `bow` | 1 Bowstring | 1 Bow | 8000 | Bowstring ×1 |
| `arrow` | (none) | 20 Arrow | 3000 | — |

## Workshop Flow

1. **Furnish** a building as Workshop → sets `workshop_enabled = true`,
   `workshop_recipe_ids` to all recipe IDs, logistics priority, and
   logistics wants computed from configured recipes' inputs.

2. **Workshop monitor** (runs each logistics heartbeat after kitchen monitor):
   - For each enabled workshop with no active Craft task:
     - Find first configured recipe whose inputs are available (unreserved)
     - Reserve inputs, create Craft task

3. **Craft task** (mirrors Cook):
   - Creature walks to workshop, increments progress each tick
   - On completion: consume reserved inputs, produce outputs, record subcomponents

4. **SetWorkshopConfig** command: update enabled state and recipe selection,
   recompute logistics wants.

## Future Directions

- **Quality inheritance:** output quality derived from input qualities + crafter skill
- **Material propagation:** recipe outputs could inherit material from inputs
- **Enchanting workflow:** separate enchanting station that creates `ItemEnchantment`
  rows and applies them to item stacks
- **Crafting skill:** creature skill levels affecting quality and speed
- **Recipe discovery:** unlock recipes through gameplay progression
