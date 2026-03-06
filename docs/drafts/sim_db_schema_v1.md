# SimDb Schema Design (v1)

> **Status:** Draft — desired end-goal schema for migrating `elven_canopy_sim`
> to tabulosity. Transition strategy is deliberately left vague; this document
> focuses on what the final schema should look like.

## Design Principles

1. **Normalize storage, not logic.** Use tables and FKs for entity
   relationships and cross-entity queries. Keep Rust enums for dispatch and
   behavioral logic within rows.
2. **Relationship tables over variant columns.** When task variants reference
   structures or positions, use relationship tables with a `role` enum rather
   than nullable variant-specific columns.
3. **Unified inventory.** All item storage (creature, building, ground pile)
   goes through a single `Inventories` + `ItemStacks` table pair.
4. **Per-variant extension tables for non-FK state.** Small value data that
   doesn't benefit from relational modeling (e.g., Haul phase, Sleep location
   enum) lives in lightweight extension tables, not in the base task row.
5. **Thoughts and paths stay inline.** Short, per-creature lists that are never
   queried across creatures remain as opaque fields on the creature row.

---

## Table Overview

```
SimDb
├── creatures          Creature (elf, capybara, etc.)
├── tasks              Task (base: shared fields)
├── task_structure_refs TaskStructureRef (task → structure, with role)
├── task_voxel_refs    TaskVoxelRef (task → voxel position, with role)
├── task_haul_data     HaulData (Haul-specific mutable state)
├── task_sleep_data    SleepData (Sleep-specific state)
├── task_acquire_data  AcquireData (AcquireItem-specific state)
├── blueprints         Blueprint (build projects)
├── structures         CompletedStructure (completed buildings)
├── inventories        Inventory (abstract container)
├── item_stacks        ItemStack (items in an inventory)
├── logistics_wants    LogisticsWant (desired items for a building or creature)
├── ground_piles       GroundPile (ground location → inventory)
└── furniture          Furniture (placed furniture positions per structure)
```

---

## Creatures

```
Creature
    id: CreatureId               PK
    species: Species             indexed
    position: VoxelCoord         (3 fields: x, y, z)
    name: String
    name_meaning: String
    current_node: Option<NavNodeId>
    current_task: Option<TaskId> indexed, FK → tasks
    food: i64
    rest: i64
    assigned_home: Option<StructureId>  indexed, FK → structures, unique
    inventory_id: InventoryId    FK → inventories
    -- rendering metadata (non-indexed payload) --
    move_from: Option<VoxelCoord>
    move_to: Option<VoxelCoord>
    move_start_tick: u64
    move_end_tick: u64
    -- inline complex data (not normalized) --
    thoughts: Vec<Thought>       opaque, per-creature only
    path: Option<CreaturePath>   opaque, transient nav data
```

**Key indexes:**
- `species` — renderer queries, species-filtered task assignment
- `current_task` — find idle creatures (IS NULL filter), find creature by task
- `assigned_home` — unique constraint, reverse home lookup
- Compound `(species, current_task)` — "find idle elves" hot path

**Notes:**
- `thoughts` stays inline. Small capped list, only accessed per-creature for
  mood scoring. The `StructureId` payloads inside `ThoughtKind` are
  informational/historical, not active FKs needing cascade.
- `path` stays inline. Transient nav data, never cross-queried.
- `wants` moves to the `logistics_wants` table (shared with structures).
- `inventory` moves to `inventories` + `item_stacks`.
- `assignees` on Task goes away. Task assignment is tracked via
  `Creature.current_task` FK. "Who's assigned to task X?" is
  `creatures.by_current_task(&task_id)`.

---

## Tasks (Base Table)

```
Task
    id: TaskId                   PK
    kind_tag: TaskKindTag        indexed (enum discriminant: GoTo, Build, ...)
    state: TaskState             indexed (Available, InProgress, Complete)
    location: NavNodeId
    progress: f32
    total_cost: f32
    required_species: Option<Species>  indexed
    origin: TaskOrigin
```

**Key indexes:**
- `state` — most queries filter on state
- `kind_tag` — many queries filter on task kind
- Compound `(state, kind_tag)` — "all active Haul tasks", "all active Cook
  tasks", etc.
