# Tabulosity — In-Memory Relational Store (Design Draft v2)

A lightweight, typed, in-memory relational database library for Rust. Designed
for game simulations and other domains where you want relational integrity
(primary keys, indexes, foreign keys) without the weight of SQL, an external
database engine, or ORM impedance mismatch.

Changes from v1: crate renamed to `tabulosity`; read/write split (reads through
table structs, writes through Database); open questions resolved (FK validation
on update goes through Database, duplicate PK returns error with separate
`upsert`, dropped `#[table(key = "...")]`); iterator semantics clarified.

## Goals

- **Typed tables.** Each table stores rows of a single Rust struct. The struct's
  fields are the columns; one field is the primary key.
- **Automatic indexing.** Annotated fields get secondary indexes that stay in
  sync with mutations. No manual bookkeeping.
- **Foreign key integrity.** Cross-table references are declared and enforced.
  Deleting a row that is still referenced fails (restrict semantics).
- **Deterministic.** All internal data structures use `BTreeMap`/`BTreeSet` —
  no hash-based containers. Iteration order is always deterministic.
- **Serializable.** If row structs implement `Serialize`/`Deserialize`, the
  entire database serializes as a collection of row vectors. Indexes are rebuilt
  on deserialization; FK constraints are verified.
- **General-purpose.** Not tied to any particular game or domain.
- **No external PRNG or runtime dependencies.** The crate depends only on
  `serde` (optional, behind a feature flag) and proc-macro utilities (`syn`,
  `quote`, `proc-macro2`).

## Non-Goals (v1)

- SQL parsing or query planning.
- Transactions, rollback, or MVCC.
- Thread safety (designed for single-threaded use).
- Compound indexes (see Future Work).
- Joins (see Future Work).
- Cascade or nullify on delete (restrict only in v1).

## Crate Structure

```
tabulosity/
├── tabulosity/              # Main library crate
│   └── src/
│       ├── lib.rs           # Public API, re-exports
│       ├── table.rs         # Table<K, V> core data structure
│       └── error.rs         # Error types
├── tabulosity_derive/       # Proc macro crate (must be separate)
│   └── src/
│       └── lib.rs           # #[derive(Table)] and #[derive(Database)]
└── Cargo.toml               # Workspace (or could be two crates in a larger workspace)
```

The proc macro crate (`tabulosity_derive`) is a compile-time dependency only.
Users interact with `tabulosity` and its derive macros.

## Read/Write Split

The central design decision in v2 is a clean split between reads and writes:

- **Reads go through the table structs directly.** `db.creatures.get(id)`,
  `db.creatures.iter_all()`, `db.creatures.by_species(Species::Elf)`, etc.
  These borrow only the one table being queried, leaving other tables free for
  concurrent (non-overlapping) borrows.
- **Writes go through the Database struct.** `db.insert_creature(c)`,
  `db.update_task(t)`, `db.remove_creature(id)`, `db.upsert_task(t)`, etc.
  These take `&mut self` on the Database, which lets them perform FK validation
  across tables before committing the mutation.

The table structs are public fields on the Database (for reads), but their
internal mutation methods are `pub(crate)` — not part of the public API. All
writes must go through the Database to ensure cross-table integrity.

**Why this split?** Reads are the hot path in most simulations — you read far
more than you write. Routing reads through the table structs means the borrow
checker only locks the specific table being queried. If reads went through
`&self` on the Database, they'd be no worse off (shared borrows compose), but
the API would be unnecessarily verbose (`db.get_creature(id)` instead of
`db.creatures.get(id)`). Writes need the Database context for FK validation, so
routing them through `&mut self` on the Database is both correct and natural.

### Clone vs. Iterator Reads

The table API provides two flavors of reads:

- **Clone-based** (`get`, `all`, `by_*`): return owned values (`Option<T>`,
  `Vec<T>`). These clone data out of the table, so the borrow is released
  immediately. Safe to interleave with writes.
- **Iterator-based** (`get_ref`, `iter_all`, `iter_by_*`): return immutable
  references (`Option<&T>`, `impl Iterator<Item = &T>`). These borrow the
  table for the lifetime of the reference/iterator, so the table (and therefore
  the Database) cannot be mutated while the borrow is held.

Clone-based reads are the primary API — they're simpler to use and don't
constrain the caller's borrow patterns. Iterator-based reads are a performance
optimization for read-only scans where you don't need to interleave writes and
want to avoid cloning every row.

