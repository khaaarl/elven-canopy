# F-fruit-variety — Procedural Fruit System Draft (v6)

Design draft for procedural fruit generation: every game world generates a
unique roster of fruit species from composable traits, guaranteeing
coverage of all gameplay-critical categories while making every playthrough
botanically distinct.

## Core Concept

Fruits are not a fixed enum. Each game world procedurally generates 20-40+
fruit species during worldgen. A fruit species is assembled from **parts**
(flesh, rind, seeds, fiber, sap, resin) and each part carries
**properties** (starchy, sweet, fibrous, luminescent, etc.).
Processing paths emerge from part properties rather than being hardcoded
per fruit. Names are generated via Vaelith phonotactics with morphemes
derived from the fruit's dominant properties.

This is the DF approach: every world has its own plump helmets, but the
specifics — names, properties, processing chains — are unique.

---

## Fruit Anatomy

Each `FruitSpecies` has 1-4 **parts**. A part is a physically separable
component of the fruit.

### Part Types

| Part Type | Description | Typical Properties |
|-----------|-------------|-------------------|
| Flesh     | The bulk interior tissue | Starchy, Sweet, Oily, Bland, Bitter |
| Rind      | Outer skin or shell | Aromatic, Tough, Luminescent (often pigmented) |
| Seed      | Interior kernel or pit | Oily, Bitter, ManaResonant |
| Fiber     | Structural threads/husk | Fibrous (Coarse or Fine) |
| Sap       | Liquid interior | Sweet, Fermentable, Luminescent, Psychoactive |
| Resin     | Thick, sticky exudate | Aromatic, Adhesive, ManaResonant |

A fruit always has at least one part. Most have 2-3. The generator assigns
each part a `yield_percent` (integer 1-100, how much of the fruit's mass
it represents), which affects output quantities when processing. A
fruit's parts must have `yield_percent` values that sum to exactly 100 —
they represent a physical mass budget, not independent weights. The
generator allocates 100 points across parts (e.g., a fruit with Flesh 60
/ Rind 25 / Seed 15 sums to 100). All numeric fruit data uses integers
to avoid floating-point determinism issues across platforms and in JSON
serialization.

### Part Properties

Properties are flags on a part, not mutually exclusive across parts but
subject to exclusion rules within a single part.

**Food properties:**
- `Starchy` — can be milled into flour for bread
- `Sweet` — palatable raw, fermentable, satisfying
- `Oily` — calorie-dense, cooking ingredient, preservative base
- `Bland` — edible but unsatisfying
- `Bitter` — inedible (or only edible when cooked/processed)

**Material properties:**
- `FibrousCoarse` — tough fibers for bowstrings, rope, fletching
- `FibrousFine` — soft fibers for thread → cloth → clothing
- `Tough` — the part is hard/structural (shells, thick rinds)

**Pigment (separate from properties):**
- Pigment is represented by the `pigment: Option<DyeColor>` field on
  `FruitPart`, not as a property flag. A part is "pigmented" when its
  pigment field is `Some`.
- Primary colors: Red, Yellow, Blue (mixable at a dye workshop to produce
  any hue). Plus Black and White for shade/tint control.
- A part can only carry one pigment color

**Chemical properties:**
- `Fermentable` — can be fermented into alcoholic beverages
- `Aromatic` — can be dried and burned as incense, or distilled
- `Luminescent` — glows; can be processed into lighting oil or alchemical reagent
- `Psychoactive` — mood-altering when consumed (type determines effect)
- `Medicinal` — healing/curative when processed into poultice or tonic
- `ManaResonant` — the core enchanting/alchemy property
- `Stimulant` — energy burst + crash when consumed

### Within-Part Exclusion Rules

These property pairs cannot coexist on the same part:

- `Starchy` / `FibrousCoarse` / `FibrousFine` — structurally different tissue
- `Sweet` / `Bitter` — contradictory flavor
- `Luminescent` / pigmented (non-None pigment) — extracting one destroys the other
- `Psychoactive` / `Stimulant` — opposing neurological effects
- A part can have at most one of: `Starchy`, `Sweet`, `Oily`, `Bland`, `Bitter`
  (these are flavor/texture categories, not combinable)

