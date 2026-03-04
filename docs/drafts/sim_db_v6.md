# Tabulosity — In-Memory Relational Store (Design Draft v6)

A lightweight, typed, in-memory relational database library for Rust. Designed
for game simulations and other domains where you want relational integrity
(primary keys, indexes, foreign keys) without the weight of SQL, an external
database engine, or ORM impedance mismatch.

Changes from v5: renamed `_unchecked` methods to `_no_fk` (less misleading than
the `unsafe`-adjacent `_unchecked` naming); added `FkTargetNotFound` error
variant (insert/update FK validation now uses a different error than
restrict-on-delete `FkViolation`); simplified cross-derive FK information flow
(dropped `check_fk_*` and `count_fk_*` intermediate methods — the Database
derive now generates FK validation directly using `fks(...)` metadata on the
`#[table]` attribute); added `DeserializeError` type for bulk error reporting
on load; `Error` and `DeserializeError` implement `Display`, `std::error::Error`,
and `PartialEq`; generated table struct internals are private, `_no_fk` methods
and `rebuild_indexes()` are `#[doc(hidden)] pub`, read methods are `pub`;
generated code uses `.clone()` consistently (only `Clone` required on PK types,
not `Copy`).

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
- Auto-generated primary keys (see Future Work).
- Compound indexes (see Future Work).
- Joins (see Future Work).
- Cascade or nullify on delete (restrict only in v1).

## Crate Structure

```
tabulosity/
├── tabulosity/              # Main library crate
│   └── src/
│       ├── lib.rs           # Public API, re-exports, Bounded trait
│       ├── table.rs         # Table<K, V> core data structure
│       └── error.rs         # tabulosity::Error enum, DeserializeError
├── tabulosity_derive/       # Proc macro crate (must be separate)
│   └── src/
│       └── lib.rs           # #[derive(Table)], #[derive(Database)], #[derive(Bounded)]
└── Cargo.toml               # Workspace
```

The proc macro crate (`tabulosity_derive`) is a compile-time dependency only.
Users interact with `tabulosity` and its derive macros.

## Error Types

All fallible write operations return `Result<T, tabulosity::Error>`. A single
enum covers every failure mode:

```rust
/// All errors returned by tabulosity write operations.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Attempted to insert a row with a primary key that already exists.
    DuplicateKey {
        table: &'static str,
        key: String,
    },

    /// Attempted to update or remove a row that does not exist.
    NotFound {
        table: &'static str,
        key: String,
    },

    /// Attempted to insert or update a row, but a foreign key field
    /// references a row that does not exist in the target table.
    FkTargetNotFound {
        table: &'static str,            // table being inserted/updated
        field: &'static str,            // the FK field name
        referenced_table: &'static str, // the target table
        key: String,                    // the key that wasn't found
    },

    /// Attempted to remove a row that is still referenced by foreign keys
    /// in other tables (restrict semantics).
    FkViolation {
        table: &'static str,
        key: String,
        /// Each entry: (referencing table name, FK field name, count of references).
        referenced_by: Vec<(&'static str, &'static str, usize)>,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::DuplicateKey { table, key } =>
                write!(f, "duplicate key in {}: {}", table, key),
            Error::NotFound { table, key } =>
                write!(f, "not found in {}: {}", table, key),
            Error::FkTargetNotFound { table, field, referenced_table, key } =>
                write!(f, "FK target not found: {}.{} references {} key {}",
                       table, field, referenced_table, key),
            Error::FkViolation { table, key, referenced_by } => {
                write!(f, "FK violation: {}.{} still referenced by", table, key)?;
                for (ref_table, ref_field, count) in referenced_by {
                    write!(f, " {}.{} ({} rows)", ref_table, ref_field, count)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {}
```

`PartialEq` is derived so that errors can be compared in tests:

```rust
assert_eq!(
    result.unwrap_err(),
    tabulosity::Error::FkTargetNotFound {
        table: "tasks",
        field: "assignee",
        referenced_table: "creatures",
        key: "CreatureId(99)".into(),
    },
);
```

**Which error variants can each write method return?**

| Method             | Possible error variants                |
|--------------------|----------------------------------------|
| `insert_{row}`     | `DuplicateKey`, `FkTargetNotFound`     |
| `update_{row}`     | `NotFound`, `FkTargetNotFound`         |
| `upsert_{row}`     | `FkTargetNotFound`                     |
| `remove_{row}`     | `NotFound`, `FkViolation`              |

`insert` can fail because the PK already exists (`DuplicateKey`) or because a
FK field references a nonexistent row (`FkTargetNotFound`). `update` can fail
because the PK doesn't exist (`NotFound`) or because a FK field references a
nonexistent row (`FkTargetNotFound`). `remove` can fail because the PK doesn't
exist (`NotFound`) or because other rows still reference it (`FkViolation`).
`upsert` can only fail on FK validation (`FkTargetNotFound`), never on PK
existence (it inserts or updates as needed).

This works cleanly with `?` — callers can propagate errors without converting
between different error types:

```rust
fn relocate_creature(db: &mut MyDb, id: &CreatureId, new_pos: VoxelCoord) -> Result<(), tabulosity::Error> {
    let mut creature = db.creatures.get(id).ok_or_else(|| tabulosity::Error::NotFound {
        table: "creatures",
        key: format!("{:?}", id),
    })?;
    creature.position = new_pos;
    db.update_creature(creature)?;
    Ok(())
}
```

### Deserialization Error Type

Deserialization can encounter multiple errors across tables (duplicate PKs,
broken FK references). Rather than failing on the first error, it collects all
of them and returns them together:

