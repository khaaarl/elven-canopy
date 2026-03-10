# Elfcyclopedia, Civilizations, and Knowledge Systems — Draft (v4)

Design draft for interconnected systems: a worldgen framework, procedural
civilization generation, a tiered knowledge model, and a web-based
elfcyclopedia that surfaces only what the player's civilization knows.
These systems build on the procedural fruit variety system (see
`fruit_variety.md`) and lay groundwork for future tech trees, magic
discovery, and DF-style legends.

## Feature Decomposition

This design spans six tracker items with the following dependency graph:

```
F-worldgen-framework ──┬──► F-fruit-variety ──┐
                       │                      ├──► F-civ-knowledge ───┐
                       └──► F-civilizations ──┘                      │
                                                                     │
F-elfcyclopedia-srv ─────────────────────────────────────┐            │
  (independent, no sim deps)                            ├──► F-elfcyclopedia-know
                                                        └────────────┘
```

- **F-worldgen-framework** — Worldgen entry point, generator sequencing,
  worldgen PRNG, WorldgenConfig. No gameplay content.
- **F-fruit-variety** — Procedural fruit species generation (existing
  tracker item, see `fruit_variety.md`). Depends on worldgen framework.
- **F-civilizations** — Civilization generation, diplomacy, species
  enums, player ownership. Depends on worldgen framework.
- **F-civ-knowledge** — Knowledge tables, tier system, fruit knowledge
  distribution, discovery commands. Depends on both F-fruit-variety and
  F-civilizations.
- **F-elfcyclopedia-srv** — Embedded localhost HTTP server, species
  bestiary, in-game browser button. No sim dependencies beyond config.
  Can be implemented independently of all other items.
- **F-elfcyclopedia-know** — Adds Civilizations and Fruits tabs to
  the elfcyclopedia, gated by civ knowledge. Depends on F-civ-knowledge
  and F-elfcyclopedia-srv.

---

## Core Concept

Every game world generates 10 civilizations during worldgen (configurable
via `CivConfig::civ_count`). Each has a primary species, cultural
identity, asymmetric diplomatic opinions of other civs, and partial
knowledge of the world's fruits. The player controls one civilization
(their elf tree-village) and starts knowing about 5 of the 10 civs
(configurable via `CivConfig::player_starting_known_civs`) and a subset
of the world's fruits at varying detail levels.

An in-game Elfcyclopedia provides a unified view of everything the
player's civilization knows — species lore (constant), known
civilizations (discovered), and known fruits (tiered detail). As the
game progresses, trade, exploration, and contact events expand what the
elfcyclopedia shows.

---

## Civilizations

### Generation

Worldgen creates civilizations according to `CivConfig`. Each
civilization has:

- **CivId** — strongly-typed newtype wrapping `u16`, assigned
  sequentially by the worldgen generator starting at 0. Not
  auto-increment (worldgen assigns all IDs in a batch, and civs are
  never created at runtime in the initial implementation).
- **Name** — Vaelith-generated for elf civs. Non-elf civs use
  placeholder phonetic generation: random syllable pairs drawn from a
  per-species hardcoded consonant/vowel table (e.g., dwarves get
  heavier consonant clusters, goblins get harsher sounds). This is
  intentionally simple — proper non-elf naming systems are future work.
- **Primary species** — one of the `CivSpecies` enum variants (Elf,
  Human, Dwarf, Goblin, Orc, Troll). See "Species Enums" below for
  how this relates to the existing `Species` enum in `types.rs`.
- **Minority species** — optional secondary species present in the civ.
  Stored as a `Vec<CivSpecies>`, kept sorted by `CivSpecies` Ord for
  deterministic iteration (usually 0-1 entries). Goblin civs might
  include Trolls; Orc civs might include Goblins and Trolls. Elf,
  Human, and Dwarf civs are typically mono-species. A `Vec` is used
  rather than a child table because minority species are never queried
  independently — they're always loaded with their parent civ.
- **Culture tag** — a lightweight flavor enum. Assigned during worldgen
  with species-biased random selection: Elf civs favor Woodland/Coastal,
  Dwarf civs favor Mountain/Subterranean, Goblin/Orc civs favor
  Subterranean/Martial, Human civs have equal weight across all tags.
  Not mechanically significant in the initial implementation.
- **Player-controlled flag** — `player_controlled: bool` on the civ
  itself (see "Player Ownership" below).

### Species Enums

The existing `Species` enum in `types.rs` tracks creature types that are
sim-active (have creature instances, rendering, pathfinding, etc.). The
new `CivSpecies` enum tracks sapient species that can form civilizations.