These exclusions are within a single part. Across parts of the same fruit,
any combination is valid. A fruit with bitter pigmented juice and sweet
starchy flesh is perfectly natural.

---

## Processing Model

### Step 1: Separation

The first processing step takes a whole fruit and separates it into its
constituent parts. This happens at a **processing workshop**. A fruit with
3 parts yields up to 3 distinct intermediate materials, each carrying the
properties of their source part.

The separation method is flavor (press, husk, crack, ret) but mechanically
uniform in the initial implementation. Different separation methods could
become mechanically distinct later (requiring different workshops or tools).

### Step 2: Transformation

Each separated part can then be transformed based on its properties. These
are the core processing chains:

**Food chains:**
- `Starchy part` → Mill → Flour → Bake → Bread
- `Sweet/Oily/Bland part` → (edible raw, or use in cooking recipes)
- `Bitter part` → Cook (with other ingredients) → edible dish (bitterness
  removed by cooking, adds complexity/quality)

**Fiber chains:**
- `FibrousCoarse part` → Twist → Cord → {Bowstring, Rope, Fletching binding}
- `FibrousFine part` → Spin → Thread → Weave → Cloth → {Clothing items}
- `FibrousFine part` → Spin → Thread → {Bowstring (inferior), Embroidery}

**Dye chains:**
- Pigmented part → Press/Boil → Dye(color)
- Dye + Cloth → Dyed Cloth(color) (at a dye workshop)
- Dye(Red) + Dye(Yellow) → Dye(Orange), etc. at a dye workshop (mixing)
- Dye + Dye(White) → lighter tint, Dye + Dye(Black) → darker shade
- Worldgen guarantees enough primary pigment fruits to mix any color.
  Secondary colors (Orange, Green, Violet) are only produced by mixing,
  never directly from fruit parts

**Fermentation chains:**
- `Fermentable part` (usually Sweet sap/juice) → Vat → Wine/Mead
- Wine → Distill → Spirit (stronger effects)
- Fermentation preserves pigment (purple berry wine is purple)
- Fermentation destroys `Medicinal` and `Luminescent`

**Medicinal chains:**
- `Medicinal part` → Mortar → Poultice (applied to wounds)
- `Medicinal part` → Brew → Tonic (consumed for ailment cure)

**Aromatic chains:**
- `Aromatic part` → Dry → Incense (burned for area mood/mana effect)
- `Aromatic part` → Distill → Essential oil (perfume, alchemy ingredient)

**Luminescent chains:**
- `Luminescent part` → Distill → Luminous oil (lantern fuel, greenhouse
  supercharger)
- `Luminescent part` → (raw) → Glow-fruit (temporary light source as-is)
- Luminous oil is also an alchemical reagent

**Drug/intoxicant chains:**
- `Psychoactive part` → (consume raw) → Trance effect (blocks work, high
  mood, possible mana generation)
- `Psychoactive part` → Brew → Dream draught (controlled dose, shorter
  trance, used ceremonially)
- `Stimulant part` → (consume raw) → Speed burst + crash
- `Stimulant part` → (use as cooking ingredient) → Spiced food (quality
  bonus, no drug effect — cooking neutralizes the stimulant intensity
  while preserving the flavor). Consumes the whole part like any recipe.

**Alchemical chains:**
- `ManaResonant part` → Refine → Mana essence (base enchanting reagent)
- Mana essence + Luminous oil → Enchantment fuel
- Mana essence + other alchemical ingredients → various enchantments
- The alchemy system design is mostly deferred; the fruit system just
  needs to produce the raw materials

### Processing Choices (Branching Paths)

Most parts have one obvious processing path. A few interesting cases have
**branching** — the player chooses one path, foreclosing the other:

- **Sweet + pigmented juice:** Ferment into pigmented wine, OR press for
  dye. Can't do both from the same batch (fermentation alters the pigment
  concentration, pressing discards the sugar).
- **Fermentable + Medicinal sap:** Brew into medicinal tonic, OR ferment
  into a drink. Fermentation destroys the medicinal compounds.
- **Aromatic + ManaResonant resin:** Burn as powerful incense (mood+mana
  area effect), OR refine for alchemical essence. The burning consumes it.
- **FibrousCoarse husk with Bitter mash residue:** Spin for cord, and the
  leftover bitter mash can be fermented into rough spirit. (This is
  multi-output, not branching — you get both.)

