# Elven Canopy — Design Document v2

## 1. Concept

Elven Canopy is a Dwarf Fortress-inspired simulation/management game set in a forest of enormous trees. The player takes the role of a **tree spirit** — the consciousness of an ancient tree — who forms a symbiotic relationship with a village of elves living on platforms, walkways, and structures grown from the tree's trunk and branches.

The player guides 5–100+ elves as they build, create, socialize, and defend their canopy home. Elves follow the player's requests (or don't, depending on their mood and personality). The core gameplay loop is symbiotic: the tree provides shelter, materials, and food (fruit); the elves provide mana and spiritual offerings that allow the tree to grow and expand. Over time, the player extends their root network to befriend neighboring trees and expand the village across multiple trunks.

### Differentiators from Dwarf Fortress

- **Deeper emotional and social simulation.** Elves have rich personalities, social relationships, and aesthetic needs. An elf abandoning construction to sulk because her rival's poem was chosen for the Midsummer Reading is a feature, not a bug.
- **The player is embodied in the world.** As a tree spirit, the player has a physical presence, can grow and be harmed, and has a relationship with the elves rather than being an invisible god. Elves are partners, not subjects.
- **Fog of war.** Invaders and the wider world are hidden unless observed by elves or sensed through the tree's root network.
- **Visual style.** 3D voxel terrain with 2D anime-style character sprites, inspired by Final Fantasy Tactics but updated for modern aesthetics.

### Tone

Somewhere between Dwarf Fortress's emergent tragedy and anime-style melodrama — earnest, occasionally absurd, always driven by the characters' emotional lives.

---

## 2. The Player as Tree Spirit

The player is the spirit of the starting tree. This is not merely flavor — it has mechanical implications throughout the design.

### Narrative Justification

- **Camera/perception:** The player perceives the world through their tree and root network. Visibility is strongest near the trunk and branches, weaker at the edges of the root network, and absent beyond it.
- **Player authority:** The player is not a king or god — they are a symbiotic partner. Elves can be requested to do things, but may refuse based on mood, personality, or competing priorities. The relationship is negotiated.
- **Death condition:** If the player's home tree dies (burned, diseased, destroyed by invaders), the game is over. This is the only hard loss state in an otherwise sandbox experience.

### The Tree as Entity

The player's tree (and all trees in the world) are first-class entities with state, not just terrain geometry.

```rust
struct Tree {
    id: TreeId,         // UUID v4, deterministically generated
    species: TreeSpecies,
    position: VoxelCoord,
    health: f32,
    growth_level: u32,
    mana_stored: f32,
    mana_capacity: f32,
    fruit_production_rate: f32,
    carrying_capacity: f32,
    current_load: f32,
    owner: Option<PlayerId>,
    disposition: f32,   // for unowned trees: how receptive to joining your network
    // generated geometry references
    trunk_voxels: Vec<VoxelCoord>,
    branch_voxels: Vec<VoxelCoord>,
}
```

### Expansion via Root Network

