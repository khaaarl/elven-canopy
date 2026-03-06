# SimDb Schema Design (v3)

> **Status:** Draft — desired end-goal schema for migrating `elven_canopy_sim`
> to tabulosity. Transition strategy is deliberately left vague; this document
> focuses on what the final schema should look like.

## Changelog from v2

- **Resolved open question #1 (Build task → Blueprint FK):** Added a
  `TaskBlueprintRef` relationship table instead of a nullable `project_id`
  column on the base Task. This keeps the base Task free of variant-specific
  columns, consistent with how other task-to-entity relationships are handled
  via relationship tables. Cascade on `task_id`.
- **Updated Task Kind Coverage table:** Build row now references
  `TaskBlueprintRef` instead of a base-task column.
- **Removed Open Questions section:** All questions from v1/v2 are now resolved.

## Changelog from v1

- **Renamed task extension tables:** `SleepData` → `TaskSleepData`,
  `HaulData` → `TaskHaulData`, `AcquireData` → `TaskAcquireData`. All
  references updated throughout.
- **Thoughts promoted to a table:** Removed `thoughts: Vec<Thought>` from
  Creature row. Added a `Thought` table with FK to creatures. Dedup and cap
  enforcement now use table queries + deletes instead of Vec manipulation.
- **LogisticsWant FK simplified:** Replaced polymorphic `structure_id` /
  `creature_id` columns with a single `inventory_id` FK → inventories.
- **Resolved open question #2 (home assignment):** Removed unique constraint
  on `Creature.assigned_home` (multiple elves can share a home). Removed
  `assigned_elf` from structures (redundant — query creatures by
  `assigned_home` instead).
- **Resolved open question #3 (ground pile identity):** UUID PK with unique
  index on position.
- **Resolved open question #4 (inventory indirection cost):** Accepted as-is.
- **Resolved open question #5 (task GC and cascade):** Cascade delete for
  task extension/relationship tables (requires F-tab-cascade-del).

---

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
5. **Thoughts as a table.** Cross-creature thought queries and cap enforcement
   benefit from relational modeling; thoughts are stored in their own table
   with FK to creatures.

---

## Table Overview

```
SimDb
├── creatures            Creature (elf, capybara, etc.)
├── thoughts             Thought (per-creature thought entries)
├── tasks                Task (base: shared fields)
├── task_blueprint_refs  TaskBlueprintRef (task → blueprint, for Build tasks)
├── task_structure_refs  TaskStructureRef (task → structure, with role)
├── task_voxel_refs      TaskVoxelRef (task → voxel position, with role)
├── task_haul_data       TaskHaulData (Haul-specific mutable state)
├── task_sleep_data      TaskSleepData (Sleep-specific state)
├── task_acquire_data    TaskAcquireData (AcquireItem-specific state)
├── blueprints           Blueprint (build projects)
├── structures           CompletedStructure (completed buildings)
├── inventories          Inventory (abstract container)
├── item_stacks          ItemStack (items in an inventory)
├── logistics_wants      LogisticsWant (desired items for an inventory)
├── ground_piles         GroundPile (ground location → inventory)
└── furniture            Furniture (placed furniture positions per structure)
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
    assigned_home: Option<StructureId>  indexed, FK → structures
    inventory_id: InventoryId    FK → inventories
    -- rendering metadata (non-indexed payload) --
    move_from: Option<VoxelCoord>
    move_to: Option<VoxelCoord>
    move_start_tick: u64
    move_end_tick: u64
    -- inline complex data (not normalized) --
    path: Option<CreaturePath>   opaque, transient nav data
```

**Key indexes:**
- `species` — renderer queries, species-filtered task assignment
- `current_task` — find idle creatures (IS NULL filter), find creature by task
- `assigned_home` — reverse home lookup (not unique — multiple elves may share)
- Compound `(species, current_task)` — "find idle elves" hot path

**Notes:**
- `thoughts` moved to a dedicated `Thought` table (see below). Thought dedup
  and cap enforcement now use table queries and deletes rather than Vec
  manipulation. The `StructureId` payloads inside `ThoughtKind` are
  informational/historical, not active FKs needing cascade.
- `path` stays inline. Transient nav data, never cross-queried.
- `wants` moves to the `logistics_wants` table (shared with structures).
- `inventory` moves to `inventories` + `item_stacks`.
- `assignees` on Task goes away. Task assignment is tracked via
  `Creature.current_task` FK. "Who's assigned to task X?" is
  `creatures.by_current_task(&task_id)`.
- `assigned_home` has no unique constraint — multiple elves will share a home
  in the near future. To find residents of a structure, query
  `creatures.by_assigned_home(&structure_id)`.

---

## Thoughts