### Quality Propagation

Fruit quality (determined by tree health, greenhouse level, growing
conditions) propagates through the processing chain. Higher quality fruit →
higher quality parts → higher quality outputs. The exact quality model is
being designed in parallel (item schema work); the fruit system just needs
to carry quality through each transformation step.

---

## Worldgen: Coverage Constraints

The fruit generator runs during world generation. It must guarantee that
the world contains enough fruit species to cover all gameplay-critical
needs. The algorithm:

1. Define **coverage requirements** (minimums per category)
2. Generate fruits one at a time
3. After each fruit, check remaining coverage gaps
4. Bias the next fruit's generation toward filling gaps
5. Once all minimums are met, generate additional "bonus" fruits with
   random properties for variety
6. Assign growth habitats (tree branch, trunk surface, ground bush, etc.)

### Coverage Requirements (Minimums)

| Category | Minimum | Notes |
|----------|---------|-------|
| Starchy (bread-making) | 3 | At least one must be common/easy |
| Sweet (fresh eating) | 3 | |
| FibrousCoarse | 2 | Bowstrings, rope |
| FibrousFine | 2 | Clothing |
| Pigment (primaries) | 3 | Red, Yellow, Blue — all three required. Mixing at a dye workshop produces secondary colors. |
| Pigment (Black) | 1 | For darkening/shading dyes |
| Pigment (White) | 1 | For lightening/tinting dyes |
| Fermentable | 3 | Often co-occurs with Sweet |
| Medicinal | 2 | |
| Aromatic | 2 | |
| Luminescent | 2 | Lighting + greenhouse fuel |
| Psychoactive | 1 | |
| Stimulant | 1 | |
| ManaResonant | 2 | Core enchanting input |

Many of these overlap (a sweet fermentable fruit with blue pigment juice
covers three categories at once), so 20-30 total fruits typically satisfies
all constraints with room for interesting bonus species.

### Rarity and Habitat

Each fruit gets a rarity tier (Common, Uncommon, Rare) and a growth
habitat:

**Habitats:**
- **Branch** — grows on tree branches (requires Leaf voxels). Most common.
- **Trunk** — grows on trunk surface. Less common, different visual.
- **Ground bush** — grows on forest floor bushes. Requires foraging
  expeditions.
- **Wild tree** — only found on NPC/wild trees. Must be discovered through
  exploration.

**Rarity affects:**
- Spawn frequency (how often the tree/bush produces this fruit)
- Greenhouse difficulty (future: mana cost to cultivate)
- How many worlds it appears in (common fruits appear in every world; rare
  ones might not)

**Wild-only fruits (future):** Some fruits (especially powerful alchemical
or drug fruits) could eventually be exclusive to wild trees/bushes and
**not greenhousable**, creating reasons to explore and defend territory.
This requires exploration and territory systems that don't exist yet.
Initial implementation: all generated fruits are domesticated/greenhousable.

---

## Greenhouses

A greenhouse is a building that cultivates a single fruit species, chosen
from the civilization's researched fruits when the greenhouse is furnished.

### Initial Implementation

- Furnishing a greenhouse assigns it a fruit species
- The greenhouse produces that fruit periodically (production rate TBD,
  possibly tied to building size)
- No input costs initially (future: mana, water, light fuel)
- Changing the fruit species requires re-furnishing (not instant — the
  player commits to a choice)

### Greenhouse Fuel (Future)

Luminous oil (distilled from luminescent fruit parts) can be used to
supercharge greenhouses: more light → faster growth → higher yield or
quality. This creates an infrastructure loop:

1. Dedicate one greenhouse to growing a luminescent fruit
2. Process the fruit into luminous oil
3. Use the oil to boost other greenhouses
4. Those greenhouses produce more/better fruit
5. Some of that fruit feeds back into oil production

This is a classic optimization puzzle — how much greenhouse capacity do you
dedicate to fuel production vs. direct output?

### Research and Starting Knowledge

A civilization starts knowing how to grow 4-5 fruit species:
- 1-2 starchy bread fruits (food security)
- 1 fiber fruit (basic clothing)
- 1 dye fruit (minimum self-expression)
- 1 luminescent or edible fruit (lighting or food variety)

