# Elven Canopy — Design Document v2

# Part I: Vision and Architecture

*What the game is, who the player is, and the technical foundations that make everything else possible.*

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

### Tree Personality (NPC Trees Only)

NPC trees have personalities expressed as preferences and aversions. An old oak might demand elaborate offerings and dislike having its bark carved with decorations. An eager young willow might love soprano singing but be fragile in storms. These preferences manifest as mechanical bonuses and penalties: a tree that loves choral singing generates extra mana when a choir performs nearby; a tree that hates golem ensoulment produces less fruit when a soul construct is stationed on its branches.

The player's tree has no personality constraints — the player can do whatever they want on their own tree, including decorating however they please. NPC trees in the network are less accommodating and may require real diplomatic effort to keep happy.

### Tree Memory

Trees are ancient, patient beings. The player's tree has accumulated memories across centuries — fragments of past events, forgotten techniques, and knowledge of buried threats. This long memory could surface through a journal or vision system: during quiet periods, the tree recalls something relevant to the current situation. Mechanically, this could unlock hints about approaching dangers, lost construction techniques, or the history of the forest. The exact implementation is open, but the core idea is that the tree's perspective is fundamentally different from the elves' — it thinks in decades, not days.

### Expansion via Root Network

- The player designates a direction to grow roots. Mana is spent over time as roots slowly extend underground.
- When roots reach another tree, a "diplomacy" phase begins: offerings and mana convince the tree to join your network.
- Each new tree provides additional buildable trunk/branch space, fruit production, and carrying capacity, but also has its own limits and its own personality (a grumpy old oak demanding more offerings, an eager young willow that's fragile). Accommodating each tree's preferences is part of managing a multi-tree network.
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

The Rust code is split into three crates:

- **`elven_canopy_sim`** — a pure Rust library with zero Godot dependencies. Contains all simulation logic: world state, elf AI, pathfinding, task scheduling, events, mana economy. Fully testable standalone, runnable headless.
- **`elven_canopy_gdext`** — depends on both `elven_canopy_sim` and `gdext`. Exposes the simulation to Godot as GDExtension classes. Thin wrapper only.
- **`elven_canopy_music`** — a standalone Palestrina-style polyphonic music generator with Vaelith (elvish) lyrics. Produces MIDI and LilyPond output. Independent of both the sim and Godot; will be integrated into the game runtime in a future phase (see §21).

The sim/gdext separation is enforced at the compiler level. The sim crate cannot accidentally depend on rendering state, frame timing, or Godot's RNG. It also enables headless testing, fast-forward stress tests, and replay verification.

### Architecture: Simulation / Rendering Split

**Rust (sim crate) owns:**
- World voxel data, tree state, structures
- Creature state: position, species, personality, needs, mood, tasks, relationships
- Nav graph and pathfinding
- Task scheduling and allocation
- Event generation and narrative log
- Mana economy
- All randomness (seeded PRNG)

**Godot (GDScript glue + scene tree) owns:**
- Rendering voxel geometry as meshes (with visual smoothing/rounding)
- Rendering creature sprites as billboarded quads
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

## 5. Data-Driven Configuration

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

# Part II: Simulation Core

*The engine beneath the game: how time advances, how entities are modeled, how the world is structured, and how creatures navigate it.*

## 6. Simulation Timing: Event-Driven Ticks

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

## 7. Entity IDs and Data Model

### Entity IDs

All entities use deterministic UUID v4 identifiers generated from the seeded PRNG. This ensures all clients in a multiplayer session generate identical IDs for entities created on the same tick.

### Core Entity Types

- **Tree** — the primary world structure and (for the player's tree) the player character. Has health, mana, growth, carrying capacity.
- **Creature** — all living entities (elves, capybaras, future species). A single `Creature` struct with a `species: Species` field. Behavioral differences (speed, heartbeat interval, allowed edge types, ground-only restriction) come from `SpeciesData` in the game config — Dwarf Fortress-style data-driven design. No code branching per species.
- **Structure** — player-designated constructions: platforms, bridges, stairs, walls, enclosures, and special structures (poetry stages, gardens, etc.).
- **BuildProject** — an in-progress construction job linking a structure blueprint to assigned elves and progress state.
- **NavNode / NavEdge** — pathfinding graph elements (may use internal IDs rather than UUIDs for compactness, since they aren't referenced in commands or events).

### CreatureAction

Every creature has an active `CreatureAction` describing what it is currently doing — moving, attacking, constructing, etc. Actions are optionally tied to one or more jobs.

```rust
struct CreatureAction {
    action_type: ActionType,     // Move, Attack, Construct, Idle, ...
    start_tick: u64,
    end_tick: u64,               // may be in the future (action still in progress)
    start_coord: VoxelCoord,     // for movement: origin
    end_coord: VoxelCoord,       // for movement: destination
    job_id: Option<JobId>,
}
```

**Animated movement:** The rendering layer uses CreatureAction to interpolate visual positions. Each frame, Godot reads the action's start/end coordinates and start/end ticks, then places the sprite at the proportional position based on the current time within the action. This produces smooth movement without the sim needing to update positions every frame.

### Serialization

All sim structs derive `Serialize` and `Deserialize` (via `serde`) from the start. This is a hard rule — nothing goes into a sim struct that isn't serializable.

Save format: JSON initially for debuggability and human readability. Can migrate to a binary format (bincode, MessagePack) later for performance if save files become large. Save files include a version identifier for forward-compatible migration.

Save versioning: a version number in the save header, with hand-written migration functions for breaking changes ("if version < 5, add default value for the new `mana_capacity` field").

---

## 8. World Structure

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

### Structure Membership

Voxels track not just their material type but which logical structure they belong to — e.g., "this wood voxel belongs to the trunk of tree #7" vs. "this wood voxel belongs to the east observation platform." This distinction matters for construction, deconstruction, structural integrity calculations, and carrying capacity accounting. A single voxel belongs to exactly one structure.

### Coordinate System

Coordinates are represented as a newtype or type alias so the underlying representation can change later without cascading code changes. The origin and mapping to floating-point render coordinates should be defined once in a utility module.

### Visual Smoothing

The voxel grid is the simulation truth, but **visual rendering applies smoothing and rounding** to produce the organic, curved aesthetic appropriate for elven architecture.

- Platforms designated as round/oval are still rectangular voxel clusters in the sim, but rendered with rounded meshes derived from the cluster's bounding shape.
- Walkways and staircases can follow curves visually while remaining grid-aligned underneath.
- Approaches: marching cubes, or simpler cluster-to-rounded-mesh generation.
- Rectangular construction is always available but carries an aesthetic penalty (see §18, Mood System).

### Trees

Trees are the dominant world feature. Initial prototype: large, roughly straight, redwood-style trees. A single tree might be 8–16 voxels in trunk diameter and 50–100+ voxels tall, with major branches extending at various heights.

Procedural generation: simple geometric rules initially (cylinder trunk, horizontal branch stubs at intervals). L-system or space colonization algorithms for more natural branching in the future.

Future: multiple tree species with different properties (branching patterns, fruit types, mana affinity, carrying capacity).

### Seasons

Seasons affect both the tree's appearance and gameplay.

**Visual changes:** Leaves change color in autumn. Spring brings new growth and blossoms. Winter shows frost on branches and bare canopy for deciduous species. These changes reinforce that the tree is a living being, not static building material.

**Autumn leaf drop:** Certain tree species shed their leaf voxels in autumn. When a leaf voxel detaches, it is removed from the world grid — any nav nodes on that voxel are invalidated, and creatures can no longer climb on or traverse those leaves. This can isolate parts of the canopy that were only reachable via leaf surfaces, creating seasonal accessibility challenges. Leaf regrowth in spring restores these paths.

**Winter hazards:** Snow and frost accumulate on platforms and walkways. Elves need warm clothing (coats, cloaks) or suffer cold-related mood and health penalties. Uncleared snow on walkways creates a slipping hazard — elves traverse snowy edges more slowly and risk injury. Snow clearing becomes a maintenance task, and covered or enclosed walkways become more valuable in winter.

The sim tracks the current season as part of world state. Seasonal effects are data-driven (per tree species and climate zone) so they can be tuned or disabled in config.

---

## 9. Structural Integrity

Structures in the canopy must obey (approximate) physics. A platform cantilevered far from the trunk with heavy loads on it should be at risk of breaking off. The design direction is **voxel-based finite element modeling** — using the existing voxel grid as the finite element mesh, avoiding the need for separate structural member identification. The exact implementation method has not been decided, but the general approach is outlined below.

### Why FEM on Voxels

The voxel grid solves the hardest part of finite element analysis for free: mesh generation. Every solid voxel is a cube element. Connectivity is trivial (face-adjacent voxels share nodes). Boundary conditions are obvious (where the tree meets the ground, those nodes are fixed). No meshing step, no heuristic structural member identification — the simulation operates directly on the world geometry as it exists.

This generality matters because elven construction is organic and curved. Trying to classify every voxel cluster as a "beam" or "column" or "strut" would be fragile and full of edge cases. The FEM approach handles any geometry — arches, spirals, irregular platforms, hollow trunks — without special-casing.

### What the Simulation Captures

The physics that matter for gameplay:

- **Bending under load.** A horizontal platform cantilevered off a trunk bends, with maximum stress at the connection point. Longer spans and heavier loads mean more stress. Players learn to keep platforms reasonably sized or add support.
- **Support struts work.** A diagonal brace under a platform converts bending stress into compression along the strut, dramatically reducing stress at the connection. Players discover that supports and braces make structures stronger.
- **Thickness matters.** A 3-voxel-thick platform is much stronger than a 1-voxel-thick one. The second moment of area handles this automatically — no special case needed.
- **Arches are efficient.** An arch distributes load into compression on both sides. The simulation rewards good structural design.
- **Asymmetric loading is dangerous.** Weight centered on a span is much safer than weight at the tip of a cantilever. Thirty elves gathering at the far edge of a platform is a real threat.
- **Cascading failure.** When a voxel fails under stress, its load redistributes to neighbors, potentially overloading them in turn. A single failure can cascade into a dramatic multi-level collapse.

### Simplifications vs. Real Engineering FEM

- **Isotropic material.** Real wood has grain direction. Game wood has a single stiffness value and a single strength value per material type.
- **Linear elastic only.** No plastic deformation, no creep. Voxels are either intact or failed, no intermediate damage state.
- **Static analysis only.** No vibration, no dynamic impact simulation. Falling debris applies a damage impulse to whatever it hits, without simulating the collision dynamics.
- **No buckling analysis.** Columns have a maximum compressive load. Euler buckling is not modeled.

### Implementation Approaches (Undecided)

Two candidate approaches, both mathematically equivalent:

**Direct solve.** Assemble a global stiffness matrix K from the per-voxel element stiffness matrices (each a constant 24×24 matrix for a cube element, reused for every voxel of the same material). Apply gravity loads and boundary conditions. Solve Ku = f for nodal displacements using a sparse solver (conjugate gradient or sparse Cholesky). Compute stress from displacement gradients. Compare to material strength.

For a structure of N voxels, the system has roughly 3N unknowns (3 displacement DOFs per unique node). A platform of 200 voxels → ~600 unknowns. A large structure of 2000 voxels → ~6000 unknowns. Sparse solvers handle these sizes in well under a millisecond.

**Iterative relaxation.** Same physics, simpler implementation. Each voxel starts with zero displacement. Iterate: for each voxel, compute the net force on it (gravity + elastic forces from neighbors based on relative displacement), adjust its displacement to reduce imbalance. Repeat until converged or for a fixed iteration count. Check bond forces between neighbors against breaking thresholds.

This never forms the global matrix — it just loops over voxels updating each one locally. Perhaps 50 lines of core logic. Convergence takes tens to low hundreds of iterations. Slower than a direct solve for large structures, but simpler to implement and debug. Can stop early for an approximate answer, which is sufficient for gameplay.

### When to Run

Not every tick. Structural analysis is triggered by:

- **Construction completion** — new structure adds load to existing supports.
- **Destruction events** — fire, combat, or deconstruction removes voxels.
- **Significant load changes** — many creatures gathering on one structure.
- **Blueprint preview** — show a stress heat map during construction planning so the player can identify weak points before building. "This connection will be under high stress — consider adding a support strut."
- **Periodic heartbeat** — a slow background check (every few hundred ticks) as a safety net.

### Failure and Cascade

When a voxel's stress exceeds its material strength:

1. The voxel is marked as failed and removed from the structure.
2. A connectivity check determines whether any portion of the structure has become disconnected from its foundation.
3. Disconnected chunks detach and fall. The fall is modeled as an event: compute fall height, schedule an impact event, apply damage to whatever is below (other structures, creatures, the ground).
4. The impact may trigger secondary structural checks on the structures it hits.
5. Load redistribution from the failed voxel may overload neighbors, causing chain failure over multiple ticks of game time.

This cascade produces the dramatic scenario described in the design discussions: fire burns through a support, a platform detaches, debris crashes down onto a lower walkway, overloading it, which partially collapses onto the forest floor.

### Determinism

The structural simulation must be deterministic for multiplayer and replay support. Key considerations:

- **Iteration order** must be fixed (e.g., always iterate in voxel-coordinate order) so that floating-point rounding produces identical results on all clients.
- **Fixed-point arithmetic** is an alternative that eliminates floating-point determinism concerns entirely. For a game where stress values only need to be approximate, 32-bit fixed-point is likely sufficient.
- **No transcendental functions** are needed — the simulation uses only basic arithmetic (addition, multiplication, division), which is deterministic on the same architecture.

The structural simulation operates on the voxel grid (sim truth) and is triggered by `SimCommand`-driven state changes, so all clients process the same structural checks in the same order.

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

### Large Creatures (Future)

Default creatures occupy a 1×1×1 voxel footprint. Larger creatures — wood golems, dragons, dinosaurs — may need a 2×2 (or larger) footprint. This requires a separate or augmented nav grid that only includes paths wide and tall enough for the larger body. Large creatures can traverse narrow platforms or low ceilings, but slowly (edge cost multiplier). The exact implementation is TBD, but the nav graph's edge capacity and cost system already provides hooks for this.

### Flying Entities (Future)

Flying creatures (invaders, potentially tamed animals, winged elves) would use a completely separate pathfinding system — true 3D movement through open space. This is architecturally separate from elf navigation and can be added independently.

### Voxel-Derived Nav Graph (Current Implementation)

The nav graph is built directly from the voxel world rather than from hardcoded geometric patterns. Every air voxel that is face-adjacent to at least one solid voxel becomes a nav node. Edges connect nav nodes within the 26-neighborhood (face, edge, and vertex adjacent). This approach means the navigation topology automatically reflects the actual world geometry.

**Node creation:** Air voxels at y≥1 with at least one solid face neighbor. Each node carries a `surface_type` derived from its adjacent solid voxel (priority: voxel below first, then horizontal/above). Ground nodes above `ForestFloor` get surface type `ForestFloor`; nodes clinging to the trunk get `Trunk`; etc.

**Edge creation:** 26-connectivity (13 positive-half neighbors to avoid duplicate bidirectional edges). Edge types and costs are derived from the surface types of both endpoints. Euclidean distance / speed determines cost.

**Why 26-connectivity?** Face-only edges (6-connectivity) leave the air shell around thin geometry (radius-1 branches) disconnected. 26-connectivity ensures all air touching the same surface stays connected.

### Dynamic Nav Updates (Future)

When a voxel changes (construction/destruction), the nav graph can be updated incrementally:
1. Invalidate nav nodes within Manhattan distance 1 of the changed voxel.
2. Re-derive affected nodes and edges.
3. Re-check connectivity for invalidated paths.

This is safe for determinism because voxel changes go through `SimCommand`, so all clients process them in the same order.

---

# Part III: Gameplay Systems

*What the player and elves actually do: build, eat, fight, explore, and manage the flow of resources through the canopy.*

## 11. Construction: Singing

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
3. Player confirms. Project enters the build queue with a priority and a suggested choir size.
4. A construction choir assembles: elves sign up based on availability, proximity, skill, vocal compatibility, and social factors. The player can adjust choir size — smaller choirs are easier to coordinate but slower; larger choirs are faster but harder to harmonize.
5. The choir walks to the site and sings together for a work session. This is a committed group action, not individual tasks — elves don't dip in and out mid-song.
6. The tree visibly grows, voxel by voxel, over time as mana is consumed. Construction speed and quality depend on the choir's harmony, which is affected by the singers' relationships, skill levels, and mood.
7. Construction completes when the full mana cost has been channeled.

An elf who abandons a choir mid-session disrupts the harmony and annoys the other singers (a social consequence), so conscientiousness matters for choir reliability. A well-matched choir of friends who enjoy working together produces subtly better results than a grudgingly assembled group. The player can hand-pick a choir for important structures, but for routine construction, auto-assignment based on the project's priority is usually sufficient.

### Cancellation

Cancelling a build project is its own `SimAction::CancelBuild`. Cancellation logic depends on project state:
- Queued but not started: removed from queue, no cost.
- In progress: construction stops, partial structure remains (potentially usable or deconstructable).
- Complex multi-phase blueprints (e.g., platform + building on top) may have per-phase cancellation.

There is no generic undo system. Each cancellation/modification is an explicit action with its own logic.

### Conservation of Mass

Each tree maintains a quantity of stored wood material, derived over time from photosynthesis (accelerated well beyond real-world rates for gameplay purposes; elves or magic may further boost the rate). Construction and fruit growth consume stored mass. Deconstruction and carving return mass to the pool.

The total storable mass is roughly proportional to the tree's exterior surface area (geometrically, the exterior is the most metabolically active part of a real tree). If the player continues constructing while stored mass is at zero, construction can optionally proceed (perhaps with a warning) by depleting wood voxels from the trunk interior. This risks reducing structural integrity over time — hollowing out a trunk to build platforms is a meaningful trade-off.

### Build Types

- **Grow Platform** — horizontal surface on or near a branch/trunk. Circular/oval designation preferred; rectangular allowed but aesthetically penalized. The UI lets the player select a center point (using the camera focal point), choose rectangular or circular shape, and adjust dimensions via on-screen buttons or keyboard shortcuts.
- **Grow Bridge/Walkway** — horizontal connector between two points, potentially spanning between trees. Can be curved. Design TBD: define start and end points, with optional control points for curves.
- **Grow Stairs/Ramp** — vertical connector between levels, spiral or switchback.
- **Grow Enclosure/Walls** — vertical surfaces around a platform, optional roof.
- **Carve Hole** — remove material from existing structures (e.g., cutting a doorway through a wall, hollowing out trunk space for storage). Returns mass to the tree's pool.
- **Grow Branches/Boughs** — extend the tree itself, increasing photosynthesis surface area and fruit-bearing capacity. A direct investment in the tree's long-term productivity.
- **Ladders** — rope or wood, a cheaper and faster vertical connector than stairs but slower to traverse and lower capacity.
- **Special structures** — poetry stage, garden, food store, workshop, dwelling. (Specific types to be defined as features are added.)

### Buildings vs. Furnishing

Construction follows a two-phase model. First, the player designates generic building geometry — walls, floors, roofing. Then the interior is furnished to give the building a purpose: a workshop, a dwelling, a dining hall, a library, an enchanting studio, a theater, etc. This means a rectangular enclosure can be repurposed from a granary to a barracks without demolishing it — only the furnishings change.

Other potential furnishable structures: dance halls, artistic gazebos, animal training zones, food preparation areas, mana crystal growth facilities.

### Defenses

Defensive structures are a specialized category of construction:
- **Ballista turrets** — mounted ranged weapons, likely requiring both construction and an assigned operator.
- **Magic traps and wards** — mana-powered defenses. Design TBD.

### Visual Customization

Eventually, everything should support extensive visual customization. Craftselves could decorate walkways with sculpted flourishes depicting various subjects. Elves who like or dislike those subjects would get mood modifiers from the decorations. Elves might also decorate their own homes autonomously, reflecting their personality and relationships.

### Dwellings and Home Assignment

The player can assign an elf to a specific dwelling, but elves also self-select. Unassigned elves assess available homes based on location, quality, proximity to friends, and personal preferences, and move in if the dwelling is unoccupied. Elves may negotiate home swaps through social interaction ("I like your place near the garden, and you'd be closer to the workshop in mine"). Elves may pair up and cohabitate.

Even an elf assigned to a dwelling by the player might disobey — either because they are generally unhappy or specifically unhappy with the assignment. Conscientiousness and mood both factor into compliance.

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

1. Select a build type from a palette (organized as a hierarchy once the number of build options grows large — e.g., Structures → Platforms → Circular Platform).
2. Define the shape on the current horizontal plane (drag for rectangles, radius for circles, point sequence for freeform).
3. Ghost preview renders showing the planned structure.
4. Confirm or cancel.
5. Project enters the build queue with a priority.

### Batch Blueprinting

The player can queue many construction tasks in blueprint mode without starting any of them. Planned structures render in a translucent blue-gray to distinguish them from active construction. When the player commits a batch of blueprints, the system automatically determines construction ordering based on structural dependencies — support structures before platforms, platforms before buildings on those platforms.

The blueprint system should display warnings for:
- **Structural instability** — planned structures that lack sufficient support.
- **Inaccessible locations** — build sites that no elf can currently path to.
- **Insufficient support** — platforms or buildings that would exceed carrying capacity.

These are advisory warnings, not hard blocks — the player can proceed anyway if they have a plan to address the issues (e.g., building access stairs as part of the same batch).

### Design Principle

Constraining interaction to one horizontal slice at a time avoids the "click a voxel behind another voxel" problem. This is DF's Z-level approach mapped into a 3D viewport.

---

## 13. Population and Food

### Food

- Primary food source: fruit from trees. Fruit production is a function of tree health, growth level, and species.
- Secondary: cultivated gardens on platforms. Competes for platform space with housing/workshops.
- Tertiary: foraging on the forest floor (available but potentially dangerous due to ground-level threats).
- Elves are culturally vegan by default. Eating meat is possible in extreme circumstances but triggers strong negative social modifiers in witnesses — equivalent to a cultural taboo.

### Fruit Variety

Trees should produce a variety of fruits, potentially including fictional species. Different fruits have different properties:
- **Longevity** — some fruits keep well, others spoil quickly.
- **Nutrition and taste** — affects how satisfying a meal is.
- **Processability** — some fruits can be dried or milled into flour for baking lembas-like breads.
- **Magical properties** — certain rare fruits may serve as potion ingredients.

Open question: How does the player direct which fruits a tree produces? Possible approaches include a UI for selecting fruit types per tree, innate limits based on tree species (some trees can only produce certain fruits without magical enhancement), or constructable collection zones that promote growth of particular fruits.

### Food Storage and Preparation

Hungry elves may seek fruit directly from the tree, but a functioning village needs infrastructure:
- **Storage structures** — dedicated buildings for food stockpiling. May require magical refrigeration to prevent spoilage.
- **Milling and drying** — processing fruit into flours and preserved forms, enabling baked goods (lembas-style breads).
- **Cooking** — elaborate cooking procedures using prepared ingredients. Well-cooked meals provide stronger mood benefits than raw fruit. This is a full crafting subsystem.
- **Magical brewing** — rare fruits processed into potions with various effects. Requires specialized facilities (alchemist's workshop or equivalent).

### Population Growth

- Primary: **migrants** attracted by the village's reputation. Reputation is a composite of village size, cultural output (poems, music), resident quality of life, and food surplus. Investing in elf happiness is the primary recruitment tool.
- Secondary (distant future): magical reproduction zones — mana-intensive special structures. Design TBD.
- Elf natural reproduction is extremely slow on gameplay timescales, consistent with typical fantasy elf lore.

---

## 14. Logistics

Construction (§11) consumes mass and mana. Food (§13) must move from trees to storage to kitchens to dining halls. As the village grows beyond a handful of elves, ad-hoc resource movement becomes unsustainable. A logistics system manages the flow of materials, food, and goods.

### Kanban-Style Flow (Aspirational)

The logistics system should follow kanban-like principles: resources flow through defined stages (harvest → storage → processing → delivery), with clear indicators of bottlenecks and throughput. This might require dedicated elven logisticians — elves assigned to manage and optimize resource flow rather than performing the labor themselves.

Critically, logistics should be **spatial, not abstract**. Elves physically carry materials along walkways and bridges. Bottlenecks are visible as traffic jams on narrow paths, not just numbers on a screen. A poorly connected workshop starves for materials because the hauling route is too long or too congested — the player can *see* the problem and solve it by building a shorter path or a closer storage depot. This spatial grounding keeps logistics feeling like part of the world rather than a spreadsheet overlay.

### Mana Crystal Growth (Future)

Specialized facilities for growing mana crystals — a secondary mana storage and generation system. Mana crystals could serve as portable mana reserves, trade goods, or power sources for enchantments and constructs. The logistics system would need to handle crystal distribution alongside food and materials.

---

## 15. Creatures and Species

Beyond elves and capybaras, the world should eventually be populated with a variety of creatures. All creatures use the same `Creature` struct with species-specific behavior driven by `SpeciesData` (see §7).

### Animal Species (Future)

- **Birds** — requires flight navigation (see §10, Flying Entities). Could serve as scouts, messengers, or tamed war animals.
- **Monkeys** — climbing specialists. Would use trunk and branch edges heavily, potentially with unique climbing speed bonuses.
- **Boars** — ground-level creatures. Forest floor dwellers, potentially tameable or a threat.
- **Dinosaurs** — various species, likely requiring multi-voxel creature logic (see §10, Large Creatures). A deliberate fantasy/anachronism element. Could be toggleable during world generation for players who prefer a more traditional fantasy bestiary.

### Winged Elves (Future)

A potential elf variant: fairy or winged elves. Mechanically similar to standard elves but capable of flight, using the flying entity pathfinding system. To maintain balance, they would need compensating downsides — lower mana generation, physical fragility, or social isolation from ground-dwelling elves. Design TBD.

### Wood Golems / Constructs (Future)

Animated wood constructs created through advanced magic. Aesthetic inspiration: the wraith constructs of Warhammer 40K's Aeldari — graceful but eerie, clearly alive but not organic. These could serve as heavy laborers, defenders, or both.

Wood golems tie into the soul mechanics system (§19) — a golem might be animated by the soul of a deceased elf, with implications for the elf's friends and the golem's behavior.

Open question (see §27): What should these be called in-world? "Golem" is generic. Something more elven/arboreal — perhaps a word from the constructed Elvish language (§20) — would fit better.

---

## 16. Combat and Invaders (Future)

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
- Ballista turrets and other constructed defensive emplacements.
- At high mana reserves and advanced development, defense approaches tower-defense gameplay.

### Elf Weapons

Elven weapons are primarily wood-based, consistent with the tree-symbiosis theme:
- **Bows and arrows** — the default ranged weapon. Can be tipped with obsidian or other materials.
- **Spears** — versatile melee weapon, effective from platform edges.
- **Clubs** — simple, effective in close quarters.

Weapons can be crafted from plain wood or enhanced with obsidian tips, magical enchantments, or other materials as the crafting system expands.

### Fire Simulation (Future)

Fire on a voxel-based tree is a significant simulation challenge with multiple layers of complexity.

**Architecture:** Fire should be modeled as an entity-level system rather than a per-voxel property. A `Fire` entity owns a set of currently-burning voxel coordinates, manages spread logic, and can be targeted by firefighting actions. This keeps the voxel struct lean and fits naturally into the event queue — a burning voxel schedules "spread check" events at intervals.

**Staged implementation:**

1. **Stage 1 — Basic spread.** Binary burning state. Each wood voxel can be normal, smoldering, burning, charred, or destroyed. Fire spreads probabilistically to face-adjacent wood voxels (probability from the PRNG for determinism). Fire destroys a voxel after N ticks. No temperature, no wind, no moisture. Enough to make fire scary and test structural consequences.

2. **Stage 2 — Heat and ignition thresholds.** Fire entities maintain a heat radius. Nearby voxels accumulate heat before igniting, rather than catching fire instantly. Living green wood resists ignition (high threshold); dead or dry wood catches easily (low threshold). This is cheaper than full thermal diffusion because heat is only calculated near active fires, not across the whole world.

3. **Stage 3 — Environmental factors.** Wind direction biases fire spread. Moisture from weather and seasons affects ignition thresholds (dry summer = fire season). Firefighting tasks added to the job system — elves can fight fires with water, smothering, or magical suppression. This requires the AI to weigh social bonds against self-preservation: an elf running into a burning platform to rescue a friend is an incredible emergent story moment.

4. **Stage 4 — Fire as ecological force.** Fire is not purely destructive. Ground-level fires clear undergrowth and enable vigorous regrowth, reflecting real forest ecology. Controlled burns become a deliberate strategy. Post-fire regrowth could be faster and more lush than normal growth, rewarding players who manage fire rather than just fearing it.

**Structural consequences:** When fire destroys load-bearing voxels, structural integrity of everything above them changes. A fire that burns through a support strut could collapse a platform, scattering burning debris and starting secondary fires. This cascade (fire → structural failure → nav graph invalidation → elf path recalculation → panic behavior) is dramatically compelling but requires the structural integrity system to operate dynamically, not just during construction planning.

**Strategic depth:** Offensive fire magic is powerful but double-edged in a civilization built on living wood. This creates a genuine trade-off: fire spells might be the most effective weapon against invaders, but a misfire or wind shift could threaten the player's own tree.

**Determinism:** All fire spread probabilities, wind patterns, and moisture levels must be derived from the seeded PRNG and sim state. No nondeterministic sources.

### Military Organization

Open question: How should the player organize military units? Dwarf Fortress's military system covers many bases but is notoriously awkward. RimWorld offers finer-grained, freeform control (no explicit military squads) but would scale poorly to large populations. The right answer probably involves some hybrid: named squads for organized defense, but flexible enough for small-scale responses.

### Fog of War Integration

Invaders are hidden until observed by elves or sensed by the root network. Detection is a first-class concern — scouts, watchtowers, and magical sensors become strategically important.

---

## 17. Fog of War and Visibility (Future)

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

# Part IV: Elf Simulation and Culture

*The game's key differentiator: how elves think, feel, create, and relate to each other. These systems are the deepest and most ambitious part of the design — they turn a building game into a story generator.*

## 18. Elf Simulation (Deferred — Design Direction)

The emotional and social simulation is the game's key differentiator but is deprioritized for initial prototyping. This section captures the intended design.

### Personality Axes

Each elf has a personality defined by continuous axes (not boolean traits). These axes are **simulation multipliers**, not flavor text:

- **Temperament** (stoic ↔ dramatic) — magnitude of mood swings, expressiveness, contagion strength.
- **Sociability** (solitary ↔ gregarious) — social need decay rate, number of bonds sought.
- **Ambition** (content ↔ striving) — importance of recognition, status, having their work chosen.
- **Conscientiousness** (flaky ↔ dutiful) — probability of following player orders vs. self-directing, task abandonment rate.
- **Sensitivity** (thick-skinned ↔ delicate) — impact of negative social events on mood.

### Personality Dynamics

Personality traits are a mix of hereditary and environmental factors. Traits fluctuate over time, and the *degree* of fluctuation varies per elf — some are emotionally volatile (analogous to bipolar disorder), others are merely moody, and others are remarkably stable. This fluctuation rate is itself a personality parameter.

Elves also vary in how strongly they react to the destruction of their art or creative work. Some shrug it off; others are devastated.

### Multi-Dimensional Emotional State

Rather than a single happiness/stress metric (as in Dwarf Fortress or RimWorld), elves track multiple independent emotional dimensions, any combination of which can be present or absent at a given time:

- **Momentary joy** — immediate pleasure from food, beauty, social warmth.
- **Fulfillment** — satisfaction from meaningful work, creative expression, purpose.
- **Sorrow** — grief from loss, loneliness, witnessing suffering.
- **Stress** — pressure from overwork, unmet needs, conflicting demands.
- **Pain** — physical discomfort from injury, hunger, exhaustion.
- **Immediate fear** — response to present danger (invaders, fire, structural collapse).
- **Anxiety** — worry about future threats, social standing, unresolved conflicts.

An elf can simultaneously feel fulfilled by their craft and anxious about an approaching threat. The interaction of these dimensions drives behavior in ways a single mood number cannot — a joyful but stressed elf acts differently from a sorrowful but calm one.

### Desires and Needs

Core desires that drive elf behavior:
- **Attention** — audience for performances, quality time with a partner or friend.
- **Delicious food** — not just satiation but enjoyment of well-prepared meals.
- **Self-expression** — writing poetry, creating art, decorating, performing.
- **Sleep** and **hunger** — basic physiological needs.
- **Love** — romantic attachment, companionship.

### Hedonic Adaptation

Elves adapt to their circumstances over time. A newly built luxury dwelling provides a strong mood boost that fades as it becomes the new normal. Conversely, a sudden downgrade feels worse than long-term poverty.

Adaptation should be **asymmetric**: elves adapt *upward* (luxury becomes normal) faster than they adapt *downward* (discomfort stays painful longer). This prevents a treadmill dynamic where the player must constantly upgrade housing just to maintain mood — improvements are still meaningful even after the novelty fades, because the *absence* of that improvement would hurt.

Early implementation can be straightforward: apply a decaying "novelty" modifier to changes in living conditions, with different decay rates for positive vs. negative changes. More sophisticated adaptation modeling can come later.

### Mood (Composite)

Mood is a weighted composite of need satisfaction levels, informed by the emotional dimensions above:
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
    details: BTreeMap<String, String>,
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

### Apprenticeships and Skill Transfer

Elves learn skills primarily through proximity and collaboration. When a less skilled elf works alongside a more experienced one (e.g., in the same construction choir, at the same workshop), the junior elf passively absorbs knowledge over time. The transfer rate depends on the mentor's teaching aptitude (a trait or skill) and the apprentice's learning speed.

This happens naturally without player intervention — repeatedly assigning elves to the same projects implicitly creates mentorship pairs. The player can also explicitly designate a mentorship when a specific skill needs to be transferred urgently, though the mentor may refuse if they dislike the apprentice.

Mentorship creates social bonds. An elf who learned to sing from Thalion feels loyalty to Thalion. If Thalion dies, the apprentice grieves harder than a stranger would. This means the skill system feeds the social graph naturally, and losing a veteran elf has consequences beyond their personal productivity — their apprentices lose a mentor and a friend.

The player faces real trade-offs: do you send your best singer to the critical construction project, or keep her teaching the next generation?

### Inter-Tree Cultural Drift

When the village expands across multiple trees, elves on different trees gradually develop slightly different cultures — different aesthetic preferences, different attitudes toward soul constructs, different musical traditions, different slang. This drift is a natural consequence of physical separation and the influence of each tree's personality on its residents.

Cultural differences give multi-tree expansion real texture beyond "more building space." When elves from different trees interact — at shared festivals, on construction projects, during migrations — cultural friction (or fascination) produces social events. An elf who moves from a conservative oak community to a progressive willow community might feel liberated or unmoored, depending on personality.

### Mana Generation Tie-In

Elf mana generation rate is modified by overall mood. Happy, fulfilled elves are better singers — this is the mechanical link between the emotional simulation and the construction/expansion loop described in §11. The entire elf simulation exists in service of this connection: every personality quirk, every social interaction, every poem and rivalry and heartbreak ultimately affects how much mana the village generates, which determines how fast the player can build. The simulation is not decoration on top of the management game — it *is* the management game.

---

## 19. Soul Mechanics

Elven souls are bound to trees, creating a deep thematic and mechanical connection between the elves and their home.

### Death and the Soul

When an elf dies, their soul passes into their home tree by default. The soul persists within the tree and can be interacted with in limited ways (communing, drawing on memories, etc.).

### Resurrection

Expensive magic rituals can resurrect a slain elf whose soul resides in the tree. This is not free — it costs significant mana and may have unpredictable personality implications. The resurrected elf might not be quite the same person they were before. Friends and family may react with joy, unease, or both.

### Soul-Powered Constructs

Elf souls in the tree can be bound into wood golems (see §15), similar to Warhammer 40K's wraith constructs. This is a culturally loaded act — some elven communities may accept it as an honor (the deceased continues to serve), while others find it distasteful or even horrifying. Cultural attitudes toward soul constructs could be a source of internal social tension.

### Soul Threats

- **Enemy necromancers** or dark magic users might snatch elf souls from the recently slain before they can return to the tree. If captured, that soul may be lost permanently — a devastating blow to morale and to the deceased's friends and family.
- **Unbonded elves** — elves not bonded to any tree can die permanently with no possibility of resurrection. This gives elves a powerful incentive to join a tree's civilization and creates real stakes for elves operating far from home (scouts, military campaigns).

### Implications

The soul system means death is not binary. An elf's death is sad but recoverable (at great cost) — unless their soul is lost. This creates a spectrum of loss that feeds into the emotional simulation: grief for a fallen comrade, hope for resurrection, horror if the soul is stolen, unease around a resurrected elf who seems... different.

---

## 20. Constructed Elvish Language and Procedural Poetry

The game uses a purpose-built constructed language ("conlang") for Elvish. This serves multiple systems: elf names, procedural poetry, song lyrics for the music composition system (§21), and general flavor text. Creating our own language avoids IP concerns (Tolkien's languages are protected; Dwarf Fortress's word lists are creative works) and — critically — allows us to design the phoneme inventory to match the audio rendering pipeline.

### Phoneme Inventory and Syllable Structure

The language uses a **CV-dominant syllable structure** (consonant-vowel pairs), similar to Japanese. This is a deliberate design choice: a small, fixed set of syllables maps directly to the vocal sample library used in audio rendering (§21, Phase 2).

**Vowels (~5):** a, e, i, o, u — open, clear sounds that carry well in singing.

**Consonants (~8-10):** Selected for a pleasant, "elven" sound:
- Liquids: l, r (flowing, melodic)
- Nasals: m, n (warm, resonant)
- Soft fricatives: s, sh, h (airy, gentle)
- Soft stops: t, k (occasional crispness for variety)
- Possibly: y, w (semivowels for glide sounds)

Harsh sounds (hard g, harsh ch, z, voiced stops like b/d) are absent or rare — the language should sound flowing and musical.

**Syllable types:**
- CV (consonant-vowel): the dominant pattern — "ta," "ri," "mo," "se"
- V (standalone vowel): "a," "i," "o"
- CVn (consonant-vowel-nasal): "tan," "rin" — allowed word-finally for variety

This gives roughly 45-55 possible syllables. That is the complete recording set per voice type for the audio system.

**Word formation:** Words are 2-4 syllables. Stress falls on the penultimate syllable (like Japanese and Italian — natural for singing). Name generation uses the same syllable inventory and stress rules, ensuring names are consistent with the language.

### Dictionary

A curated dictionary of 500-1000 words, each with:
- **Phonetic form** (syllable sequence)
- **Part of speech** (noun, verb, adjective, particle, etc.)
- **Syllable count and stress pattern**
- **Semantic tags** (nature, war, love, craft, sorrow, beauty, light, darkness, etc.)
- **Phoneme sequence** (for rhyme detection — two words rhyme if their final syllable phonemes match)

The dictionary is a data file loaded at startup, not hardcoded. It can be extended or modded.

### Grammar

Simple rules — the player never needs to parse Elvish, so the grammar just needs to produce consistently structured output that *sounds* like it has rules:

- **Word order:** SOV (subject-object-verb), like Japanese and Latin — common in constructed elven languages and natural for poetry (the verb at the end gives a sense of completion).
- **Agglutinative suffixes** for tense, mood, and case (a few syllable-length suffixes appended to stems). These add syllables predictably, which the meter system can account for.
- **Particles** for questions, emphasis, and poetic flourish.
- **No articles** (streamlines generation, sounds more poetic).

### Procedural Poetry via Simulated Annealing

Poems are composed using the same **simulated annealing** approach as music (§21). A poem is a sequence of words from the dictionary, structured into lines and stanzas.

**State representation.** A poem is a sequence of word slots organized into lines. Each slot holds a word from the dictionary (constrained by part of speech to maintain grammar).

**Scoring function:**
- **Meter adherence** — does the stress pattern match the target meter? (e.g., alternating stressed/unstressed syllables)
- **Rhyme scheme adherence** — do the right line endings rhyme, based on final-syllable phoneme matching?
- **Alliteration and assonance** — bonus for pleasing sound repetitions within lines
- **Semantic coherence** — do the semantic tags of words in a stanza cluster around related themes rather than being random? A stanza about nature shouldn't contain war words unless the poem is about the contrast.
- **Elf personality fit** — a melancholy elf's scoring function weights sorrow/loss tags higher; an ambitious elf prefers glory/achievement; a nature-loving elf favors trees/rivers/stars
- **Grammar correctness** — word sequence follows the grammar rules (SOV order, proper suffix usage)

**Mutations:** Swap a word for another with the same part of speech and similar syllable count. Rearrange clause order within a line. Replace a line-ending word with a different word that rhymes with its target.

**Player-facing output.** The Elvish text is displayed alongside an approximate translation. The translation can be template-generated from the semantic tags: "A lament about autumn leaves and lost friendship, in three stanzas." Or a looser word-by-word gloss for players who want to follow along. The Elvish version sounds beautiful; the translation conveys the gist.

**Skill mapping.** Same as music: novice poets get fewer SA iterations and a smaller vocabulary (fewer unlocked words). Master poets get more iterations and access to richer, rarer words. Quality emerges naturally from the iteration budget.

**Poems are artifacts.** A completed poem is stored as data — the word sequence, meter, rhyme scheme, semantic tags, quality score, author identity. It can be performed at poetry readings (mood boost scaled by quality and audience taste). It can be inscribed on structures (decoration with mood effects based on subject matter). It can be destroyed (emotional consequence for the author, varying by personality — see §18). An elf's masterwork poem persists in the world.

### Composition Threading

Poetry composition uses the same async threading model as music composition (§21). The SA runs in a separate thread, seeded from the main PRNG, with a fixed iteration count. Deterministic from seed. Same save/load behavior — incomplete compositions are recomputed from their seed on load.

---

## 21. Music and Performance

Music is central to elven culture and has both social and mechanical significance. Construction, performance, and daily life all involve singing. The system includes procedural composition via optimization and a phased approach to audio rendering.

### Singing in Daily Life

Elves sing as part of construction (see §11), but also in their free time. They organize themselves into informal singing groups based on friendship, proximity, and vocal compatibility. These groups can become a source of deep friendships — or rivalries, if two singers compete for the same role or audience.

### Ensemble Construction Singing

Construction singing (§11) uses a choir model rather than individual tasks. Group singing is more efficient than solo singing — a well-harmonized choir constructs faster and produces higher-quality results than the same number of elves singing independently. Harmony quality depends on the choir members' relationships, vocal compatibility, skill levels, and mood. A choir of close friends who have sung together many times achieves better harmony than strangers thrown together for the first time.

This ties construction directly into the social simulation: investing in elf friendships and musical training pays off in construction efficiency, not just morale.

### Vocal Variety

Elves have different voice types (soprano, alto, tenor, baritone, bass). Some tasks might be better suited to certain voice types — deep voices for heavy construction singing, high voices for delicate enchantment work. This is speculative but could add flavor to task assignment.

### Procedural Music Composition

Music is composed via **simulated annealing** (SA). The target aesthetic is Palestrina-style polyphony — rule-driven counterpoint that sounds beautiful precisely because it follows strict constraints. Palestrina's style is one of the most formalized in Western music, which makes it an excellent optimization target.

**Representation.** An N-voice piece over T time slots. Each slot for each voice contains a pitch (from a diatonic scale, within the voice's range) or a rest. The state is an N×T matrix.

**Scoring function.** Encodes counterpoint rules directly:

*Hard constraints (heavy penalties):*
- No parallel fifths or octaves between any two voices
- Dissonances must resolve stepwise
- Voices must stay within their type's range
- No voice crossing (soprano stays above alto, etc.)

*Soft constraints (rewards):*
- Prefer consonant intervals between simultaneous voices (thirds, sixths, octaves, perfect fifths)
- Prefer stepwise motion within each voice (small intervals)
- Penalize large leaps unless followed by contrary stepwise motion
- Reward contrary motion between voices
- Reward proper cadential motion in the final time slots — the "satisfying final chord" at project completion is a heavy reward for resolving to a tonic triad through standard cadential voice leading, with optional suspension-resolution for extra drama

**Mutations.** Change one note in one voice by a step or two. Swap two adjacent notes in a voice. Shift a passage up or down. Small, local changes that SA explores efficiently.

**Scaling effort to project importance:**
- Individual elf doing simple work: no composition, hum a pre-composed work song (template)
- Small choir building a routine platform: short piece, 2-3 voices, fewer SA iterations
- Large choir building an important structure: full 4-6 voice polyphony, more time slots, more SA iterations for higher quality
- The final chord resolving at project completion is enforced by the scoring function rewarding consonant cadences in the final slots

**Skill mapping.** Elf composing skill maps to SA iteration budget. A novice composer gets fewer iterations and settles on a worse local minimum — producing simpler, less polished music. A master composer gets more iterations and converges on something genuinely beautiful. Same algorithm, different budget, and quality differences emerge naturally.

### Composition as a Job Step

For any job that requires new music, a **composition step** precedes the actual singing. During this time, the composing elf (or elves) mill about and think while the sim runs the SA procedure.

**Threading model.** Composition does not need to match real-time ticks — it just needs to finish before the composition time expires. The SA runs in a **separate thread**, asynchronously from the main sim loop. The main sim only stalls if composition has not finished by the time the "composition complete" event fires.

**Determinism.** The SA uses a PRNG seeded from the main sim's PRNG at the moment the composition job begins. The **iteration count is fixed** (determined by project complexity and elf skill), not wall-clock dependent. This ensures all clients produce identical compositions from the same seed, regardless of machine speed. Faster machines simply finish earlier and wait.

**Save/load.** On save: complete any in-progress compositions first (save everything else immediately in case of interruption, then wait for compositions and save those). On load: any compositions that weren't completed (interrupted save) are recomputed from their seed before gameplay resumes. Since the SA is deterministic from its seed and iteration count, recomputation produces identical results.

**Compositions are artifacts.** A completed composition is saved as data — the note matrix, a quality score, the composer's identity. It exists in the world. It can be performed again at gatherings (mood boost). The composition step only happens once per piece; subsequent performances of the same piece reuse the stored score.

### Audio Rendering Phases

The composition system produces a score (notes, voices, time slots). Turning that into audible music is a separate rendering problem with three phases of increasing fidelity:

**Phase 1: Waveform synthesis.** Each voice is a simple sine or triangle wave at the right pitch. Sounds electronic, but harmonies are clearly audible. Trivially implementable. Primarily useful as a debugging and validation tool — lets you hear whether the SA is producing musically reasonable output.

**Phase 2: Sampled vocal syllables.** Record short vocal samples ("ah," "oh," Elvish syllables from the conlang — see §20) at 2-3 reference pitches per voice type (low, mid, high in the range), then pitch-shift to match the target note. For a CV-structured conlang with ~45 syllables per voice type × 3 reference pitches = ~135 short recordings per voice type. An afternoon of recording, or synthesized via TTS tools.

The composition system's output maps directly: each time slot has a pitch and a syllable (from the song text or the poem being performed). Look up the right sample, pitch-shift it, layer all voices. This already sounds remarkably like a real choir, because that's essentially what a choir *is* — individual voices each singing syllables at specified pitches. Phase 2 gets roughly 80% of the way to impressive.

**Phase 3: Continuous vocal synthesis.** Replace the discrete sample lookup with real-time vocal synthesis — formant shaping, smooth vowel transitions, vibrato, dynamics. This is where singing starts to sound truly alive rather than like a sample player. Open-source vocal synthesis engines (World, Sinsy, or similar) could be adapted. Very far future, but the architecture from Phase 2 (composition produces a score with syllable assignments) provides the same input — only the audio backend changes.

### Noise and Proximity

Musical performances and sleeping elves both have preferences about their acoustic environment:
- Sleeping elves dislike the noise of nearby construction singing.
- Performers dislike being near active construction — the competing sounds are distracting and aesthetically unpleasant.
- These proximity effects create natural zoning pressure: quiet residential areas, noisy construction zones, and performance spaces that want separation from both.

### Magical Instruments (Future)

Enchanted harps, flutes, or drums that augment construction singing, boost morale in an area, or provide other magical effects. Instruments could be crafted and have their own quality levels.

---

## 22. Magic Items

Weapons, tools, and instruments can be imbued with magical properties. Magic items are not generic stat boosts — they have personalities.

### Item Personalities

A magic item develops (or is imbued with) a personality over time. The wielder must build a relationship with the item to unlock its full power, similar to how elves build social relationships with each other.

- **Bloodthirsty weapons** require frequent combat to unlock power. An elf carrying a bloodthirsty sword who spends months peacefully tending gardens will find the weapon growing cold and unresponsive.
- **Peace-loving items** lose power when used for war. A healing staff taken into battle might resist its wielder.
- **Curious items** grow stronger when taken to new places or used in novel ways.

This system means that matching items to appropriate wielders matters — a warrior gets more from a bloodthirsty blade, while a poet might bond better with a contemplative enchanted quill.

### Crafting and Enchantment

Magic item creation requires specialized facilities (enchanting studios) and skilled enchanters. The quality and personality of an item depends on the crafter's skill, mood, and the materials used. Design TBD for the full crafting system.

---

# Part V: Presentation

*How the player sees and interacts with the world: camera, sprites, and the visual identity.*

## 23. Camera System

### Orbital Camera

A `Camera3D` node orbiting a focal point (pivot).

- **WASD** — moves the focal point horizontally, relative to the camera's facing direction. The camera moves with it; the viewing angle does not change.
- **Q/E or Left/Right arrows** — free horizontal rotation around the focal point (not snapped to 90° increments).
- **Up/Down arrows** — tilt the camera (change pitch). Clamped to 10°–80° from horizontal (i.e., 10° short of flat and 10° short of straight down).
- **Middle-mouse drag** — rotate (horizontal) and tilt (vertical) simultaneously.
- **Scroll wheel** — zoom (distance from focal point to camera).
- **R/F or Page Up / Page Down** — move the focal point up/down to navigate vertical levels, clamped to world bounds.

### Follow Mode

The elf info panel (see Elf Info on Click, below) includes a "Follow" button. Clicking it locks the camera's focal point to the selected elf — the camera tracks the elf's position as they move, while rotation and zoom remain under player control. Any WASD input breaks the follow and returns to normal camera control. This is a lightweight way for the player to observe an individual elf's daily life without adding RPG mechanics. It should be straightforward to implement in the near term.

### Elf Info on Click

Clicking an elf opens an info panel showing their name, personality traits, current emotional state, active task, relationships, and background. This is the primary way the player gets to know individual elves and diagnose problems. The info panel also provides the Follow Mode button (above).

### Sprite Billboarding

Because horizontal rotation is free, sprites use 8-directional billboarding. Sprite frame is selected based on elf facing direction relative to current camera angle. Five unique drawn angles (front, front-side, side, back-side, back) are mirrored horizontally for the remaining three.

---

## 24. Sprite Art Pipeline

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

# Part VI: Development

*Testing strategy, development roadmap, and unresolved design questions.*

## 25. Testing Infrastructure

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

## 26. Iterative Development Roadmap

Development follows iterative deepening — all areas developed concurrently at increasing fidelity.

### Phase 0: Foundations (Weeks 1–2)

Status: Done

- Complete Godot "Your first 3D game" tutorial.
- Build the orbital camera controller with placeholder cubes.
- Get gdext compiling: minimal Rust struct exposed to Godot.
- Set up the two-crate structure (`sim` + `gdext`).
- Define core types: `VoxelCoord`, `TreeId`/`ElfId` (UUID v4), `SimCommand`, `GameConfig`.
- Add `serde` derives to everything from the start.

### Phase 1: A Tree and an Elf (Weeks 2–4)

Status: Done

- Procedural generation of one tree (trunk + branches) in Rust, rendered as simple geometry in Godot.
- Nav graph for the tree surface.
- One elf as a billboard placeholder sprite, pathfinding and moving around the tree.
- Event-driven tick loop with the priority queue model.
- Basic SimCommand pipeline (even if the only command is "spawn elf at location").

### Phase 2: Construction and Persistence (Weeks 4–8)

Status: Work in Progress. Added task logic, but not blueprinting, construction, etc.

- Blueprint mode: layer-based selection, ghost previews.
- Platform designation (round preferred, rectangular allowed).
- Visual smoothing on platforms (not cubes).
- Choir-based construction singing: elves assemble into choirs, sing together, tree grows voxel by voxel.
- Mana as a resource (tree stores it, construction spends it, elves generate it at a flat rate initially).
- Conservation of mass: tree tracks stored wood material, construction consumes it.
- Nav graph updates when construction completes.
- Multiple elves, task queue with priorities, auto-assignment.
- Save/load: serialize full sim state to JSON. Load restores the world exactly. Save versioning with migration functions for schema changes.
- Camera follow mode: lock focal point to a selected elf.

### Phase 3: Vertical Village (Weeks 8–12)

- Bridges/walkways connecting different parts of the tree.
- Stairs/ramps connecting vertical levels.
- Ladders (rope or wood) as a cheaper vertical connector.
- Carve holes: remove material from existing structures (doorways, storage hollows).
- Grow branches/boughs: extend the tree for more photosynthesis and fruit capacity.
- Basic structural integrity checks (simplified, pre-FEM — at minimum, connectivity flood fill so unsupported structures detach).
- Basic elf needs: hunger (eat fruit), rest (find a sleeping spot). Elves self-direct to satisfy needs.
- Buildings vs. furnishing: generic building geometry, then furnish for purpose.
- Batch blueprinting with dependency ordering and structural warnings.

### Phase 4: Emotional Depth (Timing Flexible)

- Personality axes affecting behavior.
- Multi-dimensional emotional state (joy, fulfillment, sorrow, stress, pain, fear, anxiety).
- Mood system with escalating consequences.
- Social graph, relationships, contagion.
- Events and narrative log.
- Hedonic adaptation (asymmetric: upward adaptation faster than downward).
- Apprenticeships and skill transfer via proximity.
- Poetry readings, social gatherings.
- Mana generation tied to mood.
- Seasonal visual changes and gameplay effects (leaf drop, snow hazards, clothing needs).

### Phase 5: Structural Integrity and Fire

- Voxel-based finite element modeling for structural integrity (§9).
- Cascading structural failure: overloaded voxels fail, load redistributes, disconnected chunks fall.
- Stress heat map in blueprint mode for construction planning.
- Fire simulation Stage 1: basic probabilistic spread, voxel destruction.
- Fire simulation Stage 2: heat accumulation, ignition thresholds, green vs. dry wood.
- Structural consequences of fire: burning supports trigger collapse cascades.

### Phase 6: Culture and Language

- Constructed Elvish language (§20): phoneme inventory, dictionary, grammar.
- Procedural poetry via simulated annealing.
- Procedural music composition via simulated annealing (Palestrina-style counterpoint).
- Audio rendering Phase 1: waveform synthesis for debugging and validation.
- Elf name generation from the conlang's syllable rules.
- Ensemble construction singing with harmony mechanics.

### Phase 7: Expansion and Ecology

- Multiple trees in the world (NPC trees with personalities and preferences).
- Root network expansion mechanic (grow roots toward another tree, diplomacy phase).
- Per-tree carrying capacity.
- Inter-tree cultural drift as elves on different trees develop distinct traditions.
- Tree memory system: the player's tree surfaces ancient knowledge and warnings.
- Fire simulation Stages 3–4: environmental factors, firefighting, fire as ecological force.
- Fruit variety, food storage, cooking, and magical brewing.
- Logistics system for spatial resource flow.

### Phase 8+: Distant Future

- Combat, invaders, and fog of war.
- Elf weapons (bows, spears, clubs) and defensive structures (ballista turrets, magic wards).
- Military organization and squad management.
- Soul mechanics: death, soul passage into trees, resurrection, soul-powered constructs.
- Magic items with personalities.
- Multiple tree species with different properties.
- Crafting and non-construction jobs.
- Audio rendering Phase 2: sampled vocal syllables from the conlang.
- Final AI-generated art replacing placeholders.
- Multiplayer networking.
- Large creature pathfinding (2×2 footprint nav grid).
- Flying entity navigation.
- **Military campaigns** — sending elves on expeditions in the wider world, with direct tactical control (unlike Dwarf Fortress's hands-off approach).
- **Adventure mode** — control an individual elf in an RPG-like mode, exploring the world from a first/third-person perspective within the same simulation.
- Audio rendering Phase 3: continuous vocal synthesis.

---

## 27. Open Questions

- **Z-level visibility in the UI.** How does the player see lower platforms when upper ones occlude them? Transparency, cutaway, hide-upper-levels toggle? Deferred until multi-level construction exists.
- **FEM implementation method.** Structural integrity has its design direction (§9), but the choice between direct sparse solve and iterative relaxation is still open, as is fixed-point vs. floating-point arithmetic.
- **Day/night cycle.** Length of an in-game day in real time. Affects gameplay pacing, fruit production rates, elf sleep schedules. Tune later.
- **Weather beyond seasons.** Seasons now have gameplay mechanics (§8), but weather within seasons (rain, wind, storms) is undefined. Could tie into mood, fire spread, and construction difficulty.
- **Fire × structural integrity performance.** Fire has a staged plan (§16) and structural integrity has its own system (§9), but the interaction — fire destroying load-bearing voxels triggering FEM recalculation during an already-expensive fire tick — needs careful design to avoid cascading performance issues.
- **Modding.** The data-driven config and JSON format are a foundation, but full modding (custom structures, elf behaviors, invader types) would need a scripting layer or plugin system.
- **Wood golem naming.** "Golem" is generic. Something more elven/arboreal would fit better. Open for the conlang (§20) to solve once the dictionary is developed.
- **Fruit selection UI.** How does the player direct which fruits a tree produces? Several approaches outlined in §13, but none chosen.