```rust
/// Error returned when deserializing a database fails validation.
#[derive(Debug, Clone, PartialEq)]
pub struct DeserializeError {
    /// All errors found during validation. Variants will be `DuplicateKey`
    /// and/or `FkTargetNotFound` — the only errors that can arise from
    /// loading serialized data.
    pub errors: Vec<Error>,
}

impl std::fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "deserialization failed with {} errors:", self.errors.len())?;
        for err in &self.errors {
            write!(f, "\n  - {}", err)?;
        }
        Ok(())
    }
}

impl std::error::Error for DeserializeError {}
```

The Database's deserialization returns `Result<MyDb, DeserializeError>`. The
exact serialization/deserialization mechanism (serde derive vs a custom
`from_rows()` method) is an implementation detail to be figured out during
development. The design specifies the semantics: serialize as row vecs, rebuild
indexes on load, validate FKs, detect duplicate PKs, and report all errors.

## Read/Write Split

The central architectural decision is a clean split between reads and writes:

- **Reads go through the table structs directly.** `db.creatures.get(&id)`,
  `db.creatures.all()`, `db.creatures.by_species(&Species::Elf)`, etc. These are
  `&self` methods on the individual table struct.
- **Writes go through the Database struct.** `db.insert_creature(c)`,
  `db.update_creature(c)`, `db.remove_creature(&id)`, `db.upsert_creature(c)`.
  These take `&mut self` on the Database, which lets them perform FK validation
  across tables before committing the mutation.

The table structs are public fields on the Database (for reads), but their
internal fields (`rows`, `idx_*`) are private, and their direct mutation methods
use the `_no_fk` suffix and are `#[doc(hidden)]` — see the `_no_fk` Methods
section below.

**Why this split?** Reads are the hot path in most simulations — you read far
more than you write. Routing reads through the table structs keeps the API
natural (`db.creatures.get(&id)` rather than `db.get_creature(&id)`). Writes need
the Database context for FK validation, so routing them through `&mut self` on
the Database is both correct and natural.

### Borrow Checker Strategy

The read/write split has a real limitation that must be understood clearly.

**Clone-based reads are the primary API.** Methods like `get`, `all`, `by_*`
return owned data (`Option<T>`, `Vec<T>`). The borrow is released as soon as
the method returns, so you can freely interleave cloned reads with writes:

```rust
// This works — clone releases the borrow before the write.
let creature = db.creatures.get(&id).unwrap();
db.update_creature(Creature { hunger: creature.hunger + 10, ..creature })?;
```

**Iterator/reference-based reads are a performance optimization for read-only
scans.** Methods like `get_ref`, `iter_all`, `iter_by_*` return `&T` references
that borrow the table (and therefore the Database struct that contains it). While
any such borrow is alive, you cannot call write methods — the compiler rejects
it because write methods take `&mut self` on the Database, which conflicts with
the outstanding `&self` borrow through the table field.

```rust
// DOES NOT COMPILE — iter_all borrows db.creatures (and therefore db),
// so db.update_creature cannot take &mut self on db.
for creature in db.creatures.iter_all() {
    if creature.hunger > 50 {
        db.update_creature(Creature { hunger: 0, ..creature.clone() })?;
        //                 ^^^ ERROR: cannot borrow `db` as mutable
    }
}
```

Note: even though reads borrow only the individual table field, write methods
take `&mut self` on the whole Database. Rust's borrow checker does not do
field-level tracking through method signatures — an `&self` borrow on any field
of `db` conflicts with `&mut self` on `db`. This is a fundamental Rust
limitation, not a design flaw.

**The correct pattern** — clone first, then mutate:

```rust
// Collect the data you need, releasing the borrow.
let hungry: Vec<Creature> = db.creatures.by_hunger_range(50..);

// Now mutate freely — no outstanding borrows.
for mut creature in hungry {
    creature.hunger = 0;
    db.update_creature(creature)?;
}
```

This is the idiomatic tabulosity pattern: use clone-based reads for anything
that interleaves with writes, and use iterator-based reads for pure read-only
scans (reporting, rendering, serialization) where you want to avoid cloning
every row.

## User-Facing API

### Defining Row Structs

```rust
use tabulosity::Table;

#[derive(Table, Clone, Debug)]
pub struct Creature {
    #[primary_key]
    pub id: CreatureId,

    pub name: String,

    #[indexed]
    pub species: Species,

    #[indexed]
    pub hunger: u32,
}

#[derive(Table, Clone, Debug)]
pub struct Task {
    #[primary_key]
    pub id: TaskId,

    #[foreign_key(references = "creatures")]
    pub assignee: CreatureId,

    #[indexed]
    pub target: VoxelCoord,

    pub priority: u8,
}
```

**Attributes:**

- `#[primary_key]` — marks the PK field. Exactly one field per struct must have
  this. The caller always provides an explicit key value on insert.
- `#[indexed]` — creates a secondary index on this field. The field type must
  implement `Ord + Clone`.
- `#[foreign_key(references = "table_field")]` — marks this field as a foreign
  key referencing a specific table in the Database struct. The string must match
  a field name on the Database struct (e.g., `"creatures"` matches
  `pub creatures: CreatureTable`). This also implies `#[indexed]` — an index
  is generated automatically even without an explicit `#[indexed]` annotation,
  because the index is needed for restrict-on-delete checks.