Additional fruits are discovered through:
- **Exploration** — finding wild fruits in the forest
- **Trade** — acquiring seeds/knowledge from other civilizations (future)
- **Greenhouse experimentation** — attempting to cultivate unknown seeds
  (future)

Not every discovered fruit can be greenhoused. Some resist cultivation
(wild-only). The player learns this through failed greenhouse attempts or
through lore/knowledge systems.

---

## Vaelith Naming

Fruit names are generated via the lang crate's phonotactic system. Each
fruit gets a Vaelith name (primary, shown in all UI) and an English gloss
(secondary, shown in parentheses on tooltips and the elfcyclopedia).

### Name Generation Strategy

Fruit names are **descriptive compounds** built from botanical morphemes.
The lexicon needs new root words for:

**Textures/forms:** fibrous, smooth, spiky, soft, hard, dry, wet, round,
long, clustered

**Flavors:** sweet, bitter, sharp, rich, bland

**Colors:** red, orange, yellow, green, blue, violet, dark, pale, bright

**Botanical terms:** fruit, pod, berry, nut, gourd, blossom, vine, bulb

**Effects:** glowing, dreaming, burning, healing, singing (for
mana-resonant)

Names combine 1-2 morphemes: a dominant-property root + an optional
form/color modifier. The generator picks morphemes based on the fruit's
most notable properties, producing names that hint at function. Players who
learn Vaelith vocabulary can start guessing what a new fruit does from its
name — a lovely emergent literacy mechanic that pairs with the elves
already singing in Vaelith.

### Morpheme-to-Property Mapping (Sketch)

The following table shows example mappings from fruit properties to
Vaelith morphemes. Most morphemes are general Vaelith vocabulary (the
word for "red" is used everywhere, not just for fruit naming). This is
not exhaustive — it illustrates the pattern for implementation.

| Property / Trait | Morpheme Root | Gloss | Notes |
|-----------------|---------------|-------|-------|
| Starchy | grain-related | "grain" / "earth-food" | General agricultural term |
| Sweet | honey/nectar-related | "sweet" / "nectar" | Also used for pleasant flavors in cooking |
| Bitter | thorn/bile-related | "bitter" / "sharp-taste" | General flavor word |
| Luminescent | light-related | "glow" / "star-light" | Also used for actual light sources |
| FibrousCoarse | thorn/spike-related | "cord" / "spine" | Connotes toughness, also used for rope |
| FibrousFine | thread-related | "thread" / "silk" | Also used for weaving, spider-silk |
| ManaResonant | song/vibration-related | "singing" / "resonance" | Core magical vocabulary |
| Psychoactive | dream-related | "dream" / "vision" | Also used in ritual/ceremony contexts |
| Medicinal | healing-related | "mending" / "balm" | General medical vocabulary |
| Pigmented (Red) | red color word | "red" / "blood" | Standard Vaelith color term |
| Pigmented (Blue) | blue color word | "blue" / "sky" | Standard Vaelith color term |
| Pod shape | pod/husk-related | "pod" / "sheath" | Botanical form term |
| Clustered shape | cluster-related | "cluster" / "many-seed" | Botanical form term |

The name generator selects 1-2 morphemes based on the fruit's most
notable properties (rarest or most gameplay-relevant), combines them
according to Vaelith phonotactic rules, and produces both the Vaelith
name and the English gloss.

### Display

- Inventory slots: `thalori` (Vaelith name only, with icon)
- Tooltips: `thalori (glow-berry)` — Vaelith name + English gloss
- Elfcyclopedia/research UI: full description with properties, processing
  paths, habitat, rarity
- Recipe UI: Vaelith names with property icons

---

## Data Model (Sketch)

