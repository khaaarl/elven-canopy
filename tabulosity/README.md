# Tabulosity

A typed, in-memory relational database library for Rust. Define row structs,
derive companion table types with indexes, and compose them into a database
with foreign key integrity -- all via derive macros.

Built for determinism: all internal data structures use `BTreeMap`/`BTreeSet`,
never `HashMap`/`HashSet`, so iteration order is always reproducible.

## Quick Start

```rust
use tabulosity::{Bounded, Table, Database, QueryOpts};

// 1. Define a primary key type.
#[derive(Bounded, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct UserId(u32);

// 2. Define a row struct and derive Table.
//    This generates a companion `UserTable` type with typed CRUD + index queries.
#[derive(Table, Clone, Debug)]
struct User {
    #[primary_key]
    pub id: UserId,
    pub name: String,
    #[indexed]
    pub role: Role,
}

// 3. Compose tables into a database with FK validation.
#[derive(Database)]
struct AppDb {
    #[table(singular = "user")]
    pub users: UserTable,

    #[table(singular = "post", fks(author = "users"))]
    pub posts: PostTable,
}
```

## Defining Rows

A row is a plain Rust struct with `#[derive(Table)]`. One field must be marked
`#[primary_key]`. The PK type must implement `Clone + Eq + Ord` (plus `Hash` if using
hash indexes or hash primary storage).

### Primary Key Types

PK types are typically newtypes. Derive `Bounded` to get `MIN`/`MAX` constants
and auto-increment support:

```rust
#[derive(Bounded, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ItemId(u32);
```

`Bounded` is implemented for all integer primitives, `bool`, and `Option<T>`.

### Auto-Increment

Use `#[primary_key(auto_increment)]` to have the table assign PKs
automatically. IDs start at 0 and increment monotonically:

```rust
#[derive(Table, Clone, Debug)]
struct Item {
    #[primary_key(auto_increment)]
    pub id: ItemId,
    pub name: String,
}

let mut table = ItemTable::new();
// insert_auto_no_fk takes a closure that receives the generated PK:
let id = table.insert_auto_no_fk(|pk| Item { id: pk, name: "Sword".into() }).unwrap();
assert_eq!(id, ItemId(0));
```