- Compound `(state, required_species)` — `find_available_task` hot path
- Filtered `state != Complete` — most task queries exclude completed tasks

**Notes:**
- `assignees: Vec<CreatureId>` is removed. The relationship is tracked on the
  creature side via `Creature.current_task`. In practice, tasks have 0-1
  assignees. If multi-worker tasks are needed later, the creature FK already
  supports multiple creatures pointing to the same task.
- `kind` as an enum is replaced by `kind_tag` (discriminant only) plus
  relationship tables and extension tables for variant-specific data.
- `location` is mutable (Haul changes it on phase flip). This is fine as a
  non-indexed payload field.
- Completed tasks are never GC'd currently. A filtered index on
  `state != Complete` is essential to avoid scanning dead tasks.

---

## Task Relationship Tables

These replace the FK-carrying fields inside `TaskKind` variants.

### TaskStructureRef

```
TaskStructureRef
    id: auto-PK (or composite TaskId+role)
    task_id: TaskId              FK → tasks, indexed
    structure_id: StructureId    FK → structures, indexed
    role: TaskStructureRole
```

**`TaskStructureRole` enum:**
- `BuildTarget` — Build task → Blueprint's structure (via project)
- `FurnishTarget` — Furnish task → structure being furnished
- `CookAt` — Cook task → kitchen structure
- `HaulDestination` — Haul task → destination structure
- `HaulSourceBuilding` — Haul task → source building (when source is a building)
- `SleepAt` — Sleep task → dormitory or home structure
- `AcquireSourceBuilding` — AcquireItem → source building

**Key queries enabled:**
- "When structure X is destroyed, find all affected tasks" →
  `task_structure_refs.by_structure_id(&x)`
- "Active cook task for kitchen X" →
  `task_structure_refs.by_structure_id(&x)` filtered by role=CookAt,
  then check task state
- "Haul tasks targeting structure X" →
  `task_structure_refs.by_structure_id(&x)` filtered by role=HaulDestination

**Compound index:** `(structure_id, role)` — efficient lookup by structure +
relationship type.

### TaskVoxelRef

```
TaskVoxelRef
    id: auto-PK (or composite TaskId+role)
    task_id: TaskId              FK → tasks, indexed
    coord: VoxelCoord            indexed
    role: TaskVoxelRole
```

**`TaskVoxelRole` enum:**
- `FruitTarget` — EatFruit or Harvest task → fruit voxel position
- `BedPosition` — Sleep task → bed voxel (if sleeping in a bed)
- `HaulSourcePile` — Haul task → ground pile position (when source is a pile)
- `AcquireSourcePile` — AcquireItem → ground pile position

**Key queries enabled:**
- "Claimed fruit positions" → `task_voxel_refs.by_role(&FruitTarget)` (with
  task state != Complete filter)
- "Occupied beds" → `task_voxel_refs.by_role(&BedPosition)` (with task
  state != Complete filter)

---

## Task Extension Tables