```rust
/// A procedurally generated fruit species.
struct FruitSpecies {
    /// Unique ID for this fruit species in this world.
    id: FruitSpeciesId,
    /// Generated Vaelith name.
    vaelith_name: String,
    /// English gloss (e.g., "glow-berry").
    english_gloss: String,
    /// The separable parts of this fruit.
    parts: Vec<FruitPart>,
    /// Where this fruit grows.
    habitat: GrowthHabitat,
    /// How common this fruit is.
    rarity: Rarity,
    /// Whether this fruit can be grown in a greenhouse.
    greenhouse_cultivable: bool,
    /// Visual appearance hints for rendering.
    appearance: FruitAppearance,
}

/// A physically separable component of a fruit.
struct FruitPart {
    /// What kind of part this is.
    part_type: PartType,
    /// Properties of this part (determines processing paths).
    properties: BTreeSet<PartProperty>,
    /// If pigmented, what color.
    pigment: Option<DyeColor>,
    /// Percentage of the fruit's mass this part represents (1-100).
    /// All parts of a fruit must sum to exactly 100.
    /// Affects output quantity when processing.
    yield_percent: u8,
}

enum PartType {
    Flesh,
    Rind,
    Seed,
    Fiber,
    Sap,
    Resin,
}

enum PartProperty {
    // Food
    Starchy,
    Sweet,
    Oily,
    Bland,
    Bitter,
    // Material
    FibrousCoarse,
    FibrousFine,
    Tough,
    // Chemical
    Fermentable,
    Aromatic,
    Luminescent,
    Psychoactive,
    Medicinal,
    ManaResonant,
    Stimulant,
    Adhesive,
    // Pigment is NOT a property flag — it is represented by the
    // `pigment: Option<DyeColor>` field on FruitPart. A part is
    // "pigmented" when pigment.is_some().
}

enum DyeColor {
    // Primary colors — only these (plus Black/White) can appear on
    // pigmented fruit parts. Worldgen coverage ensures at least one
    // complete mixable set of primaries is available.
    Red, Yellow, Blue,
    // Shade/tint modifiers (also appear directly on fruit parts)
    Black, White,
    // Secondary colors — produced ONLY by mixing at a dye workshop.
    // Never appear directly on fruit parts.
    Orange, Green, Violet,
}

/// A queryable trait for co-occurrence weighting. Encompasses both
/// PartProperty flags and structural traits like "has pigment."
/// Used by CoOccurrenceEntry to express co-occurrence weights between
/// any combination of property flags and non-flag traits.
enum FruitTrait {
    Property(PartProperty),
    HasPigment,
}

enum GrowthHabitat {
    Branch,       // grows on tree branches (Leaf voxels)
    Trunk,        // grows on trunk surface
    GroundBush,   // grows on forest floor bushes
    WildTree,     // only found on NPC/wild trees
}

enum Rarity {
    Common,
    Uncommon,
    Rare,
}

struct FruitAppearance {
    /// Base color of the fruit exterior.
    exterior_color: Color,
    /// Shape hint for sprite/model generation.
    shape: FruitShape,
    /// Size relative to standard (100 = normal, 50 = half, 200 = double).
    size_percent: u16,
    /// Whether the fruit visibly glows.
    glows: bool,
}

enum FruitShape {
    Round,
    Oblong,
    Clustered,  // berry cluster
    Pod,        // elongated pod
    Nut,        // small and hard-shelled
    Gourd,      // large and bulbous
}
```

### FruitAppearance Generation

During worldgen, `FruitAppearance` is derived from the fruit's parts and
properties rather than chosen independently:

- **exterior_color** — derived from the dominant pigment. If the fruit has
  a pigmented Rind or Flesh, that pigment's color drives the exterior
  color. If no part is pigmented, a heuristic picks a color from
  properties: Luminescent → pale green/white, Starchy → tan/brown,
  Sweet → warm yellow/orange, etc.
- **shape** — derived from part composition. Fruits dominated by Fiber
  parts tend toward Pod. Fruits with many small Seeds tend toward
  Clustered. Fruits with a Tough Rind tend toward Nut. Flesh-heavy
  fruits default to Round or Gourd (depending on size).
- **size_percent** — derived from part count and dominant part yield.
  Since yields always sum to 100, size is driven by how many parts the
  fruit has and how the mass is distributed: a single-part fruit is
  small (Nut-sized); a 3-4 part fruit with a large Flesh yield is
  large (Gourd-sized).
- **glows** — true if any part has the Luminescent property.

### GameConfig Integration

The fruit system introduces the following tunable parameters, all
integers consistent with the project's no-float rule. These belong in
`GameConfig` under a `fruit` subsection:

```rust
struct FruitConfig {
    /// Total fruit species generated per world (inclusive range).
    min_species_per_world: u16,   // e.g., 20
    max_species_per_world: u16,   // e.g., 40

    /// Maximum number of parts a single fruit can have.
    max_parts_per_fruit: u8,      // e.g., 4

    /// Coverage minimums per category (see Coverage Requirements table).
    /// Keyed by category name to allow JSON tuning without code changes.
    coverage_minimums: BTreeMap<String, u16>,

    /// Co-occurrence weight entries (see weight table below).
    /// Each entry is a (trait_a, trait_b, weight) triple.
    /// Weight is an integer 0-100; higher = more likely to co-occur.
    co_occurrence_weights: Vec<CoOccurrenceEntry>,

    /// Greenhouse base production interval in ticks per fruit, for a
    /// single-tile greenhouse. Larger greenhouses divide this by their
    /// footprint area.
    greenhouse_base_production_ticks: u32,  // e.g., 60000 (60 sim-seconds)

    /// Number of fruit species the civilization starts knowing.
    starting_known_species: u8,   // e.g., 5

    /// Rarity weight distribution (relative integer weights).
    /// [Common, Uncommon, Rare] — e.g., [60, 30, 10].
    rarity_weights: [u16; 3],
}

/// A weighted co-occurrence hint for the fruit generator.
/// Biases how likely two traits are to appear on the same fruit
/// (across different parts). Uses FruitTrait so entries can reference
/// both PartProperty flags and structural traits like HasPigment.
struct CoOccurrenceEntry {
    trait_a: FruitTrait,
    trait_b: FruitTrait,
    /// 0 = never co-occur, 50 = neutral, 100 = always co-occur.
    weight: u8,
}
```

Parameters that are structural (part type list, property exclusion rules,
DyeColor primaries) are hardcoded constants, not config. Only numeric
tunables and weight tables go in config.

### Property Co-occurrence Weights

The generator uses a weight matrix to determine how likely two traits
are to appear on the same fruit (across different parts). Entries use
`FruitTrait`, so they can reference both `PartProperty` flags and
structural traits like `HasPigment`. Higher weight = more likely to
co-occur.

| Combination | Weight | Rationale |
|-------------|--------|-----------|
| Sweet + Fermentable | High | Most sweet fruits ferment well |
| Fibrous + Tough rind | High | Structural plants |
| Bitter + HasPigment | High | Dye plants often inedible |
| Aromatic + ManaResonant | Medium | Incense-magic connection |
| Luminescent + Psychoactive | Low | Rare but flavorful |
| Starchy + Fibrous | Low | Very valuable dual-output |
| ManaResonant + Luminescent | Medium | Light-magic affinity |
| Medicinal + Aromatic | Medium | Herbal medicine tradition |
| Sweet + Medicinal | Low | Medicine usually tastes bad |
| Psychoactive + Fermentable | Medium | Drug wines are a thing |

---

## Interaction with Other Systems

### F-fruit-prod (Basic Fruit Production)

F-fruit-prod implements the mechanics of fruit appearing at Leaf voxels
and elves harvesting them. The fruit variety system determines *which*
fruit appears — the production system determines *when and where*. These
two features are complementary and could be implemented in either order:
F-fruit-prod with a single generic fruit type first, then F-fruit-variety
replaces the generic fruit with procedural species. Or both together.

### F-recipes (Recipe System)

The recipe system needs to match on part properties rather than specific
fruit IDs. A recipe like "mill starchy part → flour" works for any fruit
that has a starchy part, regardless of which world generated it. This is a
natural fit for a data-driven recipe system — recipes are property queries,
not item-type lookups.

### F-food-chain (Food Pipeline)

The food chain logistics (harvest → storage → kitchen → dining) works the
same regardless of fruit variety. The variety system adds *what* flows
through the pipeline; the logistics system handles *how* it flows.

### Item Schema

`ItemKind` stays a simple enum with a single `Fruit` variant. The fruit
species is tracked via a `material` field on item stacks (being added as
part of parallel item schema work). This `material` enum has a range of
entries for fruit species IDs alongside entries for other material
categories (wood types, stone types, etc.).

Processed outputs (flour, thread, rind, etc.) are their own `ItemKind`
variants and carry the source fruit's species as their `material`. This
lets recipes match on `ItemKind::Flour` regardless of source, while
quality propagation can still consult the source species. Dyes and
alchemical essences do **not** track source fruit — they're defined by
their output property (dye color, essence type).