**Why explicit target tables?** Proc macros in Rust run before the type checker.
A proc macro processing `#[derive(Table)]` on `Task` sees `assignee: CreatureId`
as raw tokens — it cannot resolve `CreatureId` to a type, look up which table
has that PK type, or do any cross-type reasoning. By having the user write
`references = "creatures"`, the Database derive can generate FK validation code
like `self.creatures.contains(&task.assignee)` directly from the string. If the
types don't match (e.g., the user accidentally writes `references = "tasks"`
for a `CreatureId` field), the regular Rust compiler catches the type mismatch
in the generated code. The proc macro stays simple and the compiler does what
it's good at. See the Cross-Derive Information Flow section below for the full
picture.

**Required trait bounds on row structs:**

- `Clone` — for clone-based reads (`get`, `all`, `by_*`).
- The primary key type must implement `Ord + Clone + Bounded` (`Bounded`
  provides `MIN` and `MAX` values, needed for equality queries on composite
  secondary indexes).
- Indexed field types must implement `Ord + Clone`. They additionally need
  `Bounded` only if range queries are desired — see the Bounded Trait section.

### Primary Key Mutation

Do not change the primary key field on a row struct before calling `update_*`.
The update method extracts the PK from the struct to locate the existing row —
if you change the PK, it will look for a row that doesn't exist and return
`Err(NotFound)`, or worse, overwrite a different row.

If you need to re-key a row (change its primary key), use `remove` followed by
`insert`:

```rust
let old = db.remove_creature(&old_id)?;
db.insert_creature(Creature { id: new_id, ..old })?;
```

This is standard ORM behavior — primary keys are identity, not mutable data.

### Optional Foreign Keys

Foreign key fields can be `Option<FkType>`:

```rust
#[derive(Table, Clone, Debug)]
pub struct Task {
    #[primary_key]
    pub id: TaskId,

    #[foreign_key(references = "creatures")]
    pub assignee: Option<CreatureId>,

    #[indexed]
    pub priority: u8,
}
```

**Behavior:**

- **FK validation on insert/update:** `None` values are skipped — no existence
  check is performed. `Some(id)` values are validated against the referenced
  table as usual.
- **Restrict-on-delete:** When checking whether a creature can be deleted, `None`
  values in the `assignee` index do not count as references. Only `Some(id)`
  entries that match the creature being deleted block the removal.
- **Index entries:** The secondary index stores `(Option<CreatureId>, TaskId)`
  tuples. `None` sorts before all `Some` values (standard `Option` ordering),
  so `by_assignee(&None)` efficiently finds all unassigned tasks.

```rust
let mut db = MyDb::new();

// Insert a task with no assignee.
db.insert_task(Task {
    id: TaskId(1),
    assignee: None,
    priority: 5,
})?;

// Insert a task assigned to a creature.
db.insert_task(Task {
    id: TaskId(2),
    assignee: Some(creature_id),
    priority: 3,
})?;

// Find unassigned tasks.
let unassigned: Vec<Task> = db.tasks.by_assignee(&None);

// Removing the creature checks only Some(creature_id) entries —
// the None entry on TaskId(1) does not block deletion.
db.remove_creature(&creature_id)?;  // Error: TaskId(2) still references it
```

### Defining the Database Schema

```rust
use tabulosity::Database;

#[derive(Database)]
pub struct MyDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "task", fks(assignee = "creatures"))]
    pub tasks: TaskTable,
}
```

**Every field must be a table.** The Database struct contains *only* table fields,
each annotated with `#[table(singular = "...")]`. If a field is missing the
`#[table(...)]` annotation, the Database derive emits a compile error. This
simplifies the derive considerably: `new()` trivially creates empty tables,
serialization covers all fields uniformly, and there is no ambiguity about which
fields participate in FK resolution.

If you need non-table state alongside the database, wrap it in a separate struct:

```rust
struct GameState {
    db: MyDb,
    tick_count: u64,
    config: GameConfig,
}
```

This is a cleaner separation of concerns — the database holds relational data,
and the wrapper struct holds everything else. Reads go through
`state.db.creatures.get(&id)`, writes through `state.db.insert_creature(c)`.

**The `fks(...)` attribute:** Foreign key relationships are declared on the
`#[table]` attribute using the `fks(...)` syntax. Each entry maps a field name
on the row struct to the target table's field name on the Database struct:

```rust
#[table(singular = "task", fks(assignee = "creatures"))]
```

This tells the Database derive: "the `assignee` field on the `Task` row struct
is a foreign key referencing the `creatures` table." For multiple FKs:

```rust
#[table(singular = "task", fks(assignee = "creatures", location = "zones"))]
```

The Database derive uses this information to generate FK validation code
directly — see the Cross-Derive Information Flow section.

`CreatureTable` and `TaskTable` are generated by `#[derive(Table)]` on the
row structs. The `Database` derive:

1. Finds all fields and verifies each has a `#[table(singular = "...")]`
   annotation. Any unannotated field is a compile error.
2. Reads FK declarations from the `fks(...)` attribute on each `#[table]`
   annotation. Uses these to generate FK validation code directly.
3. Generates `new()` — creates all tables (empty). Since every field is a table
   and every table starts empty, this requires no trait bounds or user input.
4. Generates safe write methods (`insert_*`, `update_*`, `upsert_*`,
   `remove_*`) on the Database. These perform FK validation before delegating
   to the table's `_no_fk` methods.
5. Generates deserialization that rebuilds indexes and validates FKs.

**Singularization:** The `singular` attribute provides the name stem for
generated write methods. `#[table(singular = "creature")]` produces
`insert_creature`, `update_creature`, `remove_creature`, `upsert_creature`.
This avoids the proc macro needing to do English pluralization (which is
error-prone for words like "species", "data", "indices", etc.).

### `_no_fk` Methods on Table Structs