```
Thought
    id: ThoughtId                PK (auto-generated)
    creature_id: CreatureId      FK → creatures, indexed
    kind: ThoughtKind            opaque enum (not normalized further)
    tick: u64
```

**Key indexes:**
- `creature_id` — "all thoughts for this creature" (mood scoring, dedup, cap)

**Notes:**
- Replaces the `thoughts: Vec<Thought>` field on Creature.
- `ThoughtKind` remains an opaque Rust enum. Variants carry payload data
  (e.g., `StructureId` for home-related thoughts) but these are informational,
  not active FKs.
- **Dedup:** Before inserting, query `thoughts.by_creature_id(&cid)` and check
  for an existing thought with the same `kind`. Replace or skip as appropriate.
- **Cap enforcement:** After inserting, if the creature's thought count exceeds
  the cap, delete the oldest entries (lowest `tick` values).
- Thought expiry (if implemented) can query by `tick` range per creature.

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

### TaskBlueprintRef

```
TaskBlueprintRef
    id: auto-PK (or composite TaskId+role if only one role)
    task_id: TaskId              FK → tasks, indexed
    project_id: ProjectId        FK → blueprints, indexed
```

**Key queries enabled:**
- "Build task for blueprint X" →
  `task_blueprint_refs.by_project_id(&x)`, then check task state
- "Which blueprint is this Build task targeting?" →
  `task_blueprint_refs.by_task_id(&task_id)`

**Deletion:** Cascade on `task_id` — when a task is removed, its
`TaskBlueprintRef` row is automatically deleted (requires F-tab-cascade-del).

**Notes:**
- Only Build tasks create rows here. This is a 1:1 relationship (one Build
  task targets one blueprint), but using a relationship table keeps the base
  Task free of variant-specific columns, consistent with the design principle
  of relationship tables over variant columns.

### TaskStructureRef

```
TaskStructureRef
    id: auto-PK (or composite TaskId+role)
    task_id: TaskId              FK → tasks, indexed
    structure_id: StructureId    FK → structures, indexed
    role: TaskStructureRole
```

**`TaskStructureRole` enum:**
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

**Deletion:** Cascade — when a task is removed, its `TaskStructureRef` rows
are automatically deleted (requires F-tab-cascade-del).

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

**Deletion:** Cascade — when a task is removed, its `TaskVoxelRef` rows
are automatically deleted (requires F-tab-cascade-del).

---

## Task Extension Tables