**Safety property:** Iterators return `&T` (immutable references). Rust enforces
at compile time that you cannot modify data through `&T`, and the borrow checker
prevents mutating the table while any `&T` borrow is alive. This means iterator-
based reads are guaranteed to see a consistent snapshot of the table — no
concurrent mutation is possible. This is enforced by the compiler, not by runtime
checks.

## User-Facing API

### Defining Row Structs

```rust
use tabulosity::Table;

#[derive(Table, Clone, Debug)]
pub struct Creature {
    #[primary_key(auto)]
    pub id: CreatureId,

    pub name: String,

    #[indexed]
    pub species: Species,

    #[indexed]
    pub hunger: u32,
}

#[derive(Table, Clone, Debug)]
pub struct Task {
    #[primary_key(auto)]
    pub id: TaskId,

    #[indexed]
    #[foreign_key]
    pub assignee: CreatureId,

    #[indexed]
    pub target: VoxelCoord,

    pub priority: u8,
}
```

**Attributes:**

- `#[primary_key(auto)]` — marks the PK field. `auto` means the table assigns
  the key on insert if the caller provides a default/zero value. Without `auto`,
  the caller must supply the key.
- `#[indexed]` — creates a secondary index on this field. The field type must
  implement `Ord + Clone`.
- `#[foreign_key]` — marks this field as a foreign key. The `Database` derive
  resolves which table it references by matching the field's type to a table's
  primary key type.

**Required trait bounds on row structs:**

- `Clone` — for clone-based reads (`get`, `all`, `by_*`).
- The primary key type must implement `Ord + Clone + Default` (for auto-key
  detection) + `Bounded` (a trait providing `MIN` and `MAX` values, needed for
  range scans on the composite index).

### Defining the Database Schema

```rust
use tabulosity::Database;

#[derive(Database)]
pub struct MyDb {
    pub creatures: CreatureTable,
    pub tasks: TaskTable,
}
```

`CreatureTable` and `TaskTable` are generated by `#[derive(Table)]` on the
row structs. The `Database` derive:

1. Finds all table fields.
2. Resolves foreign keys: `Task::assignee` is type `CreatureId`, which matches
   `CreatureTable`'s PK type — the FK points from `tasks.assignee` to
   `creatures`.
3. Generates `new()`, integrity-aware write methods (`insert_*`, `update_*`,
   `remove_*`, `upsert_*`), and deserialization with FK validation.

### Writes (Through Database)

All mutations go through the Database struct, which takes `&mut self`:

```rust
let mut db = MyDb::new();

// Insert with auto-generated key — returns the assigned PK
let id = db.insert_creature(Creature {
    id: CreatureId::default(),   // sentinel; table assigns the real key
    name: "Aelindra".into(),
    species: Species::Elf,
    hunger: 0,
})?;

// Insert with explicit key
db.insert_creature(Creature {
    id: CreatureId(42),
    name: "Thorn".into(),
    species: Species::Capybara,
    hunger: 10,
})?;

// Update — PK extracted from struct, indexes updated, FK fields validated
let mut creature = db.creatures.get(id).unwrap();
creature.hunger += 10;
db.update_creature(creature)?;

// Upsert — inserts if PK doesn't exist, updates if it does
db.upsert_task(Task {
    id: TaskId(99),
    assignee: creature_id,
    target: some_coord,
    priority: 5,
})?;

// Delete with FK checks
db.remove_creature(creature_id)?;
// → checks tasks.count_by_assignee(creature_id) > 0
// → if so, returns Err listing the referencing table and count
// → if clear, removes the row and returns it
```

**Insert behavior:** Returns the assigned primary key on success. If a row with
the same PK already exists, returns `Err(DuplicateKey { ... })`. Use `upsert_*`
if you want insert-or-update semantics.

**Update behavior:** Extracts the PK from the struct, looks up the existing row,
compares each indexed field between old and new, updates only the indexes whose
values changed, then replaces the stored row. Returns an error if the PK doesn't
exist. If a foreign key field changed, the new value is validated against the
referenced table — this is why updates must go through the Database.

**Delete behavior:** The `Database` derive generates `remove_{singular}()`
methods that check all FK indexes pointing at that table's PK type before
allowing deletion (restrict semantics). The error includes enough information
for the caller to understand what's still referencing the row:

```rust
Err(FkViolation {
    table: "creatures",
    key: "CreatureId(7)",
    referenced_by: vec![
        ("tasks", "assignee", 3),  // 3 tasks reference this creature
    ],
})
```

**Write method naming convention:**

