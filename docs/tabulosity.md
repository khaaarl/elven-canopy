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
   - [Unique Indexes](#unique-indexes)
   - [Unified Query API (IntoQuery)](#unified-query-api-intoquery)
5. [Query Options (QueryOpts)](#query-options-queryopts)
6. [Closure-based Mutation](#closure-based-mutation)
   - [modify_unchecked (Single Row)](#modify_unchecked-single-row)
   - [modify_unchecked_range / modify_unchecked_all (Batch)](#modify_unchecked_range--modify_unchecked_all-batch)
   - [modify_each_by_* (Index-Driven Batch)](#modify_each_by-index-driven-batch)
   - [Debug Assertions](#debug-assertions)
7. [Auto-Increment Primary Keys](#auto-increment-primary-keys)
8. [Tracked Bounds](#tracked-bounds)
9. [Error Handling](#error-handling)
10. [Implementation Details](#implementation-details)
    - [FK Validation Behavior](#fk-validation-behavior)
    - [Row Type Name Convention](#row-type-name-convention)
    - [Type Name Canonicalization](#type-name-canonicalization)
    - [Serde DeserializeError Wrapping](#serde-deserializeerror-wrapping)
11. [Planned: Sim Integration](#planned-sim-integration)
    - [Migration Strategy](#migration-strategy)
    - [Candidate Tables](#candidate-tables)
12. [Roadmap: Missing Features](#roadmap-missing-features)
    - [Index-Maintaining Closure Update](#index-maintaining-closure-update)
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
- **Unique index enforcement** -- `#[indexed(unique)]` with duplicate detection
  on insert and update
- **Auto-increment primary keys** -- `#[primary_key(auto_increment)]` with
  monotonic counters that survive serde roundtrips
- **Non-PK auto-increment** -- `#[auto_increment]` on non-PK fields for
  globally unique tiebreaker columns in compound-PK child tables
- **Cross-table foreign key integrity** -- restrict, cascade, and nullify
  on-delete semantics with cycle detection
- **Query options** -- ascending/descending ordering and offset (skip N) on all
  index query methods
- **Closure-based mutation** -- `modify_unchecked` for in-place row mutation
  bypassing index maintenance (single-row, range, and batch variants), plus
  `modify_each_by_*` for index-driven batch mutation
- **Derive macros** for zero-boilerplate table and database definitions
- **Deterministic iteration** -- all internal data structures use
  `BTreeMap`/`BTreeSet` (no `HashMap`/`HashSet`)
- **Feature-gated serde** -- `Serialize`/`Deserialize` with FK validation on
  deserialization

Tabulosity was designed for the Elven Canopy simulation, where deterministic
iteration order is a hard requirement for lockstep multiplayer and replay
verification. It is integrated into `elven_canopy_sim` as `SimDb` (16 tables
replacing all BTreeMap entity storage) and also exists as a standalone library
with comprehensive tests.

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
  `AutoIncrementable` trait, and the unified query API (`IntoQuery`,
  `QueryBound`, `MatchAll`, `in_bounds` helper). Also provides `QueryOrder` and
  `QueryOpts` for controlling result ordering and offset. Re-exports the derive
  macros from `tabulosity_derive`.

- **`tabulosity_derive`** -- proc macro crate. Three derive macros:
  - `#[derive(Bounded)]` -- newtype min/max delegation
  - `#[derive(Table)]` -- companion table struct with CRUD, indexes, serde,
    modify_unchecked, and modify_each_by_*
  - `#[derive(Database)]` -- FK-validated write methods, database-level serde,
    delegation of modify_unchecked methods

**Source files:**

```
tabulosity/
  src/
    lib.rs          -- re-exports, module declarations
    error.rs        -- Error enum (5 variants), DeserializeError
    table.rs        -- Bounded, FkCheck, TableMeta, AutoIncrementable,
                       IntoQuery, QueryBound, MatchAll, QueryOrder, QueryOpts,
                       in_bounds
tabulosity_derive/
  src/
    lib.rs          -- proc macro entry points
    bounded.rs      -- derive(Bounded) for newtypes
    table.rs        -- derive(Table) -- companion struct, indexes, serde,
                       modify_unchecked, modify_each_by_*
    database.rs     -- derive(Database) -- FK validation, write methods, serde,
                       modify_unchecked delegation
    parse.rs        -- shared attribute parsing (#[primary_key], #[auto_increment], #[indexed], #[index])
```

**Test files:**

```
tabulosity/tests/
    auto_increment.rs              -- auto-increment PK generation and serde roundtrip
    basic_table.rs                 -- CRUD on tables without indexes
    bounded.rs                     -- derive(Bounded) on newtypes
    compound_pk.rs                 -- compound (multi-column) PK CRUD and indexes
    compound_pk_database.rs        -- compound PK with Database derive, FKs, cascades
    compound_pk_serde.rs           -- compound PK serde roundtrip
    database.rs                    -- FK validation, restrict/cascade/nullify on-delete
    indexed_table.rs               -- simple, compound, filtered indexes; IntoQuery API
    modify_unchecked.rs            -- single/range/all modify_unchecked variants + debug assertions
    nonpk_auto_increment.rs        -- non-PK #[auto_increment] with compound PKs, indexes
    nonpk_auto_increment_serde.rs  -- non-PK auto-increment serde, missing-counter fallback
    nonpk_auto_database.rs         -- non-PK auto-increment Database-level FKs, cascades, serde
    parent_pk.rs                   -- parent PK as child FK for 1:1 relations
    parent_pk_serde.rs             -- parent PK serde roundtrip
    query_opts.rs                  -- QueryOpts ordering/offset + modify_each_by_*
    serde.rs                       -- serde roundtrip (feature-gated)
    unique_index.rs                -- unique index enforcement on insert/update/upsert
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
- `upsert_no_fk(row) -> Result<(), Error>` -- insert-or-update; can fail
  with `DuplicateIndex` if a unique constraint is violated
- `remove_no_fk(&PK) -> Result<Row, Error>` -- returns removed row

**Closure-based mutation** (see [Closure-based Mutation](#closure-based-mutation)):
- `modify_unchecked(&PK, FnOnce(&mut Row)) -> Result<(), Error>`
- `modify_unchecked_range(range, FnMut(&PK, &mut Row)) -> usize`
- `modify_unchecked_all(FnMut(&PK, &mut Row)) -> usize`
- `modify_each_by_{index}(query..., opts, FnMut(&PK, &mut Row)) -> usize`

**Index query methods** (per index): `by_{name}`, `iter_by_{name}`,
`count_by_{name}` -- see [Index System](#index-system). All accept a final
`QueryOpts` parameter for ordering and offset -- see
[Query Options](#query-options-queryopts).

**Other:** `new()` (empty table), `rebuild_indexes()` (used by deserialization),
`Default` impl.

**Field attributes:**
- `#[primary_key]` -- exactly one required for single-column PKs. Optionally
  `#[primary_key(auto_increment)]` for auto-generated keys (see
  [Auto-Increment Primary Keys](#auto-increment-primary-keys)).
- `#[auto_increment]` -- at most one non-PK field. Auto-assigns sequential
  values via a per-table counter. See [Non-PK Auto-Increment Fields](#non-pk-auto-increment-fields).
- `#[indexed]` -- zero or more, creates a simple single-field index
- `#[indexed(unique)]` -- unique index that rejects duplicate values on
  insert/update (see [Unique Indexes](#unique-indexes))

**Struct attributes:**
- `#[primary_key("field1", "field2")]` -- compound (multi-column) primary key.
  The key type becomes a tuple (e.g., `(CreatureId, TraitKind)`). Requires at
  least 2 fields. Incompatible with `auto_increment`. See
  [Compound Primary Keys](#compound-primary-keys).
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
- `remove_{singular}(&PK) -> Result<(), Error>` -- restrict/cascade/nullify
  on-delete check
- `modify_unchecked_{singular}(&PK, FnOnce(&mut Row)) -> Result<(), Error>`
  -- delegates to table's `modify_unchecked`
- `modify_unchecked_range_{singular}(range, FnMut(&PK, &mut Row)) -> usize`
  -- delegates to table's `modify_unchecked_range`
- `modify_unchecked_all_{singular}(FnMut(&PK, &mut Row)) -> usize`
  -- delegates to table's `modify_unchecked_all`

**FK syntax:**
- `fks(field_name = "target_table")` -- bare FK, field type is `T`
- `fks(field_name? = "target_table")` -- optional FK, field type is `Option<T>`
- Multiple FKs: `fks(source = "creatures", target = "creatures")`

**On-delete semantics:**
- `on_delete restrict` (default) -- block deletion if referencing rows exist
- `on_delete cascade` -- automatically delete dependent rows
- `on_delete nullify` -- set the FK field to `None` (requires `Option<T>` FK)

Syntax: `fks(assignee? = "creatures" on_delete nullify)`.

The `?` suffix on the field name signals that the field is `Option<T>`. For
restrict-on-delete checks, optional FK fields are queried by wrapping the
target PK in `Some(...)`.

**Cycle detection:** Cascade chains are analyzed at compile time. If table A
cascades to B and B cascades to A, the derive macro emits a compile-time error.

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

Each method accepts `impl IntoQuery<Species>` and a `QueryOpts` parameter:
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

Each method takes one `impl IntoQuery<T>` parameter per field, plus a final
`QueryOpts`:

```rust
// Exact match on both fields, ascending
table.by_species_hunger(&Species::Elf, &30, QueryOpts::ASC)

// Prefix query: exact first, all second
table.by_species_hunger(&Species::Elf, MatchAll, QueryOpts::ASC)

// Range on first, exact on second
table.by_species_hunger(Species::Elf..Species::Capybara, &30, QueryOpts::ASC)

// All rows (useful for filtered indexes)
table.by_species_hunger(MatchAll, MatchAll, QueryOpts::ASC)
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

### Unique Indexes

Created by `#[indexed(unique)]` on a field:

```rust
#[indexed(unique)]
name: String,
```

Unique indexes use the same BTreeSet storage as regular indexes, but enforce
a uniqueness constraint on insert and update. If a duplicate value is found,
the operation returns `Error::DuplicateIndex` with a message identifying the
conflicting value.

**Enforcement points:**
- `insert_no_fk` / `insert_*` -- checks all unique indexes before inserting
- `update_no_fk` / `update_*` -- checks all unique indexes before updating
  (allows the row being updated to keep its own value)
- `upsert_no_fk` / `upsert_*` -- checks all unique indexes (allows existing
  row to keep its value on update path)

Unique indexes are compatible with `QueryOpts`, `modify_unchecked`, and all
other table features.

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

## Query Options (QueryOpts)

All index query methods (`by_*`, `iter_by_*`, `count_by_*`) and
`modify_each_by_*` accept a final `QueryOpts` parameter that controls result
ordering and offset.

```rust
pub enum QueryOrder { Asc, Desc }

pub struct QueryOpts {
    pub order: QueryOrder,
    pub offset: usize,
}
```

**Constants and constructors:**
- `QueryOpts::ASC` -- ascending, no offset (the most common case)
- `QueryOpts::DESC` -- descending, no offset
- `QueryOpts::desc()` -- same as `DESC`
- `QueryOpts::offset(n)` -- ascending with offset
- `QueryOpts::desc().with_offset(n)` -- descending with offset

**Ordering:** `Desc` reverses the BTreeSet's natural iteration using `.rev()`.
For compound indexes, this reverses the full tuple order (all fields flip
together -- there is no per-field ordering control).

**Offset:** Skips the first N matching rows via `.skip()`. Applied after
ordering. Useful for pagination-like access patterns.

**Examples:**

```rust
// Get all elves, most hungry first (descending hunger)
table.by_species(&Species::Elf, QueryOpts::DESC)

// Skip the first 5 results
table.by_species(&Species::Elf, QueryOpts::offset(5))

// Descending with offset
table.by_species(&Species::Elf, QueryOpts::desc().with_offset(10))

// count_by respects offset (counts remaining after skip)
table.count_by_species(&Species::Elf, QueryOpts::offset(3))
```

## Closure-based Mutation

Tabulosity provides three families of closure-based mutation methods that
modify rows in place without the clone-modify-update pattern.

### modify_unchecked (Single Row)

Mutates a single row by primary key using a `FnOnce(&mut Row)` closure.
**Bypasses index maintenance** -- the caller is responsible for not changing
indexed fields or the primary key.

```rust
table.modify_unchecked(&creature_id, |c| {
    c.hunger += 10;  // OK: hunger is not indexed
})?;
```

Returns `Result<(), Error>` -- `NotFound` if the PK doesn't exist.

The "unchecked" name signals that index consistency is the caller's
responsibility. The method is safe to use when the closure only modifies
non-indexed, non-PK fields.

### modify_unchecked_range / modify_unchecked_all (Batch)

Batch variants that iterate multiple rows:

```rust
// Mutate rows in a PK range
let count = table.modify_unchecked_range(id_5..id_10, |pk, row| {
    row.hunger += 1;
});

// Mutate all rows
let count = table.modify_unchecked_all(|pk, row| {
    row.hunger += 1;
});
```

Both use `FnMut(&PK, &mut Row)` and return the count of rows visited. The
range variant accepts any `impl RangeBounds<PK>` and uses
`BTreeMap::range_mut` internally. The all variant delegates to
`modify_unchecked_range(.., f)` with an unbounded range.

An empty range is not an error -- it returns 0.

### modify_each_by_* (Index-Driven Batch)

For each index on a table, a `modify_each_by_{index}` method is generated.
It queries the index, collects matching PKs, then mutates each row via
`get_mut`. Like `modify_unchecked`, it **bypasses index maintenance**.

```rust
// Modify all creatures of a species
table.modify_each_by_species(&Species::Elf, QueryOpts::ASC, |_pk, creature| {
    creature.hunger += 10;
});

// Compound index query with descending order
table.modify_each_by_species_hunger(
    &Species::Elf, MatchAll, QueryOpts::DESC, |_pk, creature| {
        creature.rest -= 5;
    },
);
```

Each method takes the same query parameters as the corresponding `by_*` method,
plus a `QueryOpts` and a `FnMut(&PK, &mut Row)` closure. Returns the count of
rows modified.

The PKs are collected into a `Vec` before mutation begins, which breaks the
borrow conflict between the immutable index reference and the mutable row
reference. This means the closure cannot observe index changes from earlier
iterations (not that it should -- indexes aren't maintained).

### Debug Assertions

In debug builds (`#[cfg(debug_assertions)]`), all closure-based mutation
methods snapshot the primary key and all indexed fields before calling the
closure, then assert they are unchanged afterward. This catches accidental
modification of indexed fields during development.

```
thread 'test' panicked at 'modify_unchecked: indexed field `species` was
changed (from Elf to Human); use update() instead'
```

The assertions are compiled out in release builds for zero overhead.

For batch variants (`modify_unchecked_range`, `modify_unchecked_all`,
`modify_each_by_*`), snapshots are taken per-row inside the loop -- not as a
bulk snapshot before iteration. This keeps the per-row overhead identical to
calling `modify_unchecked` N times.

## Auto-Increment Primary Keys

Tables can use `#[primary_key(auto_increment)]` to generate primary keys
automatically:

```rust
#[derive(Table, Clone, Debug)]
struct LogEntry {
    #[primary_key(auto_increment)]
    id: u64,
    message: String,
}
```

This adds an `insert_auto_no_fk` method (or `insert_{singular}_auto` at the
database level) that takes a closure receiving the auto-assigned PK and
returning the row to insert:

```rust
let id = table.insert_auto_no_fk(|pk| LogEntry { id: pk, message: "hello".into() })?;
// id == 0 (first insert)
let id = table.insert_auto_no_fk(|pk| LogEntry { id: pk, message: "world".into() })?;
// id == 1 (second insert)
```

**Counter behavior:**
- Starts at `AutoIncrementable::first()` (0 for all integer types)
- Advances to `successor()` after each insert
- Never reuses IDs -- deleting a row does not reclaim its ID
- Panics on overflow (e.g., `u8` after 256 inserts)

**Serde:** The auto-increment counter is serialized and deserialized alongside
the table data, so it survives save/load roundtrips. On deserialization, the
counter is set to the maximum of the serialized counter and one past the
highest PK in the loaded data -- this prevents ID reuse even if the serialized
counter was stale.

**Required trait:** The PK type must implement `AutoIncrementable`. Blanket
impls exist for all integer types. Custom newtypes need a manual impl or a
newtype over an integer with delegation.

## Compound Primary Keys

Tables can use a struct-level `#[primary_key("field1", "field2")]` attribute to
declare a compound (multi-column) primary key:

```rust
#[derive(Table, Clone, Debug)]
#[primary_key("creature_id", "trait_kind")]
struct CreatureTrait {
    #[indexed]  // needed for FK cascade lookups
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i32,
}
```

The key type becomes a tuple -- in this case `(CreatureId, TraitKind)`. All
table methods (`get`, `contains`, `remove`, `modify_unchecked`, etc.) accept
the tuple key:

```rust
table.get(&(CreatureId(1), TraitKind(10)));
table.modify_unchecked(&(CreatureId(1), TraitKind(10)), |row| row.value += 1);
```

**Key behaviors:**
- `TableMeta::Key` is the tuple type (e.g., `(CreatureId, TraitKind)`)
- `pk_val()` is generated on the row struct (returns owned tuple).
  Single-column PKs also get `pk_ref()` for backward compatibility, but
  compound PKs do not (there is no single field to reference).
- Secondary indexes flatten the compound PK into their BTreeSet tuples:
  e.g., `#[indexed] value` on the above table produces
  `BTreeSet<(i32, CreatureId, TraitKind)>`.
- Rows are ordered lexicographically by the tuple key.

**FK columns in compound PKs:** Zero, one, or multiple columns of a compound
PK can be foreign keys. Use `#[indexed]` on FK columns to enable
cascade/restrict queries, just as with single-column PKs:

```rust
#[table(singular = "creature_trait", fks(creature_id = "creatures" on_delete cascade))]
pub creature_traits: CreatureTraitTable,
```

**Constraints:**
- Requires at least 2 fields.
- Incompatible with `#[primary_key(auto_increment)]` on PK fields (use
  `#[auto_increment]` on a non-PK field instead — see below).
- Field names in the attribute must not be duplicated.
- Compound PK fields cannot appear in struct-level `#[index(fields(...))]`
  (they are automatically appended to every index tuple).

## Non-PK Auto-Increment Fields

A field-level `#[auto_increment]` attribute can be placed on any non-primary-key
field to get auto-assigned sequential values. This is useful for child tables
with compound PKs that need a globally unique tiebreaker:

```rust
#[derive(Table, Clone, Debug)]
#[primary_key("creature_id", "seq")]
struct Thought {
    pub creature_id: CreatureId,
    #[auto_increment]
    pub seq: u64,
    pub kind: String,
    pub tick: u64,
}
```

The table gets an `insert_auto_no_fk` method that takes a closure receiving the
auto-assigned value:

```rust
let pk = table.insert_auto_no_fk(|seq| Thought {
    creature_id: CreatureId(1),
    seq,
    kind: "happy".into(),
    tick: 100,
})?;
// pk == (CreatureId(1), 0)
```

**Counter behavior:** Same as auto-increment PKs — starts at 0, monotonically
increases, never reuses values, bumped by both manual and auto inserts.

**Serde:** Serialized as `{"next_<field>": N, "rows": [...]}`. On deserialize,
if the counter is missing (e.g., old save where the field was previously an
auto-PK), it is computed as `max(field) + 1` across loaded rows. The counter
is defensively set to `max(deserialized, computed)`.

**Constraints:**
- At most one `#[auto_increment]` field per table.
- Cannot be on a `#[primary_key]` field (use `#[primary_key(auto_increment)]`).
- Cannot coexist with `#[primary_key(auto_increment)]` on the same table.
- The field type must implement `AutoIncrementable`.

**Database derive:** Tables with non-PK auto-increment use `nonpk_auto` in the
`#[table(...)]` attribute to enable correct serde format at the database level:

```rust
#[table(singular = "thought", nonpk_auto, fks(creature_id = "creatures" on_delete cascade))]
pub thoughts: ThoughtTable,
```

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
    DuplicateIndex { table: &'static str, index: &'static str, key: String },
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

### Index-Maintaining Closure Update

`db.update_creature_with(&id, |c| c.hunger += 10)` -- like `modify_unchecked`,
but **maintains indexes** by comparing old and new indexed field values after
the closure runs and updating the index BTreeSets accordingly. Medium
complexity. No tracker entry yet.

The `modify_unchecked` variants fill the gap for non-indexed fields, but
modifying indexed fields still requires the clone-modify-update pattern. This
is the single highest-impact missing feature for sim integration.

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

### Schema Versioning

**Tracker:** F-tab-schema-ver

Schema versioning fundamentals: version number on Database (included in serialized
output, checked on deserialization), missing tables deserialize as empty instead of
erroring, `#[serde(default)]` convention for new fields. These changes make additive
schema changes work without any migration code.

### Schema Evolution: Custom Migrations

**Tracker:** F-tab-schema-evol · **Draft:** `docs/drafts/schema_migrations.md`

Two tiers of custom migration code for breaking schema changes: (1) typed
post-deserialize migrations on current Rust structs (for simple transforms), and
(2) low-level migrations on a format-agnostic SchemaSnapshot (for structural
changes like table renames, merges, splits). The SchemaSnapshot path is slower and
only used when a migration requires it. High complexity — defer until closer to
beta.

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
  approach (see [Index-Maintaining Closure Update](#index-maintaining-closure-update))
  is safer.

- **Bulk operations.** `insert_batch`, `remove_where`, etc. for efficiency
  when making many changes at once (e.g., ticking all creatures). Currently
  each mutation is individual.

- **Row type `row = "..."` attribute.** The current convention of stripping
  `"Table"` from the type name to find the row type is fragile. An explicit
  `#[table(row = "Creature")]` attribute would make the association clear
  and produce better error messages. See
  [Row Type Name Convention](#row-type-name-convention).

- **`from_value` deserialization entrypoint.** A non-serde pathway for
  database deserialization that returns typed `DeserializeError` instead of
  wrapping it in a serde error string. See
  [Serde DeserializeError Wrapping](#serde-deserializeerror-wrapping).