The `#[derive(Table)]` macro generates mutation methods directly on the
companion table struct, named with a `_no_fk` suffix:

```rust
// These are generated on CreatureTable:
impl CreatureTable {
    #[doc(hidden)]
    pub fn insert_no_fk(&mut self, row: Creature) -> Result<(), tabulosity::Error> { ... }

    #[doc(hidden)]
    pub fn update_no_fk(&mut self, row: Creature) -> Result<(), tabulosity::Error> { ... }

    #[doc(hidden)]
    pub fn remove_no_fk(&mut self, id: &CreatureId) -> Result<Creature, tabulosity::Error> { ... }
}
```

**Why `_no_fk` instead of `_unchecked`?** The `_unchecked` suffix has strong
`unsafe` connotations in Rust — `String::from_utf8_unchecked`,
`slice::get_unchecked`, and similar are all `unsafe fn` that can cause undefined
behavior if preconditions are violated. Tabulosity's methods are safe Rust that
merely skip FK validation. The `_no_fk` suffix is more descriptive and less
misleading: these methods do all the same work as the Database-level methods
(index maintenance, PK existence checks) except they skip foreign key checks.

These methods are **`#[doc(hidden)]`** — they are technically `pub` (because the
Database derive's generated code in a different module needs to call them), but
they are hidden from rustdoc output and from IDE autocomplete in most editors.
This keeps the public API surface clean: users see only the read methods on
table structs and the safe write methods on the Database.

The `_no_fk` methods still return `Result` for non-FK errors:
`insert_no_fk` returns `Err(DuplicateKey)` if the PK already exists, and
`update_no_fk` / `remove_no_fk` return `Err(NotFound)` if the PK is missing.

**Why `#[doc(hidden)]` instead of `pub(crate)`?** The Database derive generates
code in the user's crate, not inside the tabulosity crate. If `_no_fk`
methods were `pub(crate)` on the tabulosity crate's types, the user's generated
Database code couldn't call them. Making them `pub` but `#[doc(hidden)]`
satisfies both requirements: the generated code compiles, and the methods are
invisible to normal users browsing docs or using autocomplete.

**When to use `_no_fk`:** Prefer the Database-level methods (`insert_creature`,
`update_creature`, `remove_creature`) for normal use — they perform FK validation
and keep your data consistent. The `_no_fk` methods exist for:

- **Bulk loading:** When populating a database from a trusted source (e.g.,
  deserialization), you can insert rows without per-row FK checks and validate
  all FKs in one pass at the end.
- **Internal use by the Database derive:** The generated Database-level methods
  perform FK validation themselves, then delegate to the `_no_fk` methods
  for the actual mutation.
- **Performance-critical paths:** When you can prove FK integrity is maintained
  by other means, `_no_fk` avoids redundant lookups.

### Generated Table Struct Visibility

The table structs generated by `#[derive(Table)]` have carefully controlled
visibility:

- **Internal fields are private.** `rows: BTreeMap<PK, Row>` and
  `idx_*: BTreeSet<(FieldType, PK)>` are not accessible from outside the
  generated struct. Users interact with the table through its methods, not by
  reaching into its internals.
- **Read methods are `pub`.** `get`, `get_ref`, `contains`, `len`, `is_empty`,
  `keys`, `iter_keys`, `all`, `iter_all`, `by_*`, `iter_by_*`, `count_by_*`,
  and range variants are all public.
- **`_no_fk` mutation methods are `#[doc(hidden)] pub`.** Needed by
  Database-derive generated code, but invisible to normal users.
- **`rebuild_indexes()` is `#[doc(hidden)] pub`.** Used during deserialization
  to reconstruct indexes from the row data.

This gives a clean public API surface: users see read methods on tables and
write methods on the Database, with no internal implementation details leaking.

### Writes (Through Database) — The Safe API

All normal mutations go through the Database struct, which takes `&mut self`:

```rust
let mut db = MyDb::new();

// Insert — caller provides explicit PK. Returns () on success.
db.insert_creature(Creature {
    id: CreatureId(1),
    name: "Aelindra".into(),
    species: Species::Elf,
    hunger: 0,
})?;

// Duplicate PK is an error.
let err = db.insert_creature(Creature {
    id: CreatureId(1),
    name: "Duplicate".into(),
    species: Species::Elf,
    hunger: 0,
});
assert!(err.is_err());  // Err(DuplicateKey { ... })

// FK validation on insert — referencing a nonexistent creature.
let err = db.insert_task(Task {
    id: TaskId(1),
    assignee: CreatureId(999),  // no such creature
    target: VoxelCoord(0, 0, 0),
    priority: 5,
});
assert_eq!(err.unwrap_err(), tabulosity::Error::FkTargetNotFound {
    table: "tasks",
    field: "assignee",
    referenced_table: "creatures",
    key: "CreatureId(999)".into(),
});

// Update — PK extracted from struct, indexes updated, FK fields validated.
let mut creature = db.creatures.get(&CreatureId(1)).unwrap();
creature.hunger += 10;
db.update_creature(creature)?;

// Upsert — inserts if PK doesn't exist, updates if it does.
db.upsert_creature(Creature {
    id: CreatureId(99),
    name: "Thorn".into(),
    species: Species::Capybara,
    hunger: 10,
})?;

// Delete with FK checks — returns the removed row on success.
let removed: Creature = db.remove_creature(&creature_id)?;
```

**Insert behavior:** Validates that all FK fields reference existing rows in the
target tables (skipping `None` for `Option<FK>`), then delegates to
`insert_no_fk`. Returns `()` on success. Since auto-key is not supported in
v1, the caller always provides the PK explicitly — returning it would be
redundant. If a row with the same PK already exists, returns
`Err(DuplicateKey { ... })`. If a FK field references a nonexistent row, returns
`Err(FkTargetNotFound { ... })`. Use `upsert_*` if you want insert-or-update
semantics.

**Update behavior:** Validates any changed FK fields against the referenced
tables, then delegates to `update_no_fk`. Extracts the PK from the struct,
looks up the existing row, compares each indexed field between old and new,
updates only the indexes whose values changed, then replaces the stored row.
Returns `Err(NotFound { ... })` if the PK doesn't exist. Returns
`Err(FkTargetNotFound { ... })` if a FK field references a nonexistent row.

**Upsert behavior:** If the PK exists, behaves like update (indexes maintained,
FK fields validated). If the PK doesn't exist, behaves like insert. Can return
`Err(FkTargetNotFound { ... })` but never `DuplicateKey` or `NotFound`.

**Delete behavior:** Checks that no other table's FK index references the row
being deleted (restrict semantics), then delegates to `remove_no_fk`.
Returns `Result<RowStruct, Error>` — on success, the caller gets back the
removed row. On failure, the error includes enough information to understand
what's still referencing the row:

```rust
Err(Error::FkViolation {
    table: "creatures",
    key: "CreatureId(7)",
    referenced_by: vec![
        ("tasks", "assignee", 3),  // 3 tasks reference this creature
    ],
})
```

**Write method naming convention:**

| Method                    | Returns                          | Possible errors                   |
|---------------------------|----------------------------------|-----------------------------------|
| `insert_{row}(r)`        | `Result<(), Error>`              | `DuplicateKey`, `FkTargetNotFound`|
| `update_{row}(r)`        | `Result<(), Error>`              | `NotFound`, `FkTargetNotFound`    |
| `upsert_{row}(r)`        | `Result<(), Error>`              | `FkTargetNotFound`                |
| `remove_{row}(&pk)`      | `Result<RowStruct, Error>`       | `NotFound`, `FkViolation`         |

All methods return `Result<T, tabulosity::Error>`, making `?` propagation
straightforward.

### Reads (Through Table Structs)

Reads go directly through the table fields on the Database:

```rust
// Primary key lookup — clone
let creature: Option<Creature> = db.creatures.get(&creature_id);

// Primary key lookup — reference (perf optimization, borrows table)
let creature: Option<&Creature> = db.creatures.get_ref(&creature_id);

// Existence check — lightweight, no cloning
let exists: bool = db.creatures.contains(&creature_id);

// Row count
let total: usize = db.creatures.len();
let empty: bool = db.creatures.is_empty();

// All primary keys (no row cloning)
let ids: Vec<CreatureId> = db.creatures.keys();
for id in db.creatures.iter_keys() { /* &CreatureId */ }

// All rows (returned in primary key order)
let all: Vec<Creature> = db.creatures.all();
for creature in db.creatures.iter_all() { /* &Creature, in PK order */ }

// Secondary index — equality
let elves: Vec<Creature> = db.creatures.by_species(&Species::Elf);
for elf in db.creatures.iter_by_species(&Species::Elf) { /* &Creature */ }

// Secondary index — range (only if field type implements Bounded)
let hungry: Vec<Creature> = db.creatures.by_hunger_range(50..);
for c in db.creatures.iter_by_hunger_range(..=20) { /* &Creature */ }

// Count (cheap — index scan only, no row lookups or cloning)
let n: usize = db.creatures.count_by_species(&Species::Elf);
let m: usize = db.creatures.count_by_hunger_range(50..);
```

**Read method naming convention:**

| Pattern                      | Returns               | Notes                          |
|------------------------------|-----------------------|--------------------------------|
| `get(&pk)`                   | `Option<T>`           | Clone by primary key           |
| `get_ref(&pk)`               | `Option<&T>`          | Borrow by primary key          |
| `contains(&pk)`              | `bool`                | Existence check, no cloning    |
| `len()`                      | `usize`               | Total row count                |
| `is_empty()`                 | `bool`                | Whether table has zero rows    |
| `keys()`                     | `Vec<PK>`             | Clone all primary keys         |
| `iter_keys()`                | `impl Iterator<&PK>`  | Iterate primary keys, PK order |
| `all()`                      | `Vec<T>`              | Clone all rows, PK order       |
| `iter_all()`                 | `impl Iterator<&T>`   | Iterate all rows, PK order     |
| `by_field(&val)`             | `Vec<T>`              | Clone matching rows            |
| `iter_by_field(&val)`        | `impl Iterator<&T>`   | Iterate matching rows          |
| `by_field_range(r)`          | `Vec<T>`              | Clone rows in range            |
| `iter_by_field_range(r)`     | `impl Iterator<&T>`   | Iterate rows in range          |
| `count_by_field(&val)`       | `usize`               | Count matching rows            |
| `count_by_field_range(r)`    | `usize`               | Count rows in range            |

**Ordering guarantee:** `all()` and `iter_all()` return rows in primary key
order. `keys()` and `iter_keys()` return keys in primary key order. This is a
property of the current BTree-based implementation — the backing `BTreeMap`
iterates in key order. Future alternative storage backends (e.g., hash-based)
would have unspecified order.

Clone-based reads (`get`, `all`, `by_*`, `keys`) are safe to interleave with
writes. Iterator/reference-based reads (`get_ref`, `iter_all`, `iter_by_*`,
`iter_keys`) borrow the table and prevent mutation until released — use them for
read-only scans. See the Borrow Checker Strategy section above for details and
examples.

## Internal Data Structures

### Row Storage

```
rows: BTreeMap<PK, RowStruct>
```

The primary store. Keyed by the primary key, values are the full row structs.
This is the source of truth; indexes are derived from it.

### Secondary Indexes

```
idx_{field}: BTreeSet<(FieldType, PK)>
```

One `BTreeSet` per `#[indexed]` field (including fields with `#[foreign_key]`,
which implies `#[indexed]`). The tuple `(field_value, primary_key)` is sorted
lexicographically — first by field value, then by PK as tiebreaker.

**Equality query** — range scan over a single field value:

```rust
let start = (value.clone(), PK::MIN);
let end = (value.clone(), PK::MAX);
self.idx_field.range(start..=end)
    .map(|(_, pk)| self.rows.get(pk).unwrap())
```

This is why the PK type needs `Bounded` — equality queries on any secondary
index use `PK::MIN` and `PK::MAX` to bound the scan.

**Range query** — range scan over a range of field values:

```rust
// by_hunger_range(50..)
let start = (50, PK::MIN);
self.idx_hunger.range((Included(start), Unbounded))
    .map(|(_, pk)| self.rows.get(pk).unwrap())
```

Range queries need `Bounded` on the *field type* as well (to translate range
bounds into composite tuple bounds). See the Bounded Trait section for details.

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
the FK field. `#[foreign_key(...)]` implies `#[indexed]` — if the field isn't
already annotated `#[indexed]`, the derive adds the index automatically.

To check whether a PK is still referenced: `count_by_{fk_field}(&pk) > 0`.

For `Option<FK>` fields, the index stores `(Option<FkType>, PK)` tuples. When
checking restrict-on-delete, only `Some(target_pk)` entries are counted — `None`
entries are not references to any row. The generated restrict check looks like:

```rust
// For Option<CreatureId> FK field:
self.tasks.count_by_assignee(&Some(creature_id.clone())) > 0
```

## Serialization

Behind a `serde` feature flag. When enabled:

**Serialize:** Each table serializes as a `Vec<RowStruct>` (just the rows, in
PK order). Indexes are not serialized — they're derived data. Since the Database
struct contains only table fields, serialization covers the entire struct
uniformly with no special cases.

**Deserialize:**

1. Deserialize each table's row vec.
2. Rebuild `rows` BTreeMap from the vec. **Detect duplicate primary keys:**
   if two rows in the vec have the same PK, report a `DuplicateKey` error rather
   than silently overwriting. Duplicates in serialized data indicate corruption
   or a bug in the serializer, and silently dropping rows would make such
   problems very hard to diagnose. All duplicate PKs across all tables are
   collected and reported together (don't fail on the first one).
3. Rebuild all secondary indexes by scanning every row.
4. Validate all FK constraints: for every FK field in every row, verify the
   referenced PK exists in the target table (skipping `None` values for
   `Option<FK>` fields). Report `FkTargetNotFound` for each violation. Collect
   all violations.
5. If any errors (duplicate PKs or FK target-not-found violations) were found,
   return `Err(DeserializeError { errors: [...] })` listing them all — report
   everything so problems can be fixed in one pass.

This means saved data is just rows — compact, human-readable (in JSON), and
indexes are always consistent with the data since they're rebuilt fresh.

The exact mechanism for serialization/deserialization (serde derive, custom serde
impl, or a `from_rows()` method) is an implementation detail to be determined
during development. The design specifies the semantics described above.

## Proc Macro Implementation Notes

### `#[derive(Table)]`

Generates a companion struct `{RowName}Table` with:

- Private internal fields:
  - `rows: BTreeMap<PK, RowStruct>` (the primary store)
  - `idx_{field}: BTreeSet<(FieldType, PK)>` for each `#[indexed]` field
    (including fields with `#[foreign_key]`, which implies `#[indexed]`)
- All read methods — these are `pub`:
  - `get(&PK)`, `get_ref(&PK)`, `contains(&PK)`, `len()`, `is_empty()`
  - `keys()`, `iter_keys()`
  - `all()`, `iter_all()`
  - `by_{field}(&val)`, `iter_by_{field}(&val)`, `count_by_{field}(&val)` for
    each indexed field
  - `by_{field}_range(r)`, `iter_by_{field}_range(r)`,
    `count_by_{field}_range(r)` for each indexed field whose type implements
    `Bounded`
- Mutation methods with `_no_fk` suffix — these are `pub` but
  **`#[doc(hidden)]`**:
  - `insert_no_fk`: inserts a row and updates all indexes. Returns
    `Result<(), Error>` — `Err(DuplicateKey)` if PK exists. Does not validate
    FK fields.
  - `update_no_fk`: updates a row and maintains indexes. Returns
    `Result<(), Error>` — `Err(NotFound)` if PK missing. Does not validate FK
    fields.
  - `remove_no_fk`: removes a row and cleans up indexes. Returns
    `Result<RowStruct, Error>` — `Err(NotFound)` if PK missing. Does not check
    for incoming FK references.
- `rebuild_indexes(&mut self)` — `#[doc(hidden)] pub`. Reconstructs all
  secondary indexes by scanning every row in `self.rows`. Used during
  deserialization.

The derive macro reads the struct definition, finds annotated fields, and
generates the companion type with all the boilerplate.

**Range methods and `Bounded`:** The macro generates `by_{field}_range`,
`iter_by_{field}_range`, and `count_by_{field}_range` methods only when the
field type implements `Bounded`. This is checked at compile time — the macro
generates the methods unconditionally, but they include a `where FieldType:
Bounded` bound. If the field type doesn't implement `Bounded`, those methods
simply won't be callable. Equality queries (`by_{field}`, `iter_by_{field}`,
`count_by_{field}`) only need `Bounded` on the PK type (which is always
required), so they work for any indexed field regardless of whether the field
type implements `Bounded`.

**What the Table derive does NOT generate:** Unlike v5, the Table derive no
longer generates `check_fk_*` or `count_fk_*` helper methods. The
`#[foreign_key(references = "...")]` attribute tells the Table derive to generate
an index for that field (since `#[foreign_key]` implies `#[indexed]`), but all
FK validation logic is generated by the Database derive using the `fks(...)`
metadata on `#[table]` attributes. See the Cross-Derive Information Flow section
for why.

### `#[derive(Database)]`

Reads the schema struct, verifies every field has `#[table(singular = "...")]`,
and:

1. **Rejects non-table fields.** Any field without `#[table(singular = "...")]`
   is a compile error. This keeps the Database struct focused on relational data.
2. **Reads FK declarations from `fks(...)`.** For each `#[table]` attribute with
   `fks(field = "target_table", ...)`, the derive records the FK relationships.
   It uses these to generate FK validation code directly — no intermediate
   methods needed.
3. **Generates `new()`.** Creates all tables (empty). Since every field is a
   table and every table starts empty, this requires no trait bounds or user
   input.
4. **Generates safe write methods.** For each table with singular name `foo`,
   generates `insert_foo`, `update_foo`, `upsert_foo`, and `remove_foo` methods
   on the Database. These methods perform FK validation (checking referenced
   tables for insert/update, checking referencing tables for remove), then
   delegate to the table's `_no_fk` methods for the actual mutation.
5. **Generates deserialization** that rebuilds indexes, detects duplicate PKs,
   and validates FKs.

### Cross-Derive Information Flow

The Table derive and Database derive are separate proc macros that run
independently. They need to share information — for example, the Database derive
needs to know which fields in a Task are foreign keys and which tables they
reference.

**The pragmatic solution: FK info is declared in two places.**

This is a deliberate DRY violation, and it deserves an honest explanation.

Each proc macro in Rust can only read the attributes on the struct it is applied
to. `#[derive(Table)]` on the `Task` struct can read `Task`'s fields and their
attributes, but it cannot see the `MyDb` struct. `#[derive(Database)]` on `MyDb`
can read `MyDb`'s fields and their attributes, but it cannot look inside the
`Task` struct to find its `#[foreign_key]` annotations.

So FK information lives in two places:

1. **On the row struct field** — `#[foreign_key(references = "creatures")]` on
   `Task::assignee`. This is read by the Table derive, which uses it to generate
   an index on that field (since `#[foreign_key]` implies `#[indexed]`).

2. **On the Database `#[table]` attribute** — `fks(assignee = "creatures")`.
   This is read by the Database derive, which uses it to generate FK validation
   code in the `insert_task`, `update_task`, and `remove_creature` methods.

Yes, this means the user writes the FK relationship twice. If the two
declarations are inconsistent — say, the row struct says
`#[foreign_key(references = "creatures")]` but the Database attribute says
`fks(assignee = "zones")` — the generated code will still compile as long as
the types happen to match. But if there's a genuine mismatch (wrong field name,
wrong target table), the generated code references fields or methods that don't
exist, and the Rust compiler produces a normal compile error. A field name typo
in `fks(assignee_typo = "creatures")` generates code accessing
`row.assignee_typo`, which doesn't exist on the struct — compiler error.