```
Species (sim-active):     Elf, Capybara, Boar, Deer, Elephant, Monkey, Squirrel
CivSpecies (civ-forming): Elf, Human, Dwarf, Goblin, Orc, Troll
```

These overlap at `Elf` but serve different purposes. The `Civilization`
table uses `CivSpecies` for `primary_species` and `minority_species`.
The `Creature` table continues to use `Species`.

**Convergence plan:** When a new sapient species becomes sim-active
(e.g., Human gets creature instances, rendering, and pathfinding), it
gets added to `Species` and a `From<CivSpecies> for Species` conversion
is extended. The `CivSpecies` enum is kept permanently — it represents
"species that can form civilizations" which is a distinct concept from
"species present in the sim." Some `Species` (Capybara, Boar) will never
be in `CivSpecies`, and some `CivSpecies` (e.g., a future species that
only exists in legends) might never be in `Species`.

### Species Distribution

The generator creates civs with a weighted species distribution. Default
weights (configurable in `CivConfig`):

| Species | Weight | Typical Count (of 10) | Notes |
|---------|--------|-----------------------|-------|
| Elf     | 25     | 2-3                   | Player's civ is always one |
| Human   | 25     | 2-3                   | |
| Dwarf   | 20     | 1-2                   | |
| Goblin  | 15     | 1-2                   | Often has Troll minority |
| Orc     | 10     | 0-1                   | Often has Goblin/Troll minority |
| Troll   | 5      | 0-1 (as primary)      | More common as minority in Goblin/Orc civs |

The player's Elf civ is always created first (outside the distribution).
The remaining `civ_count - 1` civs are drawn from the weighted
distribution using the world PRNG.

### Diplomacy

Relationships between civilizations are **asymmetric** and stored as
directed pairs. Civ A's opinion of Civ B is independent of Civ B's
opinion of Civ A.

**Opinion enum:**

```rust
enum CivOpinion {
    Friendly,
    Neutral,
    Suspicious,
    Hostile,
}
```

Absence of a `CivRelationship` row means the two civs are **unaware** of
each other. Awareness is also asymmetric — Civ A can know about Civ B
while B has never heard of A.

**Worldgen seeding:** The generator creates initial relationships based on
species affinity defaults, then applies random perturbation:

| Pair | Default | Notes |
|------|---------|-------|
| Elf ↔ Elf | Friendly | Shared cultural affinity |
| Elf ↔ Human | Neutral | |
| Elf ↔ Dwarf | Neutral | |
| Dwarf ↔ Human | Neutral/Friendly | |
| Goblin ↔ most | Suspicious/Hostile | |
| Orc ↔ most | Hostile | |
| Troll ↔ most | Suspicious | |
| Same species | Neutral/Friendly | Intra-species civs tend positive |

After applying defaults, the generator:
1. Randomly shifts ~30% of relationships one step (Friendly→Neutral,
   Neutral→Suspicious, etc.) in either direction for variety.
2. Ensures asymmetry exists in some pairs (e.g., Elf civ regards a
   Human civ as Friendly, but the Human civ only regards them as
   Neutral).

### Player Ownership

Player-to-civ assignment is split into two layers:

**Sim-side (persisted in save):** Each `Civilization` has a
`player_controlled: bool` field. This is world truth — it says "this civ
takes commands from players, not AI." Currently exactly one civ has this
set (the player's elf civ). This field is what makes save files portable
— no player identity is baked in. You can send someone your save; they
load it, and the session layer assigns them to the player-controlled civ.

**Session-side (ephemeral, not persisted):** `GameSession` holds the
player→civ assignment as a `BTreeMap<SessionPlayerId, CivId>`. This is
resolved at load time:

- **Single player-controlled civ (current):** All players in the session
  are automatically assigned to it. Trivial — no UI needed.
- **Multiple player-controlled civs (future):** The load/lobby screen
  shows "these civs need player assignments" and players pick before
  the game starts. The assignment mechanism is deferred.

The elfcyclopedia queries use `civ_id` directly (not player_id). The
indirection is: session knows which civ you are → you query the
elfcyclopedia with that civ_id. The sim never needs to know about players
for knowledge purposes.

---

## Knowledge System

### Fruit Knowledge Tiers

Each civilization has independent knowledge of each fruit species in the
world. Knowledge is tracked per civ-fruit pair with three tiers:

```rust
enum FruitKnowledgeTier {
    /// Know the fruit exists — name, maybe a vague description.
    /// Enough to recognize it but not use it effectively.
    Awareness,

    /// Know the fruit's properties — nutritional value, part
    /// composition, processing paths, effects. Enough to use it
    /// if you have it, but not to produce it.
    Properties,

    /// Know how to cultivate the fruit — can assign it to a
    /// greenhouse, understands growing conditions. Full mastery.
    Cultivation,
}
```

Tiers are strictly ordered: Cultivation implies Properties implies
Awareness. A civ cannot know properties without awareness, or
cultivation without properties. Knowledge never degrades — there is no
"forgetting" mechanic. This may change in the far future (e.g., "last
elf who knew this technique died"), but for now knowledge is monotonically
increasing.

### Starting Knowledge Distribution

The player's elf civilization starts with knowledge calibrated to
bootstrap gameplay (per `fruit_variety.md` §Research and Starting
Knowledge):

- **Cultivation tier (4-5 fruits):** 1-2 starchy (bread), 1 fiber
  (clothing), 1 dye (self-expression), 1 luminescent or edible
  (lighting/variety). These are the fruits you can greenhouse from
  day one.
- **Properties tier (5-10 fruits):** Enough to know what they do if
  you acquire them through trade or exploration, but you can't grow
  them yet.
- **Awareness tier (most remaining):** You've heard of most fruits —
  you know names and vague descriptions. A few rare/distant fruits
  may be completely unknown.

If the sum of configured cultivation + properties + unknown counts
exceeds the total fruit species count, the generator clamps: it fills
Cultivation first, then Properties, then assigns remaining fruits to
Awareness (minus the unknown count). This ensures the config is never
invalid regardless of fruit count.

Other civilizations get analogous distributions biased by their species
and culture:
- Dwarf civs know more about underground/cave fruits (if any exist)
- Goblin civs know fewer fruits overall but may know rare poisonous or
  psychoactive ones
- Human civs have broad but shallow knowledge (many at Awareness, fewer
  at Cultivation)

The exact counts are configurable in `CivConfig`.

### Civilization Knowledge (Who Knows Whom)

The player's civ starts aware of 5 of 10 civs (configurable). Which civs
are known is determined by worldgen based on species affinity and random
proximity (since geography is abstract for now, "proximity" is just a
PRNG roll biased toward same-species and friendly civs).

Other civs also have partial knowledge of each other, generated
with a bias toward symmetry: if Civ A knows about Civ B, there's a high
(but not guaranteed) chance B knows about A.

### Knowledge Discovery (Future)

Knowledge expands through gameplay events that don't exist yet but the
data model supports:

- **Trade:** Visiting traders bring awareness of their civ and its
  known fruits. Repeated trade can upgrade knowledge tiers.
- **Exploration/Expeditions:** Sending elves out can discover new civs
  and wild fruits.
- **Refugees/Immigrants:** Creatures arriving from other civs bring
  their home civ's knowledge.
- **Raids:** Surviving an attack reveals the attacker's civilization.
  Captured goods might reveal fruit properties.
- **Diplomacy events:** Formal contact can result in knowledge exchange.

Each of these would create `SimCommand` variants that update the
knowledge tables. The elfcyclopedia automatically reflects changes on
the next query.

**Initial implementation note:** Until trade/exploration/combat systems
exist, the `DiscoverCiv`, `SetCivOpinion`, and `LearnFruit` commands
will only be exercised by worldgen and debug/test commands. This is
expected — the commands establish the API contract for future systems.

### Extensibility to Other Knowledge Domains

The `CivFruitKnowledge` pattern (civ FK + domain-item FK + tier enum)
generalizes to any knowledge domain:

- **CivSpellKnowledge** — tier: Awareness (know it exists) → Theory
  (understand mechanics) → Mastery (can cast/teach)
- **CivTechKnowledge** — tier: Rumor → Understanding → Adoption
- **CivCreatureKnowledge** — tier: Legend → Sighting → Studied

Each domain gets its own table and tier enum. The elfcyclopedia adds a
tab per domain. No polymorphic "knowledge" table needed — each domain
has distinct semantics and tier meanings.

---

## Elfcyclopedia (Web-Based)

### Overview

The elfcyclopedia is a **web-based UI** served by an embedded localhost
HTTP server. The running game listens on a configurable port
(`127.0.0.1` only — no external access). The player opens the
elfcyclopedia in their default web browser via an in-game toolbar button
that shows the URL. This approach offers several advantages over an
in-game Godot panel:

- Rich layout for free — HTML/CSS handles tables, nested lists,
  search/filter, responsive layout without fighting Godot's UI system.
- Easy to keep open on a second monitor while playing.
- Fast iteration — change templates and refresh, no rebuild cycle.
- Accessible for modders/wiki-builders.
- Trivially shareable in co-op (same URL format, different ports).

The elfcyclopedia is split into two implementation phases:

**F-elfcyclopedia-srv (Phase 1 — no sim dependencies):**
- Embedded HTTP server plumbing (port config, auto-fallback, thread
  management).
- Species bestiary tab — constant data from JSON, always available.
- In-game toolbar button showing URL, click to open browser.
- General game info pages (controls, mechanics summaries).

**F-elfcyclopedia-know (Phase 2 — depends on F-civ-knowledge):**
- Civilizations tab — known civs with opinions.
- Fruits tab — tier-gated detail.
- Any future knowledge-domain tabs (spells, tech, etc.).

### Architecture

**HTTP server (Rust, in `elven_canopy_gdext`):**

A lightweight HTTP server runs on a dedicated thread, spawned by
`SimBridge` on initialization. The server is strictly read-only — it
queries sim state but never mutates it. This preserves determinism
(the server is invisible to the sim).

- **Library:** `tiny_http` (or similar minimal dependency — ~2k lines,
  no transitive deps). Lives in `elven_canopy_gdext` only, not in the
  sim crate.
- **Binding:** `127.0.0.1:PORT` only. No external network access.
- **Port:** Configurable via `GameConfig` (default: 7777). If the port
  is taken, auto-increment until a free port is found. The actual port
  is reported to Godot for display.
- **Threading:** The server thread holds a read handle to sim state via
  `Arc<RwLock<SimState>>` (or a snapshot channel). The sim only writes
  during `frame_update()`, so contention is minimal. The server never
  blocks the game loop.
- **Rendering:** Server-rendered HTML templates (not a JS SPA). Keeps
  dependencies minimal and pages work without JavaScript. Templates
  can use a simple string-substitution engine — no need for a full
  template library.
- **Staleness:** Pages include a "Data as of tick N" footer and a
  meta-refresh tag (configurable interval, default 5s) for auto-update.

**Godot side (minimal):**

- Toolbar button labeled "Elfcyclopedia" showing the URL/port.
- Clicking the button calls `OS.shell_open(url)` to open the default
  browser.
- No in-game panel, no ESC chain changes, no input precedence impact.

### Pages

**Species Bestiary (F-elfcyclopedia-srv):**
- URL: `/species` (list) and `/species/{name}` (detail).
- Always fully visible — universal lore, not gated by knowledge.
- Includes all creature types: both sapient species (Elf, Human, Dwarf,
  Goblin, Orc, Troll) and wild animals (Capybara, Boar, etc.).
- Shows: name, description, behavioral traits, whether sapient/wild.
- Data source: `data/species_elfcyclopedia.json` loaded at server
  startup. Not world-specific. Contains constant flavor text,
  behavioral summaries, and tags per species.

**Civilizations (F-elfcyclopedia-know):**
- URL: `/civilizations` (list) and `/civilizations/{id}` (detail).
- Only civs your civilization is aware of (has a `CivRelationship` row
  where `from_civ` is your civ).
- Shows: name, primary species, your opinion of them, their opinion of
  you (see resolved decisions).
- Possible future: detail scales with relationship depth.
- Data source: `CivRelationship` table filtered by player civ.

**Fruits (F-elfcyclopedia-know):**
- URL: `/fruits` (list) and `/fruits/{id}` (detail).
- Only fruits your civ has at least Awareness of.
- Detail scales with knowledge tier:
  - **Awareness:** Name, appearance (color, shape, size, glow), vague
    description. "A sweet-smelling cluster fruit said to grow in
    distant groves."
  - **Properties:** Full part breakdown, processing paths, nutritional
    and material properties, habitat, rarity. Enough to plan your
    economy around.
  - **Cultivation:** Greenhouse assignment unlocked. The "you can grow
    this" marker.
- Data source: `CivFruitKnowledge` table filtered by player civ,
  joined with `FruitSpeciesRow` data. Tier determines detail level.

**Index / Home:**
- URL: `/`
- Navigation links to all available sections.
- Shows current game state summary (tick, civ name, population count).

### Query API

The HTTP server calls query methods on `SimState` (via its read handle):

```rust
/// Get the player-controlled civ.
fn get_player_civ() -> Option<CivId>;

/// Elfcyclopedia: constant species data (from JSON, cached at startup).
fn get_elfcyclopedia_species() -> Vec<SpeciesEntry>;

/// Elfcyclopedia: known civilizations (with opinions).
fn get_elfcyclopedia_civs(civ_id: CivId) -> Vec<KnownCivEntry>;

/// Elfcyclopedia: known fruits (tier-gated detail).
fn get_elfcyclopedia_fruits(civ_id: CivId) -> Vec<KnownFruitEntry>;

/// Specific fruit detail for drill-down page.
fn get_fruit_detail(civ_id: CivId, fruit_id: FruitSpeciesId)
    -> Option<KnownFruitEntry>;
```

**Return type sketches:**

```rust
struct SpeciesEntry {
    name: String,
    description: String,
    sapient: bool,
    // Future: image path, behavioral tags, etc.
}

struct KnownCivEntry {
    civ_id: CivId,
    name: String,
    primary_species: CivSpecies,
    our_opinion: CivOpinion,
    /// Their opinion of us. `Some` if they are aware of us (have a
    /// CivRelationship row where from_civ = them, to_civ = us).
    /// `None` if they don't know we exist.
    their_opinion: Option<CivOpinion>,
}

/// Fruit entry where field presence depends on knowledge tier.
struct KnownFruitEntry {
    fruit_id: FruitSpeciesId,
    tier: FruitKnowledgeTier,
    // Always present (Awareness+):
    vaelith_name: String,
    english_gloss: String,
    /// Visual appearance — color, shape, size, glow. Always present
    /// because these are observable properties, not secret knowledge.
    appearance: FruitAppearance,
    // Present at Properties+:
    parts: Option<Vec<FruitPartSummary>>,
    habitat: Option<GrowthHabitat>,
    rarity: Option<Rarity>,
    // Present at Cultivation:
    greenhouse_cultivable: Option<bool>,
}

struct FruitPartSummary {
    part_type: PartType,
    properties: Vec<PartProperty>,
    pigment: Option<DyeColor>,
    yield_percent: u8,
}
```

The query methods construct these structs by reading `FruitSpeciesRow`
and masking fields based on the civ's knowledge tier. The HTTP server
serializes them into HTML templates. The sim never leaks information
the civ doesn't know.

### Multiplayer

In co-op (multiple players, same civ), each player's game instance runs
its own elfcyclopedia server. All show the same data (same civ_id). In
competitive MP (different civs), each sees only their own civ's
knowledge. No special handling needed — the server always queries for
the local session's player-controlled civ.

---

## Data Model

### New ID Types (in `types.rs`)

```rust
/// Civilization identifier. Assigned sequentially by worldgen (0, 1, 2, ...).
/// Not auto-increment — civs are batch-created during worldgen and not
/// created at runtime in the initial implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
         Serialize, Deserialize, Bounded)]
pub struct CivId(pub u16);

/// Fruit species identifier (cross-reference with fruit_variety.md).
/// Assigned sequentially by the fruit worldgen generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
         Serialize, Deserialize, Bounded)]
pub struct FruitSpeciesId(pub u16);

/// Auto-increment PK for CivRelationship rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
         Serialize, Deserialize, Bounded)]
pub struct CivRelationshipId(pub u32);

/// Auto-increment PK for CivFruitKnowledge rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
         Serialize, Deserialize, Bounded)]
pub struct CivFruitKnowledgeId(pub u32);
```

### New SimDb Tables

```rust
/// A procedurally generated civilization.
#[derive(Clone, Debug, Serialize, Deserialize, Table)]
#[table(name = "civilizations")]
pub struct Civilization {
    #[primary_key]
    pub id: CivId,
    pub name: String,
    pub primary_species: CivSpecies,
    /// Secondary species present in this civ (usually 0-1).
    /// Kept sorted by CivSpecies Ord for deterministic iteration.
    pub minority_species: Vec<CivSpecies>,
    pub culture_tag: CultureTag,
    /// Whether this civ is controlled by player(s) vs AI.
    /// Currently exactly one civ has this set to true.
    pub player_controlled: bool,
}

/// Directed relationship: `from_civ`'s opinion of `to_civ`.
/// Absence = unaware.
///
/// Invariant: at most one row per (from_civ, to_civ) pair. Enforced
/// by lookup-before-insert in the insert logic (tabulosity does not
/// yet support compound unique indexes).
#[derive(Clone, Debug, Serialize, Deserialize, Table)]
#[table(name = "civ_relationships")]
pub struct CivRelationship {
    #[primary_key(auto_increment)]
    pub id: CivRelationshipId,
    #[indexed]
    pub from_civ: CivId,
    #[indexed]
    pub to_civ: CivId,
    pub opinion: CivOpinion,
}

/// What a civilization knows about a fruit species.
/// Absence = completely unknown.
///
/// Invariant: at most one row per (civ_id, fruit_id) pair. Enforced
/// by lookup-before-insert in the insert logic.
#[derive(Clone, Debug, Serialize, Deserialize, Table)]
#[table(name = "civ_fruit_knowledge")]
pub struct CivFruitKnowledge {
    #[primary_key(auto_increment)]
    pub id: CivFruitKnowledgeId,
    #[indexed]
    pub civ_id: CivId,
    #[indexed]
    pub fruit_id: FruitSpeciesId,
    pub tier: FruitKnowledgeTier,
}

/// Immutable worldgen data for a fruit species.
/// This is the tabulosity table form of `FruitSpecies` from
/// `fruit_variety.md`. All types used here (GrowthHabitat, Rarity,
/// FruitPart, FruitAppearance, PartType, PartProperty, DyeColor,
/// FruitShape) are defined in the fruit variety design and must be
/// implemented before or alongside this system.
///
/// Note: `parts` contains `BTreeSet<PartProperty>` inside each
/// `FruitPart`, which serde serializes as a JSON array. This
/// roundtrips correctly but should be verified in tests.
#[derive(Clone, Debug, Serialize, Deserialize, Table)]
#[table(name = "fruit_species")]
pub struct FruitSpeciesRow {
    #[primary_key]
    pub id: FruitSpeciesId,
    pub vaelith_name: String,
    pub english_gloss: String,
    pub habitat: GrowthHabitat,
    pub rarity: Rarity,
    pub greenhouse_cultivable: bool,
    /// Parts are stored as a Vec rather than a separate table — they're
    /// always loaded with their parent and never queried independently.
    pub parts: Vec<FruitPart>,
    pub appearance: FruitAppearance,
}
```

### Creature Table Addition

Add a nullable `civ_id` to the existing `Creature` table:

```rust
pub struct Creature {
    // ... existing fields ...
    /// Civilization this creature belongs to (None = wild/unaffiliated).
    #[indexed]
    pub civ_id: Option<CivId>,
}
```

### Enums

```rust
enum CivOpinion {
    Friendly,
    Neutral,
    Suspicious,
    Hostile,
}

enum FruitKnowledgeTier {
    Awareness,
    Properties,
    Cultivation,
}

enum CultureTag {
    Woodland,
    Mountain,
    Coastal,
    Subterranean,
    Nomadic,
    Martial,
}

/// Species that can form civilizations. Separate from the sim-active
/// `Species` enum — see "Species Enums" section for rationale and
/// convergence plan.
enum CivSpecies {
    Elf,
    Human,
    Dwarf,
    Goblin,
    Orc,
    Troll,
}
```

### FK Relationships

| Child Table | FK Column | Parent Table | On Delete |
|-------------|-----------|-------------|-----------|
| CivRelationship | from_civ | Civilization | Cascade |
| CivRelationship | to_civ | Civilization | Cascade |
| CivFruitKnowledge | civ_id | Civilization | Cascade |
| CivFruitKnowledge | fruit_id | FruitSpeciesRow | Restrict |
| Creature | civ_id | Civilization | Nullify |

Cascading civ deletion removes all its relationships and knowledge.
Creature's `civ_id` is nullified (creature becomes wild/unaffiliated).
Fruit species cannot be deleted while knowledge rows reference them.

**Player civ protection:** The player-controlled civ cannot be deleted
through normal gameplay. If a future "civ destruction" event is added,
destroying the player's civ triggers a game-over event rather than
cascading deletes. The `player_controlled` flag serves as the guard.

---

## Worldgen Framework (F-worldgen-framework)

Currently the sim starts with a tree and some elves but has no formal
worldgen phase. Both fruit variety and civilization generation need a
shared framework:

- **Worldgen entry point:** A function called during `StartGame` that
  runs generators in a defined order: tree → fruits → civilizations →
  knowledge distribution. Each generator is a standalone function that
  takes the world PRNG, config, and SimDb, and populates its tables.
- **Worldgen PRNG:** A dedicated `GameRng` instance seeded from the
  world seed, used exclusively during worldgen. After worldgen
  completes, the sim's runtime PRNG is seeded from the worldgen PRNG's
  final state (or from a separate split), ensuring the worldgen sequence
  doesn't affect runtime randomness order.
- **WorldgenConfig:** A subsection of `GameConfig` grouping
  `FruitConfig` and `CivConfig` together. The existing tree generation
  config stays where it is.

This framework is small — it's plumbing, not gameplay. It establishes
the pattern so that fruit and civ generators slot in as independent
steps called in the right order.

---

## Worldgen Flow

After tree generation, the worldgen framework runs fruit and civ
generators in order:

1. **Generate civilizations.** Draw `civ_count` civs from the weighted
   species distribution. Assign names (Vaelith for elf civs, placeholder
   for others). Assign culture tags (species-biased random selection).
   The player's elf civ is always generated first with `CivId(0)` and
   `player_controlled = true`. All IDs are assigned sequentially.

2. **Assign creature civ membership.** All starting elves get
   `civ_id = Some(player_civ_id)`. Future spawned creatures (immigrants,
   births) inherit their civ from context.

3. **Generate diplomacy graph.** For each ordered civ pair, decide
   whether each side is aware of the other. Base awareness probability
   is 50%; same-species pairs get +25%, adjacent entries in the species
   affinity table get a further bias (friendly +10%, hostile +15% —
   enemies tend to know about each other). Each direction is an
   independent roll. For aware pairs, assign initial opinion from the
   species-pair default table, then apply random perturbation. Each
   aware pair creates one `CivRelationship` row per direction.

4. **Distribute fruit knowledge.** For each civ:
   - Select fruits for Cultivation tier (4-5, biased by species/culture
     toward compatible habitats and properties).
   - Select additional fruits for Properties tier (5-10).
   - Select most remaining fruits for Awareness tier, leaving 0-3
     completely unknown.
   - The player's civ uses the starting knowledge distribution described
     above. Counts are clamped to available fruit species.

5. **Set up session assignment.** On game start (not in the save),
   `GameSession` scans for `player_controlled` civs and assigns
   connected players to them.

### Config

```rust
struct CivConfig {
    /// Number of civilizations to generate.
    civ_count: u16,  // default: 10

    /// Weighted species distribution for worldgen.
    /// Defaults to the weights in the Species Distribution table.
    species_weights: BTreeMap<CivSpecies, u16>,

    /// How many civs the player starts aware of.
    player_starting_known_civs: u16,  // default: 5

    /// Fruit knowledge distribution for player civ.
    /// If cultivation + properties + unknown > total fruit count,
    /// the generator fills Cultivation first, then Properties,
    /// then assigns remaining to Awareness.
    player_cultivation_count: u16,    // default: 5
    player_properties_count: u16,     // default: 8
    /// Number of fruits completely unknown to the player civ.
    player_unknown_count: u16,        // default: 2
}
```

---

## Interaction with Existing Systems

### SimCommand Extensions

New command variants for knowledge updates (used by future gameplay
events):

```rust
enum SimAction {
    // ... existing variants ...

    /// A civ becomes aware of another civ. Creates a CivRelationship
    /// row with the specified initial opinion. No-op if the
    /// relationship already exists.
    DiscoverCiv {
        civ_id: CivId,
        discovered_civ: CivId,
        initial_opinion: CivOpinion,
    },

    /// Update a civ's opinion of another civ. No-op if unaware.
    SetCivOpinion {
        civ_id: CivId,
        target_civ: CivId,
        opinion: CivOpinion,
    },

    /// A civ gains or upgrades fruit knowledge. Only upgrades — a
    /// LearnFruit with a lower tier than current knowledge is a no-op.
    LearnFruit {
        civ_id: CivId,
        fruit_id: FruitSpeciesId,
        tier: FruitKnowledgeTier,
    },
}
```

These are fire-and-forget commands. Until trade/exploration/combat
systems exist, they are only exercised by worldgen and debug/test code.

### Save/Load

All new tables (civilizations, relationships, fruit knowledge, fruit
species) are part of `SimDb` and get serialized/deserialized via
tabulosity's serde support. The `player_controlled` flag on civilizations
is the only player-related data in the save — session-level player→civ
assignment is not persisted.

This is a breaking change for pre-civilization saves. Migration strategy:
reject old saves via version check (acceptable for pre-release).

### Notifications

Civ/knowledge events pair naturally with the notification system:
"Traders from the Thalendrim arrive!" → notification + DiscoverCiv
command. "You learn the properties of starfruit!" → notification +
LearnFruit command. The notification is the player-visible event; the
command is the sim-side state change.

---

## Resolved Design Decisions

- **Web-based elfcyclopedia:** The elfcyclopedia is served as HTML via an
  embedded localhost HTTP server, not as an in-game Godot panel. This
  gives rich layout, second-monitor support, fast iteration, and
  modder accessibility. The in-game UI is just a toolbar button that
  opens the browser. See "Elfcyclopedia (Web-Based)" section.

- **Player ownership model:** `player_controlled: bool` on `Civilization`
  (persisted in save) + ephemeral `GameSession` player→civ assignment
  (not persisted). Saves are portable — no player identity baked in.
  See "Player Ownership" section for details.

- **Show opponent opinion:** Yes, show both directions in the
  elfcyclopedia for transparency. The player sees "You: Friendly / Them:
  Suspicious". This is simpler to implement and more informative. If
  espionage mechanics are added later, the "their opinion" field could
  be hidden/inaccurate based on intelligence level, but that's future
  work.

- **Species entries include animals:** The Species bestiary covers all
  creature types — sapient civilization-forming species and wild animals
  alike.

- **Compound uniqueness via insert logic:** The `(from_civ, to_civ)`
  and `(civ_id, fruit_id)` uniqueness invariants are enforced by
  lookup-before-insert in the Rust code, not by tabulosity index
  constraints. If tabulosity gains compound unique indexes later, these
  can be migrated. The insert helpers must query the relevant index
  first and either update the existing row or skip/error.

- **No runtime civ creation (initially):** Civs are only created during
  worldgen. The `Civilization` table is effectively append-only after
  game start. A future `CreateCiv` command could be added for events
  like refugee groups forming a new settlement or civ splitting.

- **NPC civs have no creatures in the sim (initially):** NPC
  civilizations are abstract entities — they exist in the elfcyclopedia
  and diplomacy tables but have no creature instances in the world.
  When trade/combat systems arrive, NPC civ creatures (traders, raiders)
  will be spawned as needed with appropriate `civ_id` assignments.

- **Two-phase elfcyclopedia implementation:** F-elfcyclopedia-srv
  ships the HTTP plumbing and species bestiary with zero sim
  dependencies. F-elfcyclopedia-know adds civ/fruit tabs later,
  once the knowledge system exists. This lets the elfcyclopedia
  infrastructure be built and tested independently.

## Deferred Design Decisions

- **Geography and civ location:** Civs are currently abstract entities
  with no spatial position. When a regional map is added, civs gain
  locations and "proximity" becomes real distance rather than PRNG bias.

- **Civ generation depth:** No generated history, founding dates,
  historical events, wars, or migrations. This is the future DF-legends
  direction but not in scope now.

- **Dynamic diplomacy:** Initial opinions are static after worldgen.
  Future: events (trade, conflict, gifts, insults) shift opinions over
  time. The `SetCivOpinion` command exists to support this.

- **Civ-to-civ knowledge sharing:** When two civs interact, do they
  automatically share fruit knowledge? At what rate? This depends on
  the trade/diplomacy systems that don't exist yet.

- **Elfcyclopedia visual design:** HTML template styling, CSS, page
  transitions are all TBD. The initial implementation can be plain
  unstyled HTML, refined later. The web approach makes iterating on
  visuals trivial (edit CSS, refresh browser).

- **Species entry content:** What exactly goes in each species'
  elfcyclopedia entry? Flavor text, stats, behavioral notes? Needs
  writing work that's independent of the systems design. Will be a
  JSON data file (`data/species_elfcyclopedia.json`).

- **Co-op command conflicts:** When multiple players share a civ, who
  can issue `SetCivOpinion` commands? Currently last-writer-wins (the
  command takes a `civ_id`, not `player_id`). This needs arbitration
  rules if/when competitive-within-civ scenarios arise.

- **HTTP server library choice:** `tiny_http` is the current leading
  candidate (minimal, no transitive deps). Could also consider
  `hyper` (more capable but heavier) or a hand-rolled server (the
  request surface is tiny — just GET requests for ~10 URL patterns).
  Decision deferred to implementation.

## Open Questions

- **Knowledge as currency?** Could knowledge itself become tradeable?
  "I'll teach you how to grow starfruit if you teach me the recipe for
  fire arrows." This is appealing but needs a trade system first.

- **Civ destruction:** Can a civilization be destroyed? What happens to
  its members (become wild? absorbed by conqueror?)? The FK table
  specifies nullify for creatures and cascade for relationships/knowledge.
  The player's civ is protected (destruction → game over). NPC civ
  destruction mechanics TBD.