For non-FK mutable state specific to a task variant. These are small — most
task kinds need nothing here (GoTo, EatBread, Mope, Furnish, Harvest have
no extra mutable state beyond what's in the base task + relationship tables).

### TaskHaulData

```
TaskHaulData
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

**Deletion:** Cascade — when a task is removed, its `TaskHaulData` row
is automatically deleted (requires F-tab-cascade-del).

### TaskSleepData

```
TaskSleepData
    task_id: TaskId              PK, FK → tasks
    sleep_location: SleepLocationType  (Home, Dormitory, Ground)
```

**Notes:**
- The structure FK for Home/Dormitory is in `TaskStructureRef` with role
  `SleepAt`. The `SleepLocationType` here is just the discriminant for
  thought generation on completion.
- `bed_pos` is in `TaskVoxelRef` with role `BedPosition`.

**Deletion:** Cascade — when a task is removed, its `TaskSleepData` row
is automatically deleted (requires F-tab-cascade-del).

### TaskAcquireData

```
TaskAcquireData
    task_id: TaskId              PK, FK → tasks
    item_kind: ItemKind
    quantity: u32
```

**Deletion:** Cascade — when a task is removed, its `TaskAcquireData` row
is automatically deleted (requires F-tab-cascade-del).

---

## Task Kind Coverage

How each `TaskKind` variant maps to the new schema:

| Kind | Base task | Blueprint refs | Structure refs | Voxel refs | Extension |
|---|---|---|---|---|---|
| GoTo | kind_tag only | — | — | — | — |
| Build | kind_tag | TaskBlueprintRef | — | — | — |
| EatBread | kind_tag only | — | — | — | — |
| EatFruit | kind_tag | — | — | FruitTarget | — |
| Sleep | kind_tag | — | SleepAt (if bed) | BedPosition (if bed) | TaskSleepData |
| Haul | kind_tag | — | HaulDestination, HaulSourceBuilding? | HaulSourcePile? | TaskHaulData |
| Cook | kind_tag | — | CookAt | — | — |
| Harvest | kind_tag | — | — | FruitTarget | — |
| AcquireItem | kind_tag | — | AcquireSourceBuilding? | AcquireSourcePile? | TaskAcquireData |
| Furnish | kind_tag | — | FurnishTarget | — | — |
| Mope | kind_tag only | — | — | — | — |

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
    inventory_id: InventoryId    FK → inventories
    logistics_priority: Option<u8>      indexed
    cooking_enabled: bool
    cooking_bread_target: u32
```

**Key indexes:**
- `furnishing` — find kitchens, dormitories, homes (currently linear scans)
- `project_id` — reverse lookup from blueprint to structure (currently linear)
- `logistics_priority` filtered (IS NOT NULL) — logistics heartbeat

**Removed fields:**
- `inventory: Vec<Item>` → replaced by `inventory_id` FK to `inventories`
- `logistics_wants: Vec<LogisticsWant>` → moves to `logistics_wants` table
- `furniture_positions` / `planned_furniture` → moves to `furniture` table
- `assigned_elf: Option<CreatureId>` → removed entirely (redundant with
  querying `creatures.by_assigned_home(&structure_id)` to find residents)

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
    id: GroundPileId             PK (auto-generated UUID)
    position: VoxelCoord         indexed, unique
    inventory_id: InventoryId    FK → inventories
```

**Notes:**
- UUID PK with a unique index on position. Position is the natural lookup key
  (one pile per voxel), but UUID PK works with tabulosity's single-PK model.
- Empty piles are removed (and their inventory cleaned up).
- `HaulSourcePile` and `AcquireSourcePile` in `TaskVoxelRef` reference the
  position, not the pile ID. This matches the current pattern and avoids FK
  issues when piles are created/destroyed frequently.

---

## Logistics Wants

```
LogisticsWant
    id: auto-PK
    inventory_id: InventoryId    FK → inventories, indexed
    item_kind: ItemKind
    target_quantity: u32
```

**Notes:**
- Both creatures and structures have an `inventory_id`. Since logistics wants
  express "this inventory wants these items," the inventory is the natural
  owner. No polymorphic columns needed.
- To find wants for a specific creature or structure, look up its
  `inventory_id` first, then query `logistics_wants.by_inventory_id(&inv_id)`.

**Indexes:**
- `inventory_id` — "wants for this inventory"

---

## SimDb Definition Sketch

```rust
#[derive(Database)]
struct SimDb {
    #[table(singular = "creature")]
    creatures: CreatureTable,

    #[table(singular = "thought",
            fks(creature_id = "creatures"))]
    thoughts: ThoughtTable,

    #[table(singular = "task")]
    tasks: TaskTable,

    #[table(singular = "task_blueprint_ref",
            fks(task_id = "tasks", project_id = "blueprints"),
            cascade(task_id))]
    task_blueprint_refs: TaskBlueprintRefTable,

    #[table(singular = "task_structure_ref",
            fks(task_id = "tasks", structure_id = "structures"),
            cascade(task_id))]
    task_structure_refs: TaskStructureRefTable,

    #[table(singular = "task_voxel_ref",
            fks(task_id = "tasks"),
            cascade(task_id))]
    task_voxel_refs: TaskVoxelRefTable,

    #[table(singular = "task_haul_data",
            fks(task_id = "tasks"),
            cascade(task_id))]
    task_haul_data: TaskHaulDataTable,

    #[table(singular = "task_sleep_data",
            fks(task_id = "tasks"),
            cascade(task_id))]
    task_sleep_data: TaskSleepDataTable,

    #[table(singular = "task_acquire_data",
            fks(task_id = "tasks"),
            cascade(task_id))]
    task_acquire_data: TaskAcquireDataTable,

    #[table(singular = "blueprint")]
    blueprints: BlueprintTable,

    #[table(singular = "structure",
            fks(project_id = "blueprints",
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
            fks(inventory_id = "inventories"))]
    logistics_wants: LogisticsWantTable,

    #[table(singular = "furniture",
            fks(structure_id = "structures"))]
    furniture: FurnitureTable,
}
```

**Notable FK declarations on other tables (not repeated in SimDb derive):**
- `Creature.current_task` → tasks: FK declared on creatures table
- `Creature.assigned_home` → structures: FK declared on creatures table
- `Blueprint.task_id` → tasks: FK declared on blueprints table

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

1. **GC pass:** Periodically remove Complete tasks older than N ticks. With
   cascade delete on task relationship/extension tables, removing the parent
   task automatically cleans up all child rows.
2. **Filtered indexes:** Index only `state != Complete` tasks. Completed tasks
   exist but are invisible to active queries. This is the minimal change.
3. **Both:** Filtered indexes for query performance now, GC later when the
   table size becomes a memory concern.

Recommend option 3: filtered indexes immediately, GC deferred.

---

## Transition Strategy

*Deliberately left vague — to be fleshed out once the end-goal schema is
agreed upon.*

The general approach is **Option B: structural big bang, behavioral
incremental** — move all data into `SimDb` in one mechanical pass (translating
`BTreeMap` access to tabulosity calls), then incrementally add indexes, FK
declarations, and optimized query patterns.