- The player designates a direction to grow roots. Mana is spent over time as roots slowly extend underground.
- When roots reach another tree, a "diplomacy" phase begins: offerings and mana convince the tree to join your network.
- Each new tree provides additional buildable trunk/branch space, fruit production, and carrying capacity, but also has its own limits and possibly its own personality (a grumpy old oak demanding more offerings, an eager young willow that's fragile).
- Carrying capacity per tree is a natural constraint that prevents mega-fortress syndrome and encourages distributed village design: one tree for residences, another for workshops, another for the poetry amphitheater.
- Root network extension also expands the player's perception/fog-of-war radius.

### Multiplayer Implications

- Each player controls their own tree spirit.
- Co-op: multiple players share a starting tree or have adjacent allied trees with shared elves.
- Competitive: rival groves in the same forest, potentially warring.
- Asymmetric: one established tree, one sapling. Configured during world setup.

---

## 3. Technology Stack

### Engine: Godot 4 (4.3+)

- Free, open-source, cross-platform.
- GDExtension system for native Rust integration.
- Handles rendering, input, UI, camera, scene management.
- GDScript used only as a thin glue layer connecting the Godot scene tree to the Rust simulation.

### Simulation: Rust via gdext (godot-rust)

- All simulation logic implemented in Rust, exposed to Godot via GDExtension.
- Rust structs derive `GodotClass`, methods annotated with `#[func]`.

### Crate Structure

The Rust code is split into two crates:

- **`elven_canopy_sim`** — a pure Rust library with zero Godot dependencies. Contains all simulation logic: world state, elf AI, pathfinding, task scheduling, events, mana economy. Fully testable standalone, runnable headless.
- **`elven_canopy_gdext`** — depends on both `elven_canopy_sim` and `gdext`. Exposes the simulation to Godot as GDExtension classes. Thin wrapper only.

This separation is enforced at the compiler level. The sim crate cannot accidentally depend on rendering state, frame timing, or Godot's RNG. It also enables headless testing, fast-forward stress tests, and replay verification.

### Architecture: Simulation / Rendering Split

**Rust (sim crate) owns:**
- World voxel data, tree state, structures
- Elf state: position, personality, needs, mood, tasks, relationships
- Nav graph and pathfinding
- Task scheduling and allocation
- Event generation and narrative log
- Mana economy
- All randomness (seeded PRNG)

**Godot (GDScript glue + scene tree) owns:**
- Rendering voxel geometry as meshes (with visual smoothing/rounding)
- Rendering elf sprites as billboarded quads
- Camera control
- Player input → SimCommand translation
- UI (build menus, task lists, elf info, blueprint mode)
- Audio and particle effects
- Interpolating visual positions between sim ticks for smooth movement

**Key principle:** Godot nodes store no gameplay state. If a sprite node is destroyed, no simulation information is lost. The Rust sim is the single source of truth.

---

## 4. Determinism and Multiplayer Readiness

Although multiplayer is a future feature, the architecture is designed from day one to support deterministic lockstep-style synchronization.

### Core Principle

The simulation is a pure function: `(previous_state, commands) → (new_state, events)`. All clients running the same sim version with the same seed and the same command stream produce bit-identical results.

### The Command Interface

**All simulation mutations go through `SimCommand`. No exceptions.**

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
struct SimCommand {
    player_id: PlayerId,
    tick: u64,
    action: SimAction,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
enum SimAction {
    DesignateBuild {
        build_type: BuildType,
        voxels: Vec<VoxelCoord>,
        priority: Priority,
    },
    CancelBuild { project_id: ProjectId },
    SetTaskPriority { project_id: ProjectId, priority: Priority },
    SetSimSpeed { speed: SimSpeed },
    // future: assign elf, set zone, extend roots, etc.
}
```

In single-player, the GDScript glue translates UI actions into `SimCommand` and passes them directly to the Rust sim. In multiplayer, commands are sent to all peers first, ordered canonically, then applied.

### Determinism Requirements

**PRNG:** All randomness in the sim uses a hand-rolled xoshiro256++ PRNG with SplitMix64 seeding — no external PRNG crate dependencies. Never use OS entropy or `thread_rng()` in the sim.

**Entity IDs:** All entity IDs are UUID v4, generated deterministically from the PRNG. Implementation: take 128 bits from the xoshiro256++ stream, set the version nibble to `0100` (v4) and the variant bits to RFC 4122 compliant. All clients with the same seed generate the same IDs.

**Floating point:** Basic arithmetic (+, -, ×, ÷) is deterministic on the same platform/architecture. Avoid transcendental functions (sin, cos, sqrt) in the sim, or use fixed-point/soft-float alternatives. Initial target: determinism across x86_64 clients. Cross-architecture determinism (x86 vs ARM) deferred.

**Collection ordering:** No `HashMap` in the sim crate. Use `BTreeMap` for ordered iteration, or `Vec` with explicit sorting. Iteration order must be deterministic.

**No system dependencies:** The sim must not depend on system time, thread scheduling, memory layout, or allocation order.

### Multiplayer Synchronization (Future)

The intended model is not strict frame-locked lockstep but rather **continuous simulation with synchronized command streams.** Players stream commands to each other, commands are serialized with tick timestamps, and a Paxos-like protocol establishes canonical ordering. Each client's simulation runs continuously, applying commands at the designated ticks. Periodic state checksums (hash world state every N ticks) detect desync.

### Replays

The command pattern enables perfect replays: record the seed plus the full `SimCommand` stream. Replay by feeding the same commands to a fresh sim.

### Config Parity

In multiplayer, all clients must have identical simulation code and identical game config data. Enforced via hash comparison during session handshake: clients exchange hashes of their sim version and config files, and mismatches prevent joining.

---

## 5. Simulation Timing: Event-Driven Ticks

### Model

The simulation uses a **discrete event simulation** model rather than fixed-timestep iteration. Time is measured in fine-grained ticks, but the sim only processes events when entities have something to do.

### How It Works

- Each entity that can act (elves, trees, construction projects, etc.) has a scheduled next-event tick.
- Events are stored in a priority queue ordered by `(tick, entity_id)` — the entity_id provides deterministic tiebreaking for simultaneous events.
- Each simulation step: pop the next event, advance the sim clock to that tick, process the event (which may schedule further events), repeat.
- If the next event is 500 ticks away, those 500 ticks are skipped instantly.

### Example: Elf Movement

An elf decides to walk from platform A to platform B. The nav graph says the path is 12 edges. For each edge:
1. Compute traversal time based on edge type, elf speed, and mood modifiers.
2. Schedule a "movement complete" event at `current_tick + traversal_ticks`.
3. When the event fires, move the elf to the next node and schedule the next edge traversal.

Between movement events, the elf simply "is on edge X" and costs zero simulation time.

### Heartbeat Events

Continuous processes (mana accumulation, need decay, mood drift) are handled via periodic heartbeat events rather than lazy evaluation. Each elf schedules a recurring "update needs" event at a configurable interval (e.g., every few hundred ticks). This ensures that world state is always fully materialized and correct at any given tick, which simplifies serialization, checksumming, and debugging.

The heartbeat interval is a tunable parameter in the game config.

### Tick Granularity

Tick granularity is a tunable parameter. Because the event-driven model makes empty ticks free, the tick duration can be very fine (potentially sub-second) without performance cost. The actual granularity will be tuned based on gameplay feel — fine enough that movement and actions feel smooth, coarse enough that tick counters and heartbeat frequencies stay reasonable.

The rendering layer interpolates entity positions between ticks for smooth visual movement regardless of tick granularity.

### Fast-Forward

Fast-forward processes events faster rather than running more ticks per frame. If the village is asleep and nothing is happening, fast-forward can skip hours of game-time nearly instantly. This is a major advantage of event-driven simulation over fixed-timestep.

### Sim Speed

Sim speed (pause, 1x, 2x, 5x, etc.) is controlled via `SimAction::SetSimSpeed`. In multiplayer, all clients must agree on sim speed. The GDScript glue layer controls how many events are processed per rendered frame based on the current speed setting.

---

## 6. Entity IDs and Data Model

### Entity IDs

All entities use deterministic UUID v4 identifiers generated from the seeded PRNG. This ensures all clients in a multiplayer session generate identical IDs for entities created on the same tick.

### Core Entity Types

- **Tree** — the primary world structure and (for the player's tree) the player character. Has health, mana, growth, carrying capacity.
- **Elf** — autonomous agents with personality, needs, mood, social relationships, and task state.
- **Structure** — player-designated constructions: platforms, bridges, stairs, walls, enclosures, and special structures (poetry stages, gardens, etc.).
- **BuildProject** — an in-progress construction job linking a structure blueprint to assigned elves and progress state.
- **NavNode / NavEdge** — pathfinding graph elements (may use internal IDs rather than UUIDs for compactness, since they aren't referenced in commands or events).

### Serialization

All sim structs derive `Serialize` and `Deserialize` (via `serde`) from the start. This is a hard rule — nothing goes into a sim struct that isn't serializable.

Save format: JSON initially for debuggability and human readability. Can migrate to a binary format (bincode, MessagePack) later for performance if save files become large. Save files include a version identifier for forward-compatible migration.

Save versioning: a version number in the save header, with hand-written migration functions for breaking changes ("if version < 5, add default value for the new `mana_capacity` field").

---

## 7. World Structure

### Voxel Grid

The world is a 3D voxel grid. Each cell is approximately 2m × 2m × 2m. The grid is the simulation truth — pathfinding, collision, construction, and spatial queries all operate on it.

Initial prototype world size: 256 × 256 × 128 voxels. This is a fixed allocation for now but code should not assume specific coordinates for specific features (e.g., no hardcoded "home tree is at (128, 128, 0)").

Voxel types include:
- Air
- Trunk (natural tree trunk material)
- Branch (natural branch material)
- Grown Platform (elf-singer-constructed surface)
- Grown Wall
- Grown Stairs/Ramp
- Bridge/Walkway
- Forest Floor (dirt, grass, roots)

### Coordinate System

Coordinates are represented as a newtype or type alias so the underlying representation can change later without cascading code changes. The origin and mapping to floating-point render coordinates should be defined once in a utility module.

### Visual Smoothing

The voxel grid is the simulation truth, but **visual rendering applies smoothing and rounding** to produce the organic, curved aesthetic appropriate for elven architecture.

- Platforms designated as round/oval are still rectangular voxel clusters in the sim, but rendered with rounded meshes derived from the cluster's bounding shape.
- Walkways and staircases can follow curves visually while remaining grid-aligned underneath.
- Approaches: marching cubes, or simpler cluster-to-rounded-mesh generation.
- Rectangular construction is always available but carries an aesthetic penalty (see §9, Mood System).

### Trees

Trees are the dominant world feature. Initial prototype: large, roughly straight, redwood-style trees. A single tree might be 8–16 voxels in trunk diameter and 50–100+ voxels tall, with major branches extending at various heights.

Procedural generation: simple geometric rules initially (cylinder trunk, horizontal branch stubs at intervals). L-system or space colonization algorithms for more natural branching in the future.

Future: multiple tree species with different properties (branching patterns, fruit types, mana affinity, carrying capacity).

---

## 8. Construction: Singing

Elves are magical singers — they sing to the trees, and the tree grows in the desired shape or produces raw wood for crafting. There is no lumber industry or deforestation.

### Resource: Mana

- Mana is the primary construction resource.
- Mana is stored in trees (the tree is a mana battery) and generated by elves.
- Elf mana generation rate is baseline + modifiers from happiness, fulfillment, and personality.
- Construction draws mana from the tree's reserves.

### Mana Economy Feedback Loop

- Happy elves → more mana → faster construction → better village → happier elves (virtuous cycle).
- Neglected elves → less mana → slower construction → stagnation → unhappier elves (death spiral).
- Building morale structures (poetry stages, gardens) costs mana upfront but increases future generation.
- This feedback loop is the central mechanical tension of the game.

### Construction Flow

1. Player enters blueprint mode and designates a build project.
2. Ghost preview shows the planned structure.
3. Player confirms. Project enters the build queue with a priority.
4. Elves auto-assign based on priority, proximity, personality, and mood.
5. Assigned elf walks to the site, enters a singing animation.
6. The tree visibly grows, voxel by voxel, over time as mana is consumed.
7. Construction completes when the full mana cost has been channeled.

### Cancellation

Cancelling a build project is its own `SimAction::CancelBuild`. Cancellation logic depends on project state:
- Queued but not started: removed from queue, no cost.
- In progress: construction stops, partial structure remains (potentially usable or deconstructable).
- Complex multi-phase blueprints (e.g., platform + building on top) may have per-phase cancellation.

There is no generic undo system. Each cancellation/modification is an explicit action with its own logic.

### Build Types

- **Grow Platform** — horizontal surface on or near a branch/trunk. Circular/oval designation preferred; rectangular allowed but aesthetically penalized.
- **Grow Bridge/Walkway** — horizontal connector between two points, potentially spanning between trees. Can be curved.
- **Grow Stairs/Ramp** — vertical connector between levels, spiral or switchback.
- **Grow Enclosure/Walls** — vertical surfaces around a platform, optional roof.
- **Special structures** — poetry stage, garden, food store, workshop, dwelling. (Specific types to be defined as features are added.)

---

## 9. Elf Simulation (Deferred — Design Direction)

The emotional and social simulation is the game's key differentiator but is deprioritized for initial prototyping. This section captures the intended design.

### Personality Axes

Each elf has a personality defined by continuous axes (not boolean traits). These axes are **simulation multipliers**, not flavor text:

- **Temperament** (stoic ↔ dramatic) — magnitude of mood swings, expressiveness, contagion strength.
- **Sociability** (solitary ↔ gregarious) — social need decay rate, number of bonds sought.
- **Ambition** (content ↔ striving) — importance of recognition, status, having their work chosen.
- **Conscientiousness** (flaky ↔ dutiful) — probability of following player orders vs. self-directing, task abandonment rate.
- **Sensitivity** (thick-skinned ↔ delicate) — impact of negative social events on mood.

### Mood

Mood is a weighted composite of need satisfaction levels:
- Physical comfort (hunger, rest, shelter)
- Social fulfillment (positive interactions)
- Aesthetic fulfillment (poetry, music, beauty of surroundings)
- Purpose/recognition (contribution, acknowledgment)
- Autonomy (self-direction vs. being ordered)

Overall mood = `Σ(need_satisfaction[i] × personality_weight[i])`. Two elves in identical circumstances can have different moods for different reasons.

**Aesthetic penalty for rectangular construction:** Elves near rectangular/blocky structures accumulate a slow negative aesthetic mood modifier. Elves with high sensitivity are affected more.

### Mood Escalation

1. **Content** — normal behavior, spontaneous positive actions (humming, decorating).
2. **Restless** — slower work, more breaks, seeks lowest need.
3. **Melancholy** — significant productivity drop, withdrawn, writes sad poetry.
4. **Despondent** — refuses non-essential tasks, withdraws from social life.
5. **Crisis** — destructive behavior: destroying projects, fighting, leaving the village.

Escalation speed depends on temperament. Dramatic elves escalate fast; stoic elves simmer then snap.

### Social Graph

- Directed relationships with valence (like ↔ dislike) and intensity (acquaintance → close friend → bonded).
- Decaying event-based modifiers ("impressed by Thalion's poem" +15, decaying).
- Personality-driven formation: gregarious elves bond more, ambitious elves form rivalries, sensitive elves are hurt more by slights.
- Mood contagion: mood spreads through social proximity, weighted by relationship intensity and temperament.

### Events and Narrative Log

Every meaningful state change emits a structured event:

```rust
struct SimEvent {
    tick: u64,
    actor: ElfId,
    event_type: EventType,
    details: HashMap<String, String>,
    witnesses: Vec<ElfId>,
}
```

Events propagate through the social graph via witnesses. A templating system translates events into readable narrative text: *"Aelindra left the poetry gathering early. She seems upset about the competition results."*

The narrative log is both a player-facing feature and a debugging/design-validation tool.

### Task Decision System

Each time an elf needs to decide what to do:
1. Crisis override check.
2. Continue current task? (Conscientiousness-based check for abandonment.)
3. Score available options: player-assigned orders, self-directed need fulfillment, social opportunities, idle activities.
4. Score = urgency × personality weight × distance cost × mood modifier.
5. Pick highest score, with personality-scaled randomness.

### Mana Generation Tie-In

Elf mana generation rate is modified by overall mood. Happy, fulfilled elves are better singers — this is the mechanical link between the emotional system and the construction/expansion system.

---

## 10. Pathfinding: Nav Graph

### Rationale

The game world is not a general 3D volume — it's a network of constrained paths and small navigable areas. Platforms are small bounded surfaces. Bridges are 1–3 voxels wide. Trunk surfaces are narrow wrapping grids. Standard 3D voxel A* would be slow and produce poor paths. A graph-based approach matches the actual topology.

### Architecture

**Nodes** are locations where paths intersect:
- Where a staircase meets a platform
- Where a platform meets the trunk surface
- Where a bridge meets a platform
- The base of a trunk at the forest floor
- Entry/exit points of any navigable structure

**Edges** are traversable path segments between nodes:

```rust
struct NavNode {
    id: NavNodeId,
    position: VoxelCoord,
    structure_id: StructureId,
}

struct NavEdge {
    from: NavNodeId,
    to: NavNodeId,
    edge_type: EdgeType,
    cost: f32,
    capacity: u8,
    current_users: u8,
}

enum EdgeType {
    PlatformTraversal,
    TrunkClimb,
    Stairs,
    Bridge,
    ForestFloor,
}
```

### Two-Tier Pathfinding

1. **Graph-level A\*:** Runs on the nav graph (tens to low hundreds of nodes). Produces a route: "cross platform 3 → descend stairs → climb trunk → cross bridge → cross platform 7."
2. **Local movement within an edge:** Simple steering across small areas. Barely pathfinding — straight lines or trivial obstacle avoidance.

### Edge Costs

Different edge types have different base speeds. Climbing is slower than walking. Stairs are faster than climbing raw trunk. Costs are further modified by personality and mood (melancholy elf climbs slower, impatient elf avoids congested routes).

### Traffic

Edges have capacity. A 1-wide bridge has capacity 1–2. When a planned route includes a congested edge, the elf either waits or reroutes at the graph level. Produces emergent behavior: elves queuing at narrow bridges, taking detours.

### Graph Updates

The nav graph updates when construction completes (new platform, bridge, stairs). These updates are infrequent since construction is slow. A new platform adds a few nodes and edges. Not a per-tick operation.

### Pathfinding Frequency

Elves compute their route once when they decide on a destination. They do not recompute every tick. Recomputation is triggered only by: destination change, route becoming invalid (structure destroyed, edge blocked), or congestion encountered mid-route. This keeps pathfinding costs low even with 100+ elves.

### Forest Floor

The one area where broader 2D pathfinding may be needed. Initially treated as direct-line edges between tree bases. Full 2D A* on the ground deferred until ground-level gameplay expands.

### Flying Entities (Future)

Flying creatures (invaders, potentially tamed animals) would use a completely separate pathfinding system — true 3D movement through open space. This is architecturally separate from elf navigation and can be added independently.

---

## 11. Camera System

### Orbital Camera

A `Camera3D` node orbiting a focal point (pivot).

- **WASD** — moves the focal point horizontally, relative to the camera's facing direction. The camera moves with it; the viewing angle does not change.
- **Q/E or Left/Right arrows** — free horizontal rotation around the focal point (not snapped to 90° increments).
- **Up/Down arrows** — tilt the camera (change pitch). Clamped to 10°–80° from horizontal (i.e., 10° short of flat and 10° short of straight down).
- **Middle-mouse drag** — rotate (horizontal) and tilt (vertical) simultaneously.
- **Scroll wheel** — zoom (distance from focal point to camera).
- **R/F or Page Up / Page Down** — move the focal point up/down to navigate vertical levels, clamped to world bounds.

### Sprite Billboarding

Because horizontal rotation is free, sprites use 8-directional billboarding. Sprite frame is selected based on elf facing direction relative to current camera angle. Five unique drawn angles (front, front-side, side, back-side, back) are mirrored horizontally for the remaining three.

---

## 12. Blueprint Mode

### Activation

A hotkey (e.g., B) transitions to blueprint mode. The orbital camera's focal point becomes explicit, snapping to voxel coordinates.

### Visual Changes

- Translucent horizontal plane at the current vertical level.
- Visible crosshair/highlight at the focal point.
- Highlighted edges on terrain at the current level, showing the "floor plan."
- Existing structures become slightly transparent.
- Tree internal structure (trunk, major branches) shown as wireframe/X-ray for structural context.

### Navigation

- WASD moves the focal point, snapped to voxel increments.
- Vertical adjustment (R/F, PgUp/PgDn) moves the plane up/down one voxel height.
- Camera rotation still works.
- Visible "current level" indicator.

### Designation Flow

1. Select a build type from a palette.
2. Define the shape on the current horizontal plane (drag for rectangles, radius for circles, point sequence for freeform).
3. Ghost preview renders showing the planned structure.
4. Confirm or cancel.
5. Project enters the build queue with a priority.

### Design Principle

Constraining interaction to one horizontal slice at a time avoids the "click a voxel behind another voxel" problem. This is DF's Z-level approach mapped into a 3D viewport.

---

## 13. Sprite Art Pipeline

### Initial Prototype

Placeholder sprites from free/CC-licensed packs. Sprite dimensions and animation frame counts are configuration-driven so swapping in final art requires no code changes.

### Long-Term: AI-Generated with Layered Compositing

1. **AI-generated reference art** — full character illustrations to establish the aesthetic. Clean cel-shaded style with strong outlines, anime-inspired.
2. **AI-generated individual assets** — clothing, hairstyles, faces as flat art on transparent backgrounds.
3. **Layered assembly** — base body templates (2–3 body types) with clothing/hair/face layers composited via scripting. Each layer drawn at 5 unique angles.
4. **Result:** distinct-looking elves via layer combinations. New clothing or hairstyles = one new asset set, not per-character regeneration.

Target sprite resolution: 64×96 to 128×192 pixels.

Art style: clean outlines, flat cel-shading, limited per-character color palettes. Strong outlines hide compositing seams and maintain consistency.

AI tools to explore: Stable Diffusion/SDXL (most control via LoRAs and ControlNet), Flux (good balance of quality and controllability), Midjourney (best raw quality, `--cref` for character consistency, inpainting/edit for spritesheet completion). Experimentation needed; consistency is the hard problem.

### Future: LOD Sprites

High-detail anime sprites for close-up, low-detail chibi sprites for zoomed-out views. Deferred until camera zoom range demands it.

---

## 14. Data-Driven Configuration

### Principle

All tunable game parameters live in a `GameConfig` struct loaded from a file at startup. The sim never uses magic numbers; it reads from the config. Initial format: JSON.

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
struct GameConfig {
    tick_granularity: TickConfig,
    heartbeat_interval_ticks: u64,
    elf_base_speed: f32,
    climb_speed_multiplier: f32,
    stair_speed_multiplier: f32,
    mana_base_generation_rate: f32,
    mana_mood_multiplier_range: (f32, f32),
    platform_mana_cost_per_voxel: f32,
    bridge_mana_cost_per_voxel: f32,
    fruit_production_base_rate: f32,
    need_decay_rates: NeedDecayConfig,
    mood_thresholds: MoodThresholdConfig,
    personality_axis_ranges: PersonalityConfig,
    // ... etc
}
```

### Benefits

- Iterating on game balance doesn't require recompilation.
- Foundation for future modding, difficulty settings, and scenario design.
- Multiplayer config parity enforced by hashing the config file at session handshake.

---

## 15. Fog of War and Visibility (Future)

### Concept

The player (as tree spirit) cannot see the entire world. Visibility is determined by:
- Proximity to the player's tree trunk and branches.
- The player's root network extent.
- Line of sight from elves' positions.
- Future: magical detection structures or abilities.

### Architectural Implication

The sim always simulates everything (goblins move whether observed or not). The sync layer between sim and rendering includes a **visibility filter** per player that determines what the rendering layer is allowed to display. In multiplayer, each player has a different visibility set.

This is a future feature but the sim→rendering boundary should be designed with it in mind: the rendering layer queries "what is visible to player X" rather than "what exists in the world."

---

## 16. Population and Food

### Food

- Primary food source: fruit from trees. Fruit production is a function of tree health, growth level, and species.
- Secondary: cultivated gardens on platforms. Competes for platform space with housing/workshops.
- Tertiary: foraging on the forest floor (available but potentially dangerous due to ground-level threats).
- Elves are culturally vegan by default. Eating meat is possible in extreme circumstances but triggers strong negative social modifiers in witnesses — equivalent to a cultural taboo.

### Population Growth

- Primary: **migrants** attracted by the village's reputation. Reputation is a composite of village size, cultural output (poems, music), resident quality of life, and food surplus. Investing in elf happiness is the primary recruitment tool.
- Secondary (distant future): magical reproduction zones — mana-intensive special structures. Design TBD.
- Elf natural reproduction is extremely slow on gameplay timescales, consistent with typical fantasy elf lore.

---

## 17. Combat and Invaders (Future)

### Threat Types (Potential)

- Ground-based (goblins, beasts): climb trunks to reach the village. Can attempt to set fires.
- Flying (birds of prey, wyverns, etc.): can attack platforms directly.
- Magical (undead, corrupted spirits): varied threat profiles.
- Potentially other elf civilizations in competitive multiplayer.

### Defense (Potential)

- Elven archers (basic, first implementation of combat).
- Tamed war animals (a nod to Dwarf Fortress elves).
- Magical wards, traps, barriers (mana-powered).
- Wizard elves with combat magic.
- At high mana reserves and advanced development, defense approaches tower-defense gameplay.

### Fog of War Integration

Invaders are hidden until observed by elves or sensed by the root network. Detection is a first-class concern — scouts, watchtowers, and magical sensors become strategically important.

---

## 18. Testing Infrastructure

### Headless Simulation Tests

The `elven_canopy_sim` crate supports running the simulation without Godot. Tests construct world states programmatically, feed command sequences, run N ticks, and assert on resulting state.

### Deterministic Scenario Tests

Given a world setup, a seed, and a command sequence, the sim produces a deterministic final state. Tests serialize and compare this state. Used for:
- Regression testing (no behavior changes from refactoring).
- Lockstep correctness verification (two sim instances produce identical output).
- Performance optimization validation: benchmark a scenario, optimize code, re-run scenario, verify identical output.

### Stress Testing

Headless fast-sim mode: run thousands of ticks per second with many elves. Verify no panics, no state corruption, no desync between instances. The event-driven tick model makes this efficient — empty ticks are free.

---

## 19. Iterative Development Roadmap

Development follows iterative deepening — all areas developed concurrently at increasing fidelity.

### Phase 0: Foundations (Weeks 1–2)

- Complete Godot "Your first 3D game" tutorial.
- Build the orbital camera controller with placeholder cubes.
- Get gdext compiling: minimal Rust struct exposed to Godot.
- Set up the two-crate structure (`sim` + `gdext`).
- Define core types: `VoxelCoord`, `TreeId`/`ElfId` (UUID v4), `SimCommand`, `GameConfig`.
- Add `serde` derives to everything from the start.

### Phase 1: A Tree and an Elf (Weeks 2–4)

- Procedural generation of one tree (trunk + branches) in Rust, rendered as simple geometry in Godot.
- Nav graph for the tree surface.
- One elf as a billboard placeholder sprite, pathfinding and moving around the tree.
- Event-driven tick loop with the priority queue model.
- Basic SimCommand pipeline (even if the only command is "spawn elf at location").

### Phase 2: Construction Basics (Weeks 4–8)

- Blueprint mode: layer-based selection, ghost previews.
- Platform designation (round preferred, rectangular allowed).
- Visual smoothing on platforms (not cubes).
- Singing: elf walks to site, sings, tree grows voxel by voxel.
- Mana as a resource (tree stores it, construction spends it, elves generate it at a flat rate initially).
- Nav graph updates when construction completes.
- Multiple elves, task queue with priorities, auto-assignment.

### Phase 3: Multi-Tree and Expansion (Weeks 8–12)

- Multiple trees in the world.
- Bridges/walkways between trees.
- Stairs/ramps connecting levels.
- Root network expansion mechanic (player grows roots toward another tree, spends mana, tree joins network).
- Per-tree carrying capacity.
- Basic elf needs: hunger (eat fruit), rest (find a sleeping spot). Elves self-direct to satisfy needs.

### Phase 4: Emotional Depth (Timing Flexible)

- Personality axes affecting behavior.
- Mood system with escalating consequences.
- Social graph, relationships, contagion.
- Events and narrative log.
- Poetry readings, social gatherings.
- Mana generation tied to mood.

### Phase 5+: Distant Future

- Combat and invaders.
- Fog of war and perception systems.
- Multiple tree species.
- Crafting and non-construction jobs.
- Final AI-generated art replacing placeholders.
- Multiplayer networking.
- Sound and music.

---

## 20. Open Questions

- **Z-level visibility in the UI.** How does the player see lower platforms when upper ones occlude them? Transparency, cutaway, hide-upper-levels toggle? Deferred until multi-level construction exists.
- **Structural integrity.** Do platforms need to be attached to trunks/branches, or can you build floating structures? Probably "must be connected" for realism, but enforcement can be soft (elves warn you) rather than hard (game prevents it).
- **Day/night cycle.** Length of an in-game day in real time. Affects gameplay pacing, fruit production rates, elf sleep schedules. Tune later.
- **Weather and seasons.** Rain, wind, seasonal fruit cycles. Far future, but could tie into mood (elves love spring, hate winter storms).
- **Fire.** If invaders can set fires, fire propagation on a voxel tree is a significant simulation system. Deferred but architecturally interesting.
- **Modding.** The data-driven config and JSON format are a foundation, but full modding (custom structures, elf behaviors, invader types) would need a scripting layer or plugin system.
- **Name generation.** Elves need names. A name generator with elven phoneme rules, or curated name lists? Names feed into the narrative log and player attachment to individual elves.