The alternative approaches all have worse tradeoffs:

- **Having the Database derive parse the row struct source:** Proc macros don't
  have access to other files' source code. You'd need a build script or
  code-generation step outside the proc macro system.
- **A shared trait carrying FK metadata:** The Database derive runs at macro
  expansion time and operates on token streams. It cannot call trait methods or
  read trait associated types — those exist at compile time, not at macro time.
  An earlier draft (v4) proposed this and it was dropped as unworkable.
- **Putting all FK info only on the Database struct:** Then the Table derive
  wouldn't know which fields need indexes for FK lookups. You'd have to
  redundantly annotate those fields with `#[indexed]` anyway, which is the same
  duplication in a different form.

The current approach is simple, explicit, and produces clear compiler errors when
things don't match. It's not elegant, but it's the pragmatic choice given Rust's
proc macro limitations.

**How the generated code fits together:**

**Step 1: The Table derive generates the index.** When `#[derive(Table)]` runs
on `Task` and sees `#[foreign_key(references = "creatures")]` on `assignee`, it
generates a `BTreeSet<(CreatureId, TaskId)>` index (or
`BTreeSet<(Option<CreatureId>, TaskId)>` for optional FKs) and the standard
`by_assignee`, `count_by_assignee`, etc. read methods. It does not generate any
FK validation methods — that's the Database derive's job.