| Method                    | Behavior                                           |
|---------------------------|----------------------------------------------------|
| `insert_{row}(r)`        | Insert new row. Error if PK exists.                |
| `update_{row}(r)`        | Update existing row. Error if PK missing.          |
| `upsert_{row}(r)`        | Insert or update. FK validated either way.         |
| `remove_{row}(pk)`       | Delete row. Error if FK references remain.         |

### Reads (Through Table Structs)

Reads go directly through the table fields on the Database. These take only
`&self` on the individual table, not the Database:

```rust
// Primary key lookup — clone
let creature: Option<Creature> = db.creatures.get(creature_id);

// Primary key lookup — reference
let creature: Option<&Creature> = db.creatures.get_ref(creature_id);

// All rows
let all: Vec<Creature> = db.creatures.all();
for creature in db.creatures.iter_all() { /* &Creature */ }

// Secondary index — equality
let elves: Vec<Creature> = db.creatures.by_species(Species::Elf);
for elf in db.creatures.iter_by_species(Species::Elf) { /* &Creature */ }

// Secondary index — range
let hungry: Vec<Creature> = db.creatures.by_hunger_range(50..);
for c in db.creatures.iter_by_hunger_range(..=20) { /* &Creature */ }

// Count (cheap — no row lookups, just index scan)
let n: usize = db.creatures.count_by_species(Species::Elf);
```

**Read method naming convention:**

| Pattern                      | Returns               | Notes                          |
|------------------------------|-----------------------|--------------------------------|
| `get(pk)`                    | `Option<T>`           | Clone by primary key           |
| `get_ref(pk)`                | `Option<&T>`          | Borrow by primary key          |
| `all()`                      | `Vec<T>`              | Clone all rows                 |
| `iter_all()`                 | `impl Iterator<&T>`   | Iterate all rows               |
| `by_field(val)`              | `Vec<T>`              | Clone matching rows            |
| `iter_by_field(val)`         | `impl Iterator<&T>`   | Iterate matching rows          |
| `by_field_range(r)`          | `Vec<T>`              | Clone rows in range            |
| `iter_by_field_range(r)`     | `impl Iterator<&T>`   | Iterate rows in range          |
| `count_by_field(val)`        | `usize`               | Count matching rows            |
| `count_all()`                | `usize`               | Total row count                |

**All iterator methods return `&T` (immutable references).** The Rust borrow
checker enforces that the table cannot be mutated while any iterator or reference
is alive. Combined with the read/write split (mutations require `&mut self` on
the Database), this guarantees that iterator-based reads always see a consistent
snapshot — no data races, no concurrent mutation, enforced at compile time.

## Internal Data Structures

### Row Storage

```
rows: BTreeMap<PK, RowStruct>
```

The primary store. Keyed by the primary key, values are the full row structs.
This is the source of truth; indexes are derived from it.

### Auto-Key Counter

```
next_key: u64
```

Monotonically increasing counter. On `insert` with `auto` key, the table
assigns `PK::from(next_key)` and increments. The PK type needs a
`From<u64>` impl (or a similar trait) for this.

On deserialization, `next_key` is set to `max(existing PKs) + 1`.

### Secondary Indexes

```
idx_{field}: BTreeSet<(FieldType, PK)>
```

One `BTreeSet` per `#[indexed]` field. The tuple `(field_value, primary_key)`
is sorted lexicographically — first by field value, then by PK as tiebreaker.

**Equality query** — range scan over a single field value:

```rust
let start = (value.clone(), PK::MIN);
let end = (value.clone(), PK::MAX);
self.idx_field.range(start..=end)
    .map(|(_, pk)| self.rows.get(pk).unwrap())
```

**Range query** — range scan over a range of field values:

```rust
// by_hunger_range(50..)
let start = (50, PK::MIN);
self.idx_hunger.range((Included(start), Unbounded))
    .map(|(_, pk)| self.rows.get(pk).unwrap())
```

Translating `RangeBounds<FieldType>` to `RangeBounds<(FieldType, PK)>`:

| Input start bound        | Composite start bound          |
|--------------------------|--------------------------------|
| `Included(v)`            | `Included((v, PK::MIN))`       |
| `Excluded(v)`            | `Excluded((v, PK::MAX))`       |
| `Unbounded`              | `Unbounded`                    |

| Input end bound          | Composite end bound            |
|--------------------------|--------------------------------|
| `Included(v)`            | `Included((v, PK::MAX))`       |
| `Excluded(v)`            | `Excluded((v, PK::MIN))`       |
| `Unbounded`              | `Unbounded`                    |

**Insert maintenance:**

```rust
// For each indexed field:
self.idx_field.insert((new_row.field.clone(), pk.clone()));
```