For non-FK mutable state specific to a task variant. These are small — most
task kinds need nothing here (GoTo, EatBread, Mope, Furnish, Harvest have
no extra mutable state beyond what's in the base task + relationship tables).

### HaulData

```
HaulData
    task_id: TaskId              PK, FK → tasks
    item_kind: ItemKind
    quantity: u32                mutable (updated on pickup)
    phase: HaulPhase             mutable (GoingToSource → GoingToDestination)
    destination_nav_node: NavNodeId
```

**Notes:**
- Haul is the only task kind that mutates its variant data mid-lifecycle.
- `phase` change also triggers `Task.location` update on the base table.
- `quantity` is updated on pickup (may be less than originally planned if
  source was partially depleted).

### SleepData

```
SleepData
    task_id: TaskId              PK, FK → tasks
    sleep_location: SleepLocationType  (Home, Dormitory, Ground)
```

**Notes:**
- The structure FK for Home/Dormitory is in `TaskStructureRef` with role
  `SleepAt`. The `SleepLocationType` here is just the discriminant for
  thought generation on completion.
- `bed_pos` is in `TaskVoxelRef` with role `BedPosition`.

### AcquireData

```
AcquireData
    task_id: TaskId              PK, FK → tasks
    item_kind: ItemKind
    quantity: u32
```

---

## Task Kind Coverage

How each `TaskKind` variant maps to the new schema:

| Kind | Base task | Structure refs | Voxel refs | Extension |
|---|---|---|---|---|
| GoTo | kind_tag only | — | — | — |
| Build | kind_tag | BuildTarget (→ structure via blueprint) | — | — |
| EatBread | kind_tag only | — | — | — |
| EatFruit | kind_tag | — | FruitTarget | — |
| Sleep | kind_tag | SleepAt (if bed) | BedPosition (if bed) | SleepData |
| Haul | kind_tag | HaulDestination, HaulSourceBuilding? | HaulSourcePile? | HaulData |
| Cook | kind_tag | CookAt | — | — |
| Harvest | kind_tag | — | FruitTarget | — |
| AcquireItem | kind_tag | AcquireSourceBuilding? | AcquireSourcePile? | AcquireData |
| Furnish | kind_tag | FurnishTarget | — | — |
| Mope | kind_tag only | — | — | — |

**Note on Build:** The Build task currently stores `project_id: ProjectId`
(FK to Blueprint, not directly to Structure). The structure doesn't exist until
the build completes. We have two options:
1. A `TaskBlueprintRef` table (task → blueprint), or
2. Store `project_id` directly on the base Task as an optional FK field, since
   it's the only task kind that references a blueprint.

Option 2 is simpler given it's a single case. A nullable `project_id` on the
base Task, with FK to blueprints, avoids a whole relationship table for one
variant.

---

## Blueprints

```
Blueprint
    id: ProjectId                PK
    build_type: BuildType
    priority: Priority
    state: BlueprintState        indexed
    task_id: Option<TaskId>      FK → tasks
    stress_warning: bool
    -- inline complex data --
    voxels: Vec<VoxelCoord>             opaque
    face_layout: Option<Vec<(VoxelCoord, FaceData)>>  opaque
    original_voxels: Vec<(VoxelCoord, VoxelType)>     opaque
```

**Notes:**
- `voxels`, `face_layout`, `original_voxels` are spatial data used only during
  construction. No cross-entity queries on them. Keep as opaque blob fields.
- Index on `state` for finding in-progress blueprints.

---

## Structures

```
CompletedStructure
    id: StructureId              PK
    project_id: ProjectId        FK → blueprints, indexed
    build_type: BuildType
    anchor: VoxelCoord
    width: i32
    depth: i32
    height: i32
    completed_tick: u64
    name: Option<String>
    furnishing: Option<FurnishingType>  indexed
    assigned_elf: Option<CreatureId>    FK → creatures, unique
    inventory_id: InventoryId    FK → inventories
    logistics_priority: Option<u8>      indexed
    cooking_enabled: bool
    cooking_bread_target: u32
```

**Key indexes:**
- `furnishing` — find kitchens, dormitories, homes (currently linear scans)
- `project_id` — reverse lookup from blueprint to structure (currently linear)
- `logistics_priority` filtered (IS NOT NULL) — logistics heartbeat
- `assigned_elf` — unique constraint (one elf per home)

**Removed fields:**
- `inventory: Vec<Item>` → replaced by `inventory_id` FK to `inventories`
- `logistics_wants: Vec<LogisticsWant>` → moves to `logistics_wants` table
- `furniture_positions` / `planned_furniture` → moves to `furniture` table

---

## Furniture

```
Furniture
    id: auto-PK
    structure_id: StructureId    FK → structures, indexed
    coord: VoxelCoord
    placed: bool                 false = planned, true = placed
```

**Notes:**
- Replaces `furniture_positions` (placed=true) and `planned_furniture`
  (placed=false) on `CompletedStructure`.
- "How many beds does this dormitory have?" →
  `furniture.by_structure_id(&sid)` filtered by placed=true, count.

---

## Inventories and Item Stacks

### Inventories

```
Inventory
    id: InventoryId              PK (auto-generated UUID)
```

A minimal indirection table. Each creature, structure, and ground pile has
an `inventory_id` FK pointing here. This enables a single `ItemStacks` table
to hold all items regardless of container type.

### ItemStacks

```
ItemStack
    id: ItemStackId              PK (auto-generated UUID)
    inventory_id: InventoryId    FK → inventories, indexed
    kind: ItemKind               indexed
    quantity: u32
    owner: Option<CreatureId>    FK → creatures
    reserved_by: Option<TaskId>  FK → tasks, indexed
```

**Key indexes:**
- `inventory_id` — "all items in this container"
- `kind` — "all bread stacks anywhere"
- `reserved_by` — find items reserved by a task (for cleanup on abandonment)
- Compound `(inventory_id, kind)` — "bread in this kitchen"
- Filtered `reserved_by IS NULL` — find unreserved items

**Key queries enabled:**
- "Find unreserved fruit anywhere" → scan `item_stacks` with kind=Fruit,
  reserved_by=NULL (or compound index)
- "Total bread in the world" → `item_stacks.by_kind(&Bread)` sum quantities
- "Clear reservations for task X" → `item_stacks.by_reserved_by(&task_id)`
- "Count owned bread for creature X" → filter by owner + kind on creature's
  inventory

**Stacking:** `add_item` merges stacks with matching `(kind, owner,
reserved_by)` within the same inventory. Stack splitting on partial reservation
creates a new `ItemStack` row.

---

## Ground Piles

```
GroundPile
    id: GroundPileId             PK (auto-generated UUID or position-based)
    position: VoxelCoord         indexed, unique
    inventory_id: InventoryId    FK → inventories
```

**Notes:**
- Position is unique — one pile per voxel.
- Empty piles are removed (and their inventory cleaned up).
- `HaulSourcePile` and `AcquireSourcePile` in `TaskVoxelRef` reference the
  position, not the pile ID. This matches the current pattern and avoids FK
  issues when piles are created/destroyed frequently.

---

## Logistics Wants

```
LogisticsWant
    id: auto-PK
    -- polymorphic owner --
    structure_id: Option<StructureId>   FK → structures
    creature_id: Option<CreatureId>     FK → creatures
    -- want data --
    item_kind: ItemKind
    target_quantity: u32
```

**Constraint:** Exactly one of `structure_id` or `creature_id` must be set
(application-level, not enforced by tabulosity).

**Indexes:**
- `structure_id` — "wants for this building"
- `creature_id` — "wants for this creature"

**Alternative:** Two separate tables (`StructureWant`, `CreatureWant`) instead
of one polymorphic table. Simpler FK constraints but duplicates schema. Given
the type is identical and both are small lists, polymorphic seems acceptable.

---

## SimDb Definition Sketch

```rust
#[derive(Database)]
struct SimDb {
    #[table(singular = "creature")]
    creatures: CreatureTable,

    #[table(singular = "task")]
    tasks: TaskTable,

    #[table(singular = "task_structure_ref",
            fks(task_id = "tasks", structure_id = "structures"))]
    task_structure_refs: TaskStructureRefTable,

    #[table(singular = "task_voxel_ref",
            fks(task_id = "tasks"))]
    task_voxel_refs: TaskVoxelRefTable,

    #[table(singular = "haul_data",
            fks(task_id = "tasks"))]
    haul_data: HaulDataTable,

    #[table(singular = "sleep_data",
            fks(task_id = "tasks"))]
    sleep_data: SleepDataTable,

    #[table(singular = "acquire_data",
            fks(task_id = "tasks"))]
    acquire_data: AcquireDataTable,

    #[table(singular = "blueprint")]
    blueprints: BlueprintTable,

    #[table(singular = "structure",
            fks(project_id = "blueprints",
                assigned_elf? = "creatures",
                inventory_id = "inventories"))]
    structures: CompletedStructureTable,

    #[table(singular = "inventory")]
    inventories: InventoryTable,

    #[table(singular = "item_stack",
            fks(inventory_id = "inventories",
                owner? = "creatures",
                reserved_by? = "tasks"))]
    item_stacks: ItemStackTable,

    #[table(singular = "ground_pile",
            fks(inventory_id = "inventories"))]
    ground_piles: GroundPileTable,

    #[table(singular = "logistics_want",
            fks(structure_id? = "structures",
                creature_id? = "creatures"))]
    logistics_wants: LogisticsWantTable,

    #[table(singular = "furniture",
            fks(structure_id = "structures"))]
    furniture: FurnitureTable,
}
```

**Notable FK omissions:**
- `Creature.current_task` → tasks: FK declared on creatures table
- `Creature.assigned_home` → structures: FK declared on creatures table
- `Blueprint.task_id` → tasks: FK declared on blueprints table
- `Task.project_id` → blueprints: optional FK on base task (Build only)

---

## What Stays on SimState (Not in SimDb)

| Field | Reason |
|---|---|
| `tick` | Scalar metadata |
| `rng` | PRNG state |
| `config` | Immutable config |
| `event_queue` | Priority queue, not relational |
| `trees` | Single tree, not worth a table |
| `player_tree_id`, `player_id` | Scalar metadata |
| `next_structure_id` | Counter |
| `world` | Dense spatial grid (transient) |
| `nav_graph`, `large_nav_graph` | Graph data (transient) |
| `placed_voxels`, `carved_voxels` | Append-only spatial lists |
| `face_data` / `face_data_list` | Per-voxel spatial data |
| `ladder_orientations` / list | Per-voxel spatial data |
| `species_table` | Config-derived lookup (transient) |
| `lexicon` | Language data (transient) |
| `last_build_message` | Transient UI state |
| `structure_voxels` | Transient reverse index (could become a tabulosity index on a structure_voxels child table, but low priority) |

---

## Completed Task Retention

Currently, completed tasks are never removed. With this schema, completed tasks
and their relationship/extension rows accumulate indefinitely. Options:

1. **GC pass:** Periodically remove Complete tasks older than N ticks. Need to
   also clean up their `TaskStructureRef`, `TaskVoxelRef`, and extension rows
   (cascade delete, or application-level cleanup).
2. **Filtered indexes:** Index only `state != Complete` tasks. Completed tasks
   exist but are invisible to active queries. This is the minimal change.
3. **Both:** Filtered indexes for query performance now, GC later when the
   table size becomes a memory concern.

Recommend option 3: filtered indexes immediately, GC deferred.

---

## Open Questions

1. **Build task → Blueprint FK:** Should `project_id` be a nullable column on
   the base Task, or a `TaskBlueprintRef` relationship table? Leaning toward
   column since it's the only task-to-blueprint reference.

2. **Bidirectional home assignment:** `Creature.assigned_home` and
   `Structure.assigned_elf` must stay in sync. With tabulosity, one side is the
   FK source of truth. The other could be:
   - An indexed query (e.g., "find creature where assigned_home = X")
   - Kept as a denormalized field with application-level sync
   Having both as FK columns with a unique constraint on each is cleanest but
   requires maintaining both on every assignment change.

3. **GroundPile identity:** Should piles have a UUID PK, or use position as PK?
   Position-as-PK is natural (one pile per voxel, unique) but `VoxelCoord` is a
   composite type. A UUID PK with a unique index on position works with
   tabulosity's single-PK model.

4. **Inventory indirection cost:** Every item query now requires knowing the
   `inventory_id`, which means looking up the creature/structure/pile first.
   Is this an acceptable cost for the unified model? (Probably yes — you almost
   always know which entity you're querying.)

5. **Task GC and cascade:** When a completed task is garbage-collected, its
   `TaskStructureRef`, `TaskVoxelRef`, and extension rows must also be removed.
   Tabulosity's restrict-on-delete would block this unless cascade delete
   (F-tab-cascade-del) is implemented first. Workaround: application-level
   cleanup that removes children before the task.

---

## Transition Strategy

*Deliberately left vague — to be fleshed out once the end-goal schema is
agreed upon.*

The general approach is **Option B: structural big bang, behavioral
incremental** — move all data into `SimDb` in one mechanical pass (translating
`BTreeMap` access to tabulosity calls), then incrementally add indexes, FK
declarations, and optimized query patterns.