**Step 2: The Database derive generates FK validation directly.** When
`#[derive(Database)]` runs on `MyDb` and sees
`fks(assignee = "creatures")` on the `tasks` table, it generates:

For insert/update (checking that the FK target exists):

```rust
// Generated by #[derive(Database)] for insert_task:
fn insert_task(&mut self, row: Task) -> Result<(), tabulosity::Error> {
    // FK validation — generated from fks(assignee = "creatures").
    // For non-Option FK fields:
    if !self.creatures.contains(&row.assignee) {
        return Err(tabulosity::Error::FkTargetNotFound {
            table: "tasks",
            field: "assignee",
            referenced_table: "creatures",
            key: format!("{:?}", row.assignee),
        });
    }
    self.tasks.insert_no_fk(row)
}
```

For `Option<FK>` fields, the generated validation skips `None`:

```rust
// For Option<CreatureId> FK field:
if let Some(ref fk_val) = row.assignee {
    if !self.creatures.contains(fk_val) {
        return Err(tabulosity::Error::FkTargetNotFound {
            table: "tasks",
            field: "assignee",
            referenced_table: "creatures",
            key: format!("{:?}", fk_val),
        });
    }
}
```

For restrict-on-delete (checking that no rows reference the row being removed):

```rust
// Generated by #[derive(Database)] for remove_creature:
fn remove_creature(&mut self, id: &CreatureId) -> Result<Creature, tabulosity::Error> {
    // Restrict check — generated from fks(assignee = "creatures") on tasks.
    // For Option<CreatureId> FK fields, count only Some(id) entries:
    let count = self.tasks.count_by_assignee(&Some(id.clone()));
    // For non-Option FK fields, it would be:
    // let count = self.tasks.count_by_assignee(id);
    if count > 0 {
        return Err(tabulosity::Error::FkViolation {
            table: "creatures",
            key: format!("{:?}", id),
            referenced_by: vec![("tasks", "assignee", count)],
        });
    }
    self.creatures.remove_no_fk(id)
}
```