**Update maintenance:**

```rust
// For each indexed field, compare old and new:
if old_row.field != new_row.field {
    self.idx_field.remove(&(old_row.field.clone(), pk.clone()));
    self.idx_field.insert((new_row.field.clone(), pk.clone()));
}
```

**Delete maintenance:**

```rust
// For each indexed field:
self.idx_field.remove(&(row.field.clone(), pk.clone()));
```

### Foreign Key Index

No additional data structure needed. FK checks reuse the secondary index on
the FK field. `#[foreign_key]` implies `#[indexed]` — if the field isn't
already annotated `#[indexed]`, the derive adds the index automatically.

To check whether a PK is still referenced: `count_by_{fk_field}(pk) > 0`.

## Serialization

Behind a `serde` feature flag. When enabled:

**Serialize:** Each table serializes as a `Vec<RowStruct>` (just the rows, in
PK order). Indexes are not serialized — they're derived data. The auto-key
counter is serialized.

**Deserialize:**

1. Deserialize each table's row vec.
2. Rebuild `rows` BTreeMap from the vec.
3. Rebuild all secondary indexes by scanning every row.
4. Set `next_key` to `max(existing PKs) + 1`.
5. Validate all FK constraints: for every FK field in every row, verify the
   referenced PK exists in the target table. Collect all violations.
6. If any FK violations found, return an error listing them all (don't fail
   on the first one — report all violations so they can be fixed in one pass).

This means saved data is just rows — compact, human-readable (in JSON), and
indexes are always consistent with the data since they're rebuilt fresh.

## Proc Macro Implementation Notes

### `#[derive(Table)]`

Generates a companion struct `{RowName}Table` with:

- `rows: BTreeMap<PK, RowStruct>` (the primary store)
- `next_key: u64` (if auto-key)
- `idx_{field}: BTreeSet<(FieldType, PK)>` for each `#[indexed]` field
- All read methods (`get`, `get_ref`, `all`, `iter_all`, `by_*`, `iter_by_*`,
  `count_by_*`, range variants) — these are `pub`
- Internal mutation methods (`insert`, `update`, `upsert`, `remove`) with
  index maintenance — these are `pub(crate)`, not exposed to users
- `rebuild_indexes()` for deserialization

The derive macro reads the struct definition, finds annotated fields, and
generates the companion type with all the boilerplate.

### `#[derive(Database)]`

Reads the schema struct, identifies all `*Table` fields, and:

1. **Resolves FKs.** For each table with `#[foreign_key]` fields, matches the
   FK field type to another table's PK type. Ambiguity (two tables with the
   same PK type) is a compile-time error — the user would need a disambiguation
   attribute.
2. **Generates `new()`.** Creates all tables.
3. **Generates write methods.** For each table, generates `insert_*`,
   `update_*`, `upsert_*`, and `remove_*` methods on the Database. Insert and
   upsert validate that FK fields reference existing rows in the target table.
   Update validates any changed FK fields. Remove checks that no other table's
   FK index references the row being deleted (restrict semantics).
4. **Generates deserialization** that rebuilds indexes and validates FKs.

### Bounded Trait

The PK type needs MIN and MAX values for range scans. Options:

- A `Bounded` trait in the `tabulosity` crate (simple, explicit).
- Blanket impls for common types (`u8`, `u16`, `u32`, `u64`, etc.).
- A derive macro for newtype wrappers: `#[derive(Bounded)]` on
  `struct CreatureId(u64)` generates `MIN = CreatureId(0)`,
  `MAX = CreatureId(u64::MAX)`.

Indexed field types also need `Bounded` for range queries. Same approach.

## Future Work

- **Compound indexes.** `BTreeSet<(Field1, Field2, ..., PK)>` with prefix
  query support. Attribute syntax for grouping fields:
  `#[compound_index(name = "...", fields = ["assignee", "priority"])]`.
  Useful for "all tasks for creature X, ordered by priority" in a single
  index scan.
- **Joins.** Query API that traverses FK relationships:
  `db.tasks.join_assignee()` -> iterator of `(&Task, &Creature)`. Ergonomic
  but complex to generate.
- **Cascade / nullify on delete.** `#[foreign_key(on_delete = "cascade")]`
  and `#[foreign_key(on_delete = "nullify")]` (field becomes `Option<PK>`).
- **Unique indexes.** `#[indexed(unique)]` — enforced on insert/update.
- **Filtered indexes / partial indexes.** Index only rows matching a predicate.
- **Change tracking.** Tables emit a list of changes (insert/update/delete)
  per tick, useful for event-driven rendering.