The `FruitSpecies` table is keyed by the same ID used in the material
enum, making the lookup straightforward.

### Serialization

The fruit species roster is part of the world save. All generated species
(names, parts, properties, habitats) must be serialized and restored on
load. Individual fruit items reference species by ID.

---

## Resolved Design Decisions

- **Vaelith lexicon expansion:** New botanical morphemes should be
  multipurpose — most are general vocabulary that happens to be useful for
  fruit naming. The word for "red" is the word for "red" everywhere, not a
  fruit-specific morpheme. This keeps the lexicon cohesive and means many
  of the ~44 estimated new entries serve double duty across the language.

- **Lossy processing:** Decided in principle, parameters TBD. Some
  separation methods are lossy — a crude press might yield juice but
  destroy the fiber, while a careful husk preserves both. This creates
  room for workshop-tier progression (better workshop = less waste) and
  interesting trade-offs. The number of workshop tiers and exact loss
  ratios are open (see Open Questions).

- **Procedural fruit sprites:** Fruit visuals will be procedurally
  generated sprites (like creature sprites via `sprite_factory.gd`), not
  3D models. The `FruitAppearance` parameters (shape, color, size, glow)
  drive sprite generation.

- **Greenhouse production model:** Decided in principle, parameters TBD.
  Initially, greenhouses autonomously produce one fruit after another at
  rates proportional to building footprint size. No input costs. Later
  development: fruit visibly appears on greenhouse tiles, embiggens/ripens
  over time, and requires an elf to pluck it into the building's
  inventory. Exact base production rates and footprint scaling are open
  (see Open Questions).

- **Seeds:** Eventually, but not in initial implementation. Greenhouses
  start from research/knowledge, not seed items. Seeds add a logistics
  step and trade possibilities for later.

- **Dye mixing:** Yes, from the start. Worldgen guarantees all three
  primary pigment colors (Red, Yellow, Blue) plus Black and White are
  available from fruit parts. A dye workshop mixes primary dyes into
  secondary colors (Orange, Green, Violet). Secondary colors are only
  produced by mixing, never directly from fruit parts. This keeps the
  pigment fruit count to 3-5 instead of 12+, freeing roster slots.

- **No floating-point numbers:** All numeric fruit data uses integers —
  `yield_percent: u8` (1-100), `size_percent: u16` (50-200), etc. Avoids
  cross-platform determinism hazards and JSON serialization ambiguity.

- **Spoilage:** Deferred to late development, but the property system is
  designed to inform spoilage rates when it arrives. Sweet parts spoil
  fast, Oily parts are preservative, Starchy/dried/tough parts last
  indefinitely. Each fruit's spoilage profile emerges naturally from its
  part properties.

- **Seasons:** When seasons are added, each fruit species gets a growing
  season. The generator should ensure coverage constraints are met *per
  season* for critical categories (e.g., at least one edible fruit per
  season), but it's fine if specific subcategories like bread fruits are
  seasonal — that variety makes games interesting and creates storage
  pressure.

## Deferred Integration Points

These are known architectural questions that depend on parallel or future
work. They are intentionally not resolved in this draft.

- **VoxelType::Fruit species data:** `VoxelType::Fruit` is currently a
  bare enum variant with no associated data. Fruit voxels in the 3D grid
  need a way to know which species they are. Likely solution: a parallel
  `BTreeMap<VoxelCoord, FruitSpeciesId>` on the world (consistent with
  existing patterns), or extending `VoxelType` to carry data. Deferred
  until F-fruit-prod implements fruit voxel spawning.