**Step 3: The Rust compiler connects everything.** After both macros have
expanded, the generated code goes through normal Rust compilation — type
checking, borrow checking, name resolution. If `fks(assignee = "creatures")`
names a field that doesn't exist on the row struct, the generated
`row.assignee` doesn't compile. If it names a target table that doesn't exist
on the Database struct, `self.creatures` doesn't compile. If the FK type doesn't
match the target table's PK type, `self.creatures.contains(&row.assignee)`
produces a type error. The proc macro doesn't need to validate any of this — the
Rust compiler does what it's good at.

### Bounded Trait

The `Bounded` trait provides `MIN` and `MAX` associated constants. It serves
two distinct purposes:

1. **PK types always need `Bounded`.** Equality queries on secondary indexes use
   `PK::MIN` and `PK::MAX` to bound the composite-tuple range scan. Every PK
   type must implement `Bounded`.

2. **Indexed field types need `Bounded` only for range queries.** The
   `by_{field}_range`, `iter_by_{field}_range`, and `count_by_{field}_range`
   methods need to translate `RangeBounds<FieldType>` into composite tuple
   bounds, which requires `FieldType::MIN` and `FieldType::MAX`. If a field type
   doesn't implement `Bounded`, the equality-based methods (`by_{field}`,
   `iter_by_{field}`, `count_by_{field}`) still work — they only need `Bounded`
   on the PK type. The range methods simply won't be available.