For non-PK fields (typically sequence numbers in compound-PK child tables),
use `#[auto_increment]` without `#[primary_key]`. The table generates a
`next_<field>()` counter. See [Parent + Sequence Child Tables](#parent--sequence-child-tables)
for the full pattern.

## Indexes

Fields marked `#[indexed]` get a secondary index. The table generates
`by_<field>`, `iter_by_<field>`, and `count_by_<field>` query methods.

### Simple Indexes

```rust
#[derive(Table, Clone, Debug)]
struct Creature {
    #[primary_key]
    pub id: CreatureId,
    #[indexed]
    pub species: Species,
    pub name: String,
}

let elves = table.by_species(&Species::Elf, QueryOpts::ASC);
let count = table.count_by_species(&Species::Elf, QueryOpts::ASC);
```

### Index Variants

| Attribute              | Storage    | Lookup | Ordering          |
|------------------------|------------|--------|-------------------|
| `#[indexed]`           | BTreeSet   | Range  | Sorted            |
| `#[indexed(unique)]`   | BTreeSet   | Range  | Sorted, unique    |
| `#[indexed(hash)]`     | InsOrdHash | O(1)   | Insertion order   |
| `#[indexed(hash, unique)]` | InsOrdHash | O(1) | Insertion order, unique |
| `#[indexed(spatial)]`  | R-tree     | Intersection | Sorted by PK |

Unique indexes reject duplicate values on insert/update with
`Error::DuplicateIndex`.

### Compound Indexes

Struct-level `#[index(...)]` creates multi-field indexes supporting prefix
queries:

```rust
#[derive(Table, Clone, Debug)]
#[index(name = "by_owner_priority", fields("owner", "priority"))]
struct Task {
    #[primary_key]
    pub id: TaskId,
    pub owner: PlayerId,
    pub priority: u32,
    pub label: String,
}

// Query by first field only (prefix query — use MatchAll for remaining fields):
let player_tasks = table.by_owner_priority(&player_id, MatchAll, QueryOpts::ASC);
// Query by both fields:
let urgent = table.by_owner_priority(&player_id, &0, QueryOpts::ASC);
```

### Filtered Indexes

Index only rows matching a predicate:

```rust
// Filter must be a free fn taking &Row -> bool, defined before the struct:
fn is_active(task: &Task) -> bool { task.active }

#[derive(Table, Clone, Debug)]
#[index(name = "active_by_owner", fields("owner"), filter = "is_active")]
struct Task {
    #[primary_key]
    pub id: TaskId,
    pub owner: PlayerId,
    pub active: bool,
    pub label: String,
}

// Only returns tasks where is_active() returns true:
let active = table.by_active_by_owner(&player_id, QueryOpts::ASC);
```

### Spatial Indexes

Fields implementing `SpatialKey` can use R-tree-backed spatial indexes for
axis-aligned bounding box intersection queries. Results are always sorted
by PK for determinism.

```rust
use tabulosity::SpatialKey;

#[derive(Clone, Debug, PartialEq, Eq)]
struct BBox { min: [i32; 3], max: [i32; 3] }

impl SpatialKey for BBox {
    type Point = [i32; 3];  // or [i32; 2] for 2D
    fn spatial_min(&self) -> [i32; 3] { self.min }
    fn spatial_max(&self) -> [i32; 3] { self.max }
}

#[derive(Table, Clone, Debug)]
struct Entity {
    #[primary_key]
    pub id: EntityId,
    #[indexed(spatial)]
    pub bounds: BBox,          // required field
    // pub bounds: Option<BBox>,  // also works — None entries excluded from R-tree
}

// Intersection query — finds all entities whose bounds overlap the envelope:
let nearby = table.intersecting_bounds(&BBox { min: [0,0,0], max: [10,10,10] });
let count = table.count_intersecting_bounds(&envelope);
```

Spatial indexes can also use struct-level syntax with filters:
`#[index(name = "pos", fields("bounds"), kind = "spatial", filter = "is_alive")]`

**Constraints:** Spatial indexes cannot be `unique`, cannot index PK fields,
and are limited to one field (compound spatial indexes are not yet supported).

**Serde:** Spatial indexes are transient — not serialized. They are rebuilt
automatically from row data on deserialization.

## Reading Data

All read methods are on the table struct directly (`db.users`, not `db`):

```rust
// By primary key
let user: Option<User> = db.users.get(&user_id);       // owned copy
let user: Option<&User> = db.users.get_ref(&user_id);  // borrowed

// Existence check
let exists: bool = db.users.contains(&user_id);

// All rows (PK order)
let all: Vec<User> = db.users.all();
for user in db.users.iter_all() { /* &User */ }

// All keys (PK order)
let pks: Vec<UserId> = db.users.keys();
for pk in db.users.iter_keys() { /* &UserId */ }

// Count and emptiness
let n: usize = db.users.len();
let empty: bool = db.users.is_empty();

// By index (see Indexes section)
let elves = db.creatures.by_species(&Species::Elf, QueryOpts::ASC);
```

### Query Options

`QueryOpts` controls ordering and offset on index queries:

```rust
use tabulosity::QueryOpts;

// Ascending (default)
table.by_species(&Species::Elf, QueryOpts::ASC);

// Descending
table.by_species(&Species::Elf, QueryOpts::DESC);

// Skip first N results
table.by_species(&Species::Elf, QueryOpts::offset(5));
```

### Range Queries

Index queries accept ranges via the `IntoQuery` trait:

```rust
// Exact match
table.by_priority(&5, QueryOpts::ASC);

// Range (half-open)
table.by_priority(3..7, QueryOpts::ASC);

// Range (inclusive)
table.by_priority(3..=7, QueryOpts::ASC);

// All values
table.by_priority(.., QueryOpts::ASC);
// or: table.by_priority(MatchAll, QueryOpts::ASC);
```

## Writing Data

Tabulosity provides two tiers of write methods. **Use the safe tier by
default.** The unchecked tier exists for performance-critical inner loops and
should be reached for deliberately, not habitually.

### Safe: Database-Level Methods (Recommended)

The `#[derive(Database)]` macro generates safe write methods that **validate
foreign keys** and **maintain all indexes**:

```rust
// Insert -- validates FKs, rejects duplicate PKs
db.insert_user(user)?;

// Update -- replaces the row, validates FKs, rejects if PK not found
db.update_user(user)?;

// Upsert -- insert or update, validates FKs either way
db.upsert_user(user)?;

// Remove -- checks FK constraints (restrict/cascade/nullify)
db.remove_user(&user_id)?;

// Auto-increment insert
let id = db.insert_item_auto(|pk| Item { id: pk, name: "Bow".into() })?;
```

**This is the right default.** These methods are safe, correct, and handle
index maintenance automatically. If you're writing new code and wondering "how
do I update a row?", the answer is `db.update_<singular>(row)`.

### Table-Level Methods (`_no_fk`)

The table struct has lower-level methods that maintain indexes but skip FK
validation:

```rust
table.insert_no_fk(row)?;
table.update_no_fk(row)?;
table.upsert_no_fk(row)?;
table.remove_no_fk(&key)?;
```

**These are not your default.** They exist primarily for internal use by the
Database derive macro and for rare cases where you are truly confident that FK
integrity is handled elsewhere. If you have a `Database` struct, use the
database-level methods above -- they call these internally after validating
FKs, so there is no reason to bypass them.

### Unchecked: Closure-Based Mutation

> **Strong recommendation:** Do not reach for `modify_unchecked` unless (1)
> you have profiled and confirmed a real performance need, AND (2) you are
> confident that the fields being mutated will never gain an index in the
> future. Every call site should include a comment explaining **why** unchecked
> mutation is necessary and **which fields** are being modified. If in doubt,
> use `db.update_<singular>(row)` -- it is always correct.

`modify_unchecked` mutates a row in place **without rebuilding indexes**:

```rust
// SAFETY: only modifying hp and last_hit_tick, neither is indexed.
db.creatures.modify_unchecked(&creature_id, |c| {
    c.hp -= damage;
    c.last_hit_tick = now;
})?;
```

Batch variants:

```rust
db.creatures.modify_unchecked_range(start..end, |_pk, c| { ... });
db.creatures.modify_unchecked_all(|_pk, c| { ... });
```

`modify_each_by_<index>` combines an index query with `modify_unchecked`:

```rust
db.creatures.modify_each_by_species(&Species::Elf, QueryOpts::ASC, |_pk, c| { ... });
```

**Rules:**
- **Never modify the primary key or any `#[indexed]` field.** In debug builds,
  this panics. In release builds, it silently corrupts indexes.
- If you need to change an indexed field, use `update` or `upsert` instead --
  they remove the old index entry and insert the new one.
- If a field you're modifying gains an `#[indexed]` attribute later, every
  `modify_unchecked` call site touching that field becomes a silent corruption
  bug. This is why the recommendation to prefer `update` is so strong.

### Decision Guide

| Situation | Method |
|-----------|--------|
| Adding a new row | `db.insert_<singular>(row)` |
| Replacing / updating a row | `db.update_<singular>(row)` |
| Insert-or-update | `db.upsert_<singular>(row)` |
| Removing a row | `db.remove_<singular>(&key)` |
| Changing an indexed field | `db.update_<singular>(row)` (never modify_unchecked) |
| Profiled hot path, non-indexed fields only | `modify_unchecked` (with justifying comment) |

## Databases and Foreign Keys

`#[derive(Database)]` composes tables and generates FK-validated write methods.

### Declaring Foreign Keys

```rust
#[derive(Database)]
struct MyDb {
    #[table(singular = "user")]
    pub users: UserTable,

    // required FK: author must exist in users
    #[table(singular = "post", fks(author = "users"))]
    pub posts: PostTable,

    // optional FK: reviewer may be null
    #[table(singular = "review", fks(reviewer? = "users"))]
    pub reviews: ReviewTable,
}
```

### On-Delete Behavior

```rust
#[table(singular = "post", fks(author = "users" on_delete cascade))]
#[table(singular = "tag",  fks(author? = "users" on_delete nullify))]
```

| Behavior | What happens when parent is deleted |
|----------|-------------------------------------|
| **restrict** (default) | Delete fails with `Error::FkViolation` |
| **cascade** | Dependent rows are automatically deleted |
| **nullify** | FK field set to `None` (requires `Option<T>`) |

### 1:1 Relationships (Parent-PK FK)

When a child's PK is also its FK to the parent:

```rust
#[table(singular = "tree_info", fks(id = "trees" pk))]
pub tree_infos: TreeInfoTable,
```

The `pk` marker tells the derive macro that `id` is both the primary key of
`tree_infos` and a foreign key to `trees`.

### Auto-Increment at Database Level

For tables with `#[primary_key(auto_increment)]`, add `auto` to the database
attribute:

```rust
#[table(singular = "item", auto)]
pub items: ItemTable,
```

This generates `db.insert_item_auto(|pk| ...)` with FK validation.

## Common Patterns

### Compound Primary Keys

For child rows and junction tables, use a struct-level `#[primary_key(...)]`
with multiple fields:

```rust
#[derive(Table, Clone, Debug)]
#[primary_key("creature_id", "trait_kind")]
struct CreatureTrait {
    #[indexed]
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i64,
}
```

The generated key type is a tuple -- here `(CreatureId, TraitKind)`. Lookups,
updates, and removes all take `&(creature_id, trait_kind)`.

Note that `creature_id` is also `#[indexed]`, which lets you query all traits
for a creature via `table.by_creature_id(&id, QueryOpts::ASC)`. This is the
normal pattern: the first field of a compound PK is almost always indexed so
you can query by parent.

### Parent + Sequence Child Tables

The most common compound PK pattern is `(parent_id, seq)` for ordered 1:N
child rows. Combine with `#[auto_increment]` on `seq` so you don't have to
manage sequence numbers manually:

```rust
#[derive(Table, Clone, Debug)]
#[primary_key("task_id", "seq")]
struct TaskBlueprintRef {
    #[indexed]
    pub task_id: TaskId,
    #[auto_increment]
    pub seq: u64,
    #[indexed]
    pub project_id: ProjectId,
}
```

On the database side, declare with `nonpk_auto`:

```rust
#[table(singular = "task_blueprint_ref",
        nonpk_auto,
        fks(task_id = "tasks" on_delete cascade,
            project_id = "blueprints"))]
pub task_blueprint_refs: TaskBlueprintRefTable,
```

Note: `nonpk_auto` does not generate a database-level auto-insert method.
Use the table's `insert_auto_no_fk` directly for auto-incrementing the
sequence field, or use `db.insert_task_blueprint_ref(row)` with a manually
assigned sequence from `db.task_blueprint_refs.next_seq()`.

The `on_delete cascade` on `task_id` means deleting a task automatically
removes all its blueprint refs.

### Junction Tables

For many-to-many relationships, both fields of the compound PK are indexed
FKs:

```rust
#[derive(Table, Clone, Debug)]
#[primary_key("activity_id", "creature_id")]
struct ActivityParticipant {
    #[indexed]
    pub activity_id: ActivityId,
    #[indexed]
    pub creature_id: CreatureId,
    pub role: ParticipantRole,
}
```

Both indexes let you query from either direction:
- `table.by_activity_id(&id, ...)` -- all participants in an activity
- `table.by_creature_id(&id, ...)` -- all activities a creature is in

On the database side, both FKs typically cascade on delete:

```rust
#[table(singular = "activity_participant",
        fks(activity_id = "activities" on_delete cascade,
            creature_id = "creatures" on_delete cascade))]
pub activity_participants: ActivityParticipantTable,
```

## Serde Support

Enable with the `serde` feature flag:

```toml
tabulosity = { path = "../tabulosity", features = ["serde"] }
```

Tables serialize as JSON arrays (rows in PK order). Databases serialize as
JSON objects. Deserialization rebuilds all indexes automatically and validates
all FK constraints, collecting errors into `DeserializeError`:

```rust
let json = serde_json::to_string(&db)?;
let db: MyDb = serde_json::from_str(&json)?;
```

Missing tables in the JSON default to empty (allows additive schema changes).

### Schema Versioning

```rust
#[derive(Database)]
#[schema_version(1)]
struct MyDb { ... }
```

Serialization includes `"schema_version": 1`. Deserialization rejects
mismatched versions.

## Error Handling

All fallible operations return `Result<T, tabulosity::Error>`:

| Variant | When |
|---------|------|
| `DuplicateKey` | Insert with an existing PK |
| `NotFound` | Update/remove with a missing PK |
| `DuplicateIndex` | Unique index violation |
| `FkTargetNotFound` | FK target doesn't exist (insert/update) |
| `FkViolation` | Deleting a row that others reference (restrict) |

## Historical Reference

The original design and implementation reference is preserved at
`docs/drafts/tabulosity_design_reference.md`. It covers internal architecture,
derive macro codegen details, and the roadmap. Useful for tabulosity
maintainers, but not needed for day-to-day usage.