- **Material field on ItemStack:** The `material` enum/field on item
  stacks is being designed as part of parallel item schema work. This
  draft specifies the *semantics* (fruit species as material, processed
  outputs carry source species, dyes/essences don't) but the concrete
  type definition lives in the item schema design.

- **FruitSpeciesId type:** Likely a strongly-typed newtype wrapping `u16`
  (only 20-40 species per world). Exact definition deferred to
  implementation.

- **SimDb vs. SimState storage:** `FruitSpecies` is immutable worldgen
  data, not a mutable entity. It could live in a tabulosity table (free
  serialization/indexing) or a `BTreeMap<FruitSpeciesId, FruitSpecies>`
  on `SimState` (simpler for immutable data). Decision deferred to
  implementation.

- **FruitAppearance Color type:** The sim crate has no color type
  (rendering is Godot's domain). `FruitAppearance` needs an RGB triple
  of `u8` values to stay integer-only. Trivial to add at implementation
  time.

- **Logistics wants for property-based matching:** The current
  `LogisticsWantRow` is keyed by `ItemKind`. Fruit variety eventually
  needs property-based wants ("any starchy fruit for this kitchen").
  This is a logistics system change, not a fruit system change. Initial
  implementation can use `ItemKind::Fruit` as a coarse match and refine
  later.

- **FurnishingType for greenhouses:** `Greenhouse` as a `FurnishingType`
  needs to carry a `FruitSpeciesId`, unlike other furnishing types.
  Deferred until the furnishing system is extended for this use case.

- **Design doc fruit selection UI:** The design doc (§13) lists three
  approaches to directing which fruits a tree produces. The greenhouse
  model is our chosen direction — it replaces the "UI for selecting
  fruit types per tree" approach with a building-based system. The
  design doc open question can be closed when this draft is finalized.

- **Platform gardens:** The design doc mentions "cultivated gardens on
  platforms" as a secondary food source. Greenhouses are thematically
  related but mechanically distinct (enclosed buildings vs. open garden
  plots). The relationship between these can be resolved when platform
  gardens are designed.

- **Name collision avoidance:** With 20-40 fruits per world, the name
  generator needs guaranteed uniqueness. The current elf name generator's
  retry approach is insufficient. The fruit name generator will need a
  rejection-sampling loop or a combinatorial approach that guarantees
  no duplicates. Deferred to implementation.

- **NameTag for botanical morphemes:** The current `NameTag` enum only
  has `Given` and `Surname`. Botanical morphemes need a separate tag
  (e.g., `Botanical`) or a parallel filtering mechanism so they don't
  leak into elf names. Straightforward to add when lexicon expansion
  happens.

- **Save/load migration:** Adding `FruitSpecies` to the save format is
  a breaking change for pre-fruit-variety saves. Migration strategy
  (generate a default fruit roster for old saves, or reject them)
  deferred to implementation.

- **Nutritional value model:** The existing sim uses flat
  `food_per_meal` / `elf_food_per_tick` values. Fruit variety should
  eventually make Sweet fruits more satisfying than Bland ones, cooked
  food better than raw, etc. The property system supports this, but
  the nutrition model itself is not designed here.

- **Thought system integration:** Elves could have thoughts about eating
  favorite fruits, trying new ones, eating well-cooked meals vs. raw
  fruit, etc. This is where fruit variety becomes emotionally meaningful.
  Deferred until the thought system is more developed.

- **Separation task/workshop specifics:** Which `FurnishingType` handles
  fruit separation? What task type? Does the elf carry fruit to the
  workshop or does the workshop pull from storage? These depend on the
  logistics and task system designs (F-food-chain, F-recipes).

- **Coverage solver diversity:** The generator could produce samey
  multi-category "Swiss army" fruits. May need a constraint like "no
  fruit covers more than N coverage categories" to ensure variety.
  Tuning issue for implementation.

## Open Questions

- **Workshop tiers for lossy processing:** How many tiers? Two (crude /
  refined) or three (crude / standard / masterwork)? Does the workshop
  type determine lossiness, or is it the worker's skill level, or both?

- **Greenhouse footprint details:** What's the minimum/maximum size? Does
  shape matter or just area? Can a greenhouse be irregularly shaped?

- **Fruit appearance variety:** How much visual distinctiveness can we get
  from {shape, color, size, glow} alone? May need additional parameters
  (surface texture, stem style, cluster count for berries) to avoid
  fruits looking too similar.

- **Property-to-name mapping ambiguity:** When a fruit has multiple
  notable properties across parts, which ones dominate the name? Need a
  priority system (e.g., rarest property wins, or most gameplay-relevant,
  or random weighted choice).

- **Multi-output recipe UI:** How does the player understand that
  processing fruit X yields both fiber and dye? Tooltip on the fruit?
  Recipe book entry? Workshop preview panel? Needs UI design work.
