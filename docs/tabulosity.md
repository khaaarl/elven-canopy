# Tabulosity -- Design and Reference

Authoritative reference for tabulosity's design, current implementation, and
roadmap. **Supersedes** the older draft documents:
- `docs/drafts/sim_db_v9.md` -- implementation errata from the initial build
- `docs/drafts/tabulosity_advanced_indexes_v5.md` -- compound/filtered index design

Those drafts are retained as historical artifacts but should not be consulted
for current behavior. This document is the single source of truth.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Current API](#current-api)
   - [derive(Bounded)](#derivebounded)
   - [derive(Table)](#derivetable)
   - [derive(Database)](#derivedatabase)
   - [Serde Support](#serde-support)
4. [Index System](#index-system)
   - [Simple Indexes](#simple-indexes)
   - [Compound Indexes](#compound-indexes)
   - [Filtered Indexes](#filtered-indexes)
   - [Unified Query API (IntoQuery)](#unified-query-api-intoquery)
5. [Tracked Bounds](#tracked-bounds)
6. [Error Handling](#error-handling)
7. [Implementation Details](#implementation-details)
   - [FK Validation Behavior](#fk-validation-behavior)
   - [Row Type Name Convention](#row-type-name-convention)
   - [Type Name Canonicalization](#type-name-canonicalization)
   - [Serde DeserializeError Wrapping](#serde-deserializeerror-wrapping)
8. [Planned: Sim Integration](#planned-sim-integration)
   - [Migration Strategy](#migration-strategy)
   - [Candidate Tables](#candidate-tables)
9. [Roadmap: Missing Features](#roadmap-missing-features)
   - [Closure-based Row Update (F-tab-update-with)](#closure-based-row-update)
   - [Unique Index Enforcement (F-tab-unique-idx)](#unique-index-enforcement)
   - [Auto-generated Primary Keys (F-tab-auto-pk)](#auto-generated-primary-keys)
   - [Cascade/Nullify on Delete (F-tab-cascade-del)](#cascadenullify-on-delete)
   - [Change Tracking (F-tab-change-track)](#change-tracking)
   - [Join Iterators (F-tab-joins)](#join-iterators)
   - [Schema Evolution (F-tab-schema-evol)](#schema-evolution)
   - [Other Gaps](#other-gaps)

---

## Overview

Tabulosity is a lightweight, typed, in-memory relational database library for
Rust. It provides:

- **Typed tables** with primary keys and automatic secondary indexes
- **Compound and filtered indexes** using BTreeSet storage
- **Cross-table foreign key integrity** (restrict-on-delete)
- **Derive macros** for zero-boilerplate table and database definitions
- **Deterministic iteration** -- all internal data structures use
  `BTreeMap`/`BTreeSet` (no `HashMap`/`HashSet`)
- **Feature-gated serde** -- `Serialize`/`Deserialize` with FK validation on
  deserialization

Tabulosity was designed for the Elven Canopy simulation, where deterministic
iteration order is a hard requirement for lockstep multiplayer and replay
verification. It is not yet integrated into the sim crate -- it currently
exists as a standalone library with comprehensive tests.

### Determinism

Tabulosity is fully deterministic and suitable for use in deterministic
simulations. All internal data structures use `BTreeMap`/`BTreeSet` for
ordered iteration, and all code emitted by the derive macros follows the same
rule. `HashMap` and `HashSet` must never be introduced into library code,
generated code, or test helpers -- even where iteration order appears
irrelevant -- because hash ordering varies across platforms, Rust versions,
and process invocations. This guarantee is critical for `elven_canopy_sim`,
which requires identical results given the same seed for lockstep multiplayer
and replay verification.

## Architecture

Two crates in the workspace:

- **`tabulosity`** -- runtime library. Contains error types (`Error`,
  `DeserializeError`), the `Bounded` trait, `FkCheck` trait, `TableMeta` trait,
  and the unified query API (`IntoQuery`, `QueryBound`, `MatchAll`, `in_bounds`
  helper). Re-exports the derive macros from `tabulosity_derive`.

- **`tabulosity_derive`** -- proc macro crate. Three derive macros:
  - `#[derive(Bounded)]` -- newtype min/max delegation
  - `#[derive(Table)]` -- companion table struct with CRUD, indexes, serde
  - `#[derive(Database)]` -- FK-validated write methods, database-level serde

**Source files:**

```
tabulosity/
  src/
    lib.rs          -- re-exports, module declarations
    error.rs        -- Error enum (4 variants), DeserializeError
    table.rs        -- Bounded, FkCheck, TableMeta, IntoQuery, QueryBound, MatchAll, in_bounds
tabulosity_derive/
  src/
    lib.rs          -- proc macro entry points
    bounded.rs      -- derive(Bounded) for newtypes
    table.rs        -- derive(Table) -- companion struct, indexes, serde
    database.rs     -- derive(Database) -- FK validation, write methods, serde
    parse.rs        -- shared attribute parsing (#[primary_key], #[indexed], #[index])
```

**Test files:**

```
tabulosity/tests/
    basic_table.rs      -- CRUD on tables without indexes
    bounded.rs          -- derive(Bounded) on newtypes
    database.rs         -- FK validation, restrict-on-delete
    indexed_table.rs    -- simple, compound, filtered indexes; IntoQuery API
    serde.rs            -- serde roundtrip (feature-gated)
```

## Current API

### derive(Bounded)

Generates `Bounded` trait impl for single-field tuple structs (newtypes),
delegating `MIN`/`MAX` to the inner type.

```rust
#[derive(Bounded)]
struct CreatureId(u64);

assert_eq!(CreatureId::MIN, CreatureId(0));
assert_eq!(CreatureId::MAX, CreatureId(u64::MAX));
```

Blanket impls exist for all integer types, `bool`, and `Option<T: Bounded>`.

**Note:** The `Bounded` trait is a legacy convenience. Generated index code no
longer requires it -- tracked runtime bounds (see below) replaced the static
`Bounded` requirement. The trait is retained for user convenience and backward
compatibility.

### derive(Table)

Generates a companion `{Name}Table` struct. Given a `Creature` struct, you get
`CreatureTable` with:

**Storage:** `rows: BTreeMap<PK, Row>`

**Read methods:**
- `get(&PK) -> Option<Row>` -- cloned
- `get_ref(&PK) -> Option<&Row>` -- borrowed
- `contains(&PK) -> bool`
- `len() -> usize`
- `is_empty() -> bool`
- `keys() -> Vec<PK>` -- cloned, PK order
- `iter_keys() -> impl Iterator<Item = &PK>`
- `all() -> Vec<Row>` -- cloned, PK order
- `iter_all() -> impl Iterator<Item = &Row>`

**Mutation methods** (doc-hidden, used by Database derive):
- `insert_no_fk(row) -> Result<(), Error>` -- `DuplicateKey` on collision
- `update_no_fk(row) -> Result<(), Error>` -- `NotFound` if missing
- `upsert_no_fk(row)` -- infallible insert-or-update
- `remove_no_fk(&PK) -> Result<Row, Error>` -- returns removed row

**Index query methods** (per index): `by_{name}`, `iter_by_{name}`,
`count_by_{name}` -- see [Index System](#index-system).

**Other:** `new()` (empty table), `rebuild_indexes()` (used by deserialization),
`Default` impl.

**Field attributes:**
- `#[primary_key]` -- exactly one required
- `#[indexed]` -- zero or more, creates a simple single-field index

**Struct attributes:**
- `#[index(name = "...", fields("a", "b"), filter = "...")]` -- compound and/or
  filtered indexes (see below)

**Required trait bounds:**
- **Primary key type:** `Ord + Clone + Debug` (`Debug` is used for error messages)
- **Indexed field types:** `Ord + Clone` (needed for BTreeSet storage and
  tracked bounds maintenance)
- **Row type:** `Clone` (rows are cloned on get/insert/update)

These bounds apply to the concrete types used in `#[primary_key]` and
`#[indexed]`/`fields(...)` positions. If you define custom newtype IDs or enum
fields and use them in indexes, they must implement these traits. Standard
derives (`#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]`) cover all
requirements.

Example:

```rust
#[derive(Table, Clone, Debug)]
#[index(name = "species_hunger", fields("species", "hunger"))]
struct Creature {
    #[primary_key]
    id: CreatureId,
    #[indexed]
    species: Species,
    #[indexed]
    hunger: u32,
    name: String,
}
```

This produces `CreatureTable` with three indexes:
- `idx_species` -- simple, from `#[indexed]`
- `idx_hunger` -- simple, from `#[indexed]`
- `idx_species_hunger` -- compound, from `#[index(...)]`

### derive(Database)

Generates FK-validated write methods on a database schema struct. Every field
must have a `#[table(singular = "...", fks(...))]` attribute.

```rust
#[derive(Database)]
struct GameDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "task", fks(assignee? = "creatures"))]
    pub tasks: TaskTable,

    #[table(
        singular = "friendship",
        fks(source = "creatures", target = "creatures")
    )]
    pub friendships: FriendshipTable,
}
```

**Generated methods** (for each table):
- `insert_{singular}(row) -> Result<(), Error>` -- FK check then insert
- `update_{singular}(row) -> Result<(), Error>` -- FK check then update
- `upsert_{singular}(row) -> Result<(), Error>` -- FK check then upsert
- `remove_{singular}(&PK) -> Result<Row, Error>` -- restrict-on-delete check

**FK syntax:**
- `fks(field_name = "target_table")` -- bare FK, field type is `T`
- `fks(field_name? = "target_table")` -- optional FK, field type is `Option<T>`
- Multiple FKs: `fks(source = "creatures", target = "creatures")`

The `?` suffix on the field name signals that the field is `Option<T>`. For
restrict-on-delete checks, optional FK fields are queried by wrapping the
target PK in `Some(...)`.

**Also generates:** `new()`, `Default` impl.

### Serde Support

Behind `#[cfg(feature = "serde")]`:

**Table serialization:** Tables serialize as JSON arrays of rows in PK order.
Deserialization rebuilds indexes via `rebuild_indexes()`. Duplicate PKs in
serialized data produce an error.

**Database serialization:** Databases serialize as JSON objects with one field
per table. Deserialization validates FK constraints across all tables and
collects ALL errors (duplicate PKs + FK violations) into a `DeserializeError`,
rather than failing fast.

## Index System

All indexes are stored as `BTreeSet` of tuples: `(field_values..., PK)`. The
PK is appended as a tiebreaker to ensure uniqueness (since multiple rows can
share the same indexed field value).

### Simple Indexes

Created by `#[indexed]` on a field. Sugar for a single-field compound index.

```rust
#[indexed]
species: Species,
```

Produces `idx_species: BTreeSet<(Species, CreatureId)>` and methods
`by_species`, `iter_by_species`, `count_by_species`.

Each method accepts `impl IntoQuery<Species>`, so you can pass:
- `&Species::Elf` -- exact match
- `Species::Elf..Species::Capybara` -- range
- `MatchAll` or `..` -- all values

### Compound Indexes

Created by `#[index(name = "...", fields("a", "b"))]` on the struct.

```rust
#[index(name = "species_hunger", fields("species", "hunger"))]
```

Produces `idx_species_hunger: BTreeSet<(Species, u32, CreatureId)>` and methods
`by_species_hunger`, `iter_by_species_hunger`, `count_by_species_hunger`.

Each method takes one `impl IntoQuery<T>` parameter per field:

```rust
// Exact match on both fields
table.by_species_hunger(&Species::Elf, &30)

// Prefix query: exact first, all second
table.by_species_hunger(&Species::Elf, MatchAll)

// Range on first, exact on second
table.by_species_hunger(Species::Elf..Species::Capybara, &30)

// All rows (useful for filtered indexes)
table.by_species_hunger(MatchAll, MatchAll)
```

**Leftmost prefix semantics:** Leading exact fields use the BTreeSet range scan
efficiently. Once a non-exact (range or MatchAll) field appears, subsequent
fields are post-filtered. This matches the mental model of database compound
indexes.

**PK field prohibition:** The primary key field must NOT appear in `fields(...)`.
It is automatically appended as the tiebreaker. Including it produces a
compile-time error.

### Filtered Indexes

Add `filter = "path::to::fn"` to only index rows matching a predicate:

```rust
#[index(name = "active_priority", fields("assignee", "priority"), filter = "Task::is_active")]
```

The filter function must have signature `fn(&Row) -> bool`.

**Index maintenance on mutations:**
- Insert: only add to index if filter passes
- Update: four cases based on (old_passes, new_passes) -- remove, add, update,
  or no-op
- Remove: remove from index if the removed row was in it
- Rebuild: re-evaluate filter for all rows

A filter with a single field creates a filtered-only index, useful for
frequently queried subsets.

**Panic safety:** Filter functions must not panic. A panic during index
maintenance (insert, update, or rebuild) leaves the table in an inconsistent
state -- the row storage and index may disagree about which rows exist. Keep
filter logic simple and infallible.

### Unified Query API (IntoQuery)

The `IntoQuery<T>` trait converts query parameters into `QueryBound<T>`:

```rust
pub enum QueryBound<T> {
    Exact(T),
    Range { start: Bound<T>, end: Bound<T> },
    MatchAll,
}
```

**Implementations:**
| Input | Resolves to |
|---|---|
| `&T` | `QueryBound::Exact(T)` (cloned) |
| `a..b` | `QueryBound::Range` (start included, end excluded) |
| `a..=b` | `QueryBound::Range` (both included) |
| `a..` | `QueryBound::Range` (start included, end unbounded) |
| `..b` | `QueryBound::Range` (start unbounded, end excluded) |
| `..=b` | `QueryBound::Range` (start unbounded, end included) |
| `..` | `QueryBound::MatchAll` |
| `MatchAll` | `QueryBound::MatchAll` |
| `(Bound<T>, Bound<T>)` | `QueryBound::Range` (arbitrary bounds) |

**Note on owned vs reference:** Exact matches use `&T` (reference), while
ranges use owned values (following Rust's standard `Range<T>` convention).

**Generated query method structure:** Each index produces three public methods
(`by_*`, `iter_by_*`, `count_by_*`) that convert parameters via `IntoQuery` and
delegate to a private `_query_*` helper. The helper uses a match cascade with
N+1 arms (from most exact to least exact), selecting the optimal BTreeSet range
scan and post-filter strategy.

## Tracked Bounds

Instead of requiring the static `Bounded` trait on all indexed types, tabulosity
uses **tracked runtime bounds** -- `Option<(min, max)>` pairs that widen on
every insert/update. This is a cross-cutting concern that affects all index
types (simple, compound, and filtered).

**Key properties:**
- One tracked bounds pair per unique type across all indexes (not per field)
- `Option<CreatureId>` and `CreatureId` are separate types with separate bounds
- Bounds never shrink -- only grow outward ("ever seen" semantics)
- Removing a row does NOT contract bounds (stale bounds are safe -- wider bounds
  never exclude valid results, they just cause the BTreeSet scan to visit a few
  empty nodes at the edges)
- `rebuild_indexes()` recomputes bounds from current data (resets staleness)
- When bounds are `None` (empty table), queries return empty immediately

**Type name canonicalization:** The proc macro derives field names from types:
- `u8` -> `_bounds_u8`
- `TaskId` -> `_bounds_task_id`
- `Option<CreatureId>` -> `_bounds_option_creature_id`
- Path prefixes (`crate::`, etc.) are stripped

This enables `String` and other types without known compile-time MIN/MAX to be
used in indexes.

## Error Handling

All errors are returned via the `Error` enum:

```rust
pub enum Error {
    DuplicateKey { table: &'static str, key: String },
    NotFound { table: &'static str, key: String },
    FkTargetNotFound { table, field, referenced_table, key },
    FkViolation { table, key, referenced_by: Vec<(table, field, count)> },
}
```

`DeserializeError` wraps `Vec<Error>` for bulk-loading scenarios.

Both implement `Display` and `std::error::Error`.

## Implementation Details

### FK Validation Behavior

**Outbound FK checks (insert/update/upsert):** Short-circuit on the first FK
target miss. If a row has two FK fields and the first fails validation, the
second is not checked. Rationale: the caller needs to fix the first problem
before retrying anyway.

**Inbound FK checks (restrict-on-delete):** Collect ALL violations across all
referencing tables/fields. The error reports every referencing table, field name,
and count of references. Rationale: the caller needs the full picture to decide
whether to delete dependents, reassign them, or abort.

### Row Type Name Convention

The `Database` derive macro obtains the row type name by stripping the `"Table"`
suffix from the field's type name. For example, `CreatureTable` -> `Creature`.

**Convention (must follow):** Table companion structs MUST be named
`{RowName}Table`.

**Potential improvement:** Add an explicit `row = "..."` attribute to
`#[table(...)]` to make the association explicit and remove the naming
convention requirement.

### Type Name Canonicalization

See also [Tracked Bounds](#tracked-bounds) for an overview of the bounds system
and examples of canonicalized names.

Tracked bounds field names are derived from types by converting the last path
segment to snake_case and appending generic parameters recursively. Two fields
with the same written type share one tracked bounds pair.

Supported types: simple paths (no generics) and `Option<T>` where T is simple.
Other compound generics (`Vec<T>`, `Result<A, B>`, etc.) are not supported in
index fields.

Type aliases (`type Foo = Bar`) are undetectable by the proc macro and will
create redundant tracking -- this is a known limitation.

### Serde DeserializeError Wrapping

`DeserializeError` is wrapped via `serde::de::Error::custom()` when returned
through serde's deserialization pathway. This makes `DeserializeError`
inaccessible as a typed value -- callers must inspect error message strings.

**Potential improvement:** Add a separate `from_value` entrypoint that bypasses
serde's error type and returns `Result<Db, DeserializeError>` directly, enabling
pattern matching on specific error variants.

## Planned: Sim Integration

Tabulosity was designed to replace the raw `BTreeMap` entity storage in
`elven_canopy_sim`. This section describes the planned integration.

### Migration Strategy

The sim currently stores entities in hand-managed `BTreeMap<EntityId, Entity>`
fields on `SimState`. Migration would:

1. Define row types for each entity (creatures, tasks, voxels, nav nodes, etc.)
2. Define a `SimDb` struct with `#[derive(Database)]`
3. Replace raw BTreeMap lookups with tabulosity's typed API
4. Replace ad-hoc iteration with index-based queries
5. Wire serde through the Database derive for save/load

The migration can be incremental -- one table at a time, starting with the
simplest entities (e.g., creatures) and progressing to more complex ones.

### Candidate Tables

Based on the current sim architecture, likely tables include:

| Table | PK | Key Indexes | FKs |
|---|---|---|---|
| `creatures` | `CreatureId` | `species`, `location` | -- |
| `tasks` | `TaskId` | `assignee`, `status`, `task_type` | `assignee -> creatures` |
| `items` | `ItemId` | `owner`, `item_type`, `location` | `owner? -> creatures` |
| `buildings` | `BuildingId` | `building_type`, `location` | -- |
| `thoughts` | `ThoughtId` | `creature`, `category` | `creature -> creatures` |

Compound indexes would be valuable for queries like:
- "all pending tasks assigned to creature X" (compound: assignee + status)
- "all items at location Y" (simple: location)
- "all hungry elves" (filtered: species + hunger with hunger > threshold)

## Roadmap: Missing Features

Features needed for full sim integration, roughly ordered by priority.

### Closure-based Row Update

**Tracker:** F-tab-update-with

`db.update_creature_with(&id, |c| c.hunger += 10)` to avoid the
clone-modify-update pattern. The closure mutates the row in place; indexes are
updated afterward by comparing old and new field values. Low complexity.

This is the single highest-impact missing feature for sim integration. The
current API requires cloning a row, modifying the clone, and calling
`update_*()` -- verbose and wasteful for frequent field updates like hunger,
position, and tick counters.

### Unique Index Enforcement

**Tracker:** F-tab-unique-idx

`#[indexed(unique)]` enforced on insert and update -- returns an error if a
duplicate value is found. Low complexity, builds on existing index
infrastructure. Useful for fields like creature names or external identifiers
that must be unique.

### Auto-generated Primary Keys

**Tracker:** F-tab-auto-pk

`#[primary_key(auto)]` with a monotonic counter so callers don't need to
generate IDs manually. `insert_creature_auto()` returns the generated key.
Medium complexity.

Simplifies entity creation in the sim, where IDs are currently managed by a
hand-rolled counter. The auto-PK counter must be deterministic (part of sim
state) and survive serde roundtrips.

### Cascade/Nullify on Delete

**Tracker:** F-tab-cascade-del

`on_delete cascade` or `on_delete nullify` in the `fks()` syntax. Cascade
removes dependent rows; nullify sets the FK field to `None`. Medium complexity.

Currently, all FK violations use restrict semantics (block deletion). The sim
needs cascade for "when a creature dies, remove its tasks" and nullify for
"when a creature dies, unassign its items."

### Change Tracking

**Tracker:** F-tab-change-track

Tables emit insert/update/delete diffs per tick, enabling event-driven
rendering. The rendering layer can subscribe to changes rather than polling
the full table each frame. Medium complexity.

Not blocking for initial sim integration, but important for performance once
entity counts grow.

### Join Iterators

**Tracker:** F-tab-joins

`db.tasks.join_assignee()` returning an iterator of `(&Task, &Creature)`.
High complexity due to lifetime management and derive macro codegen.

### Schema Evolution

**Tracker:** F-tab-schema-evol

Migration utilities for adding/removing fields across save-file versions.
Needed for long-lived save games that span schema changes. High complexity.

### Other Gaps

Additional items identified during design review that don't have dedicated
tracker entries:

- **`Vec<T>` FK fields.** The current FK system handles `T` and `Option<T>`
  but not `Vec<T>`. A creature with a `Vec<ItemId>` inventory would need
  special handling -- the `FkCheck` trait would need a `Vec` impl that
  validates all elements.

- **In-place mutation / `get_mut`.** Beyond `update_with`, a raw `get_mut`
  that returns `&mut Row` would be useful for hot paths, but it cannot
  maintain indexes automatically. If exposed, it would need a manual
  `reindex(&PK)` call afterward, which is error-prone. The closure-based
  approach (F-tab-update-with) is safer.

- **Bulk operations.** `insert_batch`, `remove_where`, etc. for efficiency
  when making many changes at once (e.g., ticking all creatures). Currently
  each mutation is individual.

- **Query result ordering.** Index queries return rows in index order (the
  BTreeSet's natural order). There is no way to request a different sort
  order. For compound indexes, this is the tuple's lexicographic order.
  Alternative orderings require collecting and sorting the results.

- **Row type `row = "..."` attribute.** The current convention of stripping
  `"Table"` from the type name to find the row type is fragile. An explicit
  `#[table(row = "Creature")]` attribute would make the association clear
  and produce better error messages. See
  [Row Type Name Convention](#row-type-name-convention).

- **`from_value` deserialization entrypoint.** A non-serde pathway for
  database deserialization that returns typed `DeserializeError` instead of
  wrapping it in a serde error string. See
  [Serde DeserializeError Wrapping](#serde-deserializeerror-wrapping).