Implementation:

- A `Bounded` trait defined in the `tabulosity` crate:
  ```rust
  pub trait Bounded {
      const MIN: Self;
      const MAX: Self;
  }
  ```
- Blanket impls for common types (`u8`, `u16`, `u32`, `u64`, `i8`, `i16`,
  `i32`, `i64`, `bool`, etc.).
- A blanket impl for `Option<T>`:
  ```rust
  impl<T: Bounded> Bounded for Option<T> {
      const MIN: Self = None;
      const MAX: Self = Some(T::MAX);
  }
  ```
  This is needed for range queries on optional FK fields. The ordering is
  correct: `None < Some(anything)` in Rust's standard `Option` ordering, and
  `Some(T::MAX)` is the largest possible `Some` value. This means
  `by_assignee_range(Some(CreatureId(5))..)` works naturally — it finds all rows
  with `assignee >= Some(CreatureId(5))`, which is all rows assigned to creature
  5 or higher (excluding unassigned rows where `assignee = None`).
- A `#[derive(Bounded)]` macro for newtype wrappers:
  `#[derive(Bounded)] struct CreatureId(u64)` generates
  `MIN = CreatureId(u64::MIN)`, `MAX = CreatureId(u64::MAX)`.
- Types like `String` don't implement `Bounded`, so string-indexed fields get
  equality queries but not range queries. This is correct — there's no sensible
  `String::MAX`.

## Future Work

- **Auto-generated primary keys.** `#[primary_key(auto)]` with a monotonically
  increasing counter. The PK type needs `From<u64>` (or similar). The table
  assigns the key on insert when the caller provides a sentinel value. Deferred
  from v1 to keep the initial implementation simple. When added, the insert
  method for auto-key tables would use a distinct name (e.g., `insert_creature_auto`)
  or a separate method that returns the generated PK as `Result<PK, Error>`,
  keeping the standard `insert_creature` signature unchanged.
- **Compound indexes.** `BTreeSet<(Field1, Field2, ..., PK)>` with prefix
  query support. Attribute syntax for grouping fields:
  `#[compound_index(name = "...", fields = ["assignee", "priority"])]`.
  Useful for "all tasks for creature X, ordered by priority" in a single
  index scan.
- **Joins.** Query API that traverses FK relationships:
  `db.tasks.join_assignee()` -> iterator of `(&Task, &Creature)`. Ergonomic
  but complex to generate.
- **Cascade / nullify on delete.** `#[foreign_key(references = "...", on_delete = "cascade")]`
  and `#[foreign_key(references = "...", on_delete = "nullify")]` (field becomes
  `Option<FK>`, set to `None` on referenced row deletion).
- **Unique indexes.** `#[indexed(unique)]` — enforced on insert/update.
- **Filtered indexes / partial indexes.** Index only rows matching a predicate.
- **Change tracking.** Tables emit a list of changes (insert/update/delete)
  per tick, useful for event-driven rendering.
