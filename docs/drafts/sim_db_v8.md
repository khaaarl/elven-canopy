# Tabulosity — In-Memory Relational Store (Design Draft v8)

A lightweight, typed, in-memory relational database library for Rust. Designed
for game simulations and other domains where you want relational integrity
(primary keys, indexes, foreign keys) without the weight of SQL, an external
database engine, or ORM impedance mismatch.

Changes from v7: added `upsert_no_fk` to the table struct's generated methods
(infallible, returns `()`); `remove_*` now explicitly collects ALL inbound FK
violations across all referencing tables before returning an error (no
short-circuit); introduced `fks(assignee? = "creatures")` syntax where the `?`
suffix marks optional FK fields, enabling correct restrict-on-delete codegen for
`Option<T>` fields; Table derive generates conditional serde impls behind the
`serde` feature flag (serialize rows only, rebuild indexes on deserialize); added
`modify`/`update_with` closure-based API and schema evolution to Future Work.

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
│       ├── lib.rs           # Public API, re-exports
│       ├── table.rs         # Bounded trait, FkCheck trait, range bound helpers
│       └── error.rs         # tabulosity::Error enum, DeserializeError
├── tabulosity_derive/       # Proc macro crate (must be separate)
│   └── src/
│       └── lib.rs           # #[derive(Table)], #[derive(Database)], #[derive(Bounded)]
└── Cargo.toml               # Workspace
```

The proc macro crate (`tabulosity_derive`) is a compile-time dependency only.
Users interact with `tabulosity` and its derive macros.

**`table.rs` contents:** This module holds shared types and traits used by
generated code. It does NOT contain a generic `Table<K, V>` type — there is no
such type. Everything table-specific is code-generated per row struct by the
Table derive macro. What `table.rs` provides:

- `Bounded` trait (with blanket impls for common types and `Option<T>`).
- `FkCheck` trait (with impls for `T` and `Option<T>` — used by Database-derive
  generated FK validation code).
- Helper functions for translating `RangeBounds<FieldType>` into composite
  `RangeBounds<(FieldType, PK)>` for secondary index range scans.

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
        /// ALL referencing tables/fields are checked and all violations are
        /// collected — the check does NOT short-circuit on the first violation.
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

    #[indexed]
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

That's it. The Table derive knows about exactly two attributes: `#[primary_key]`
and `#[indexed]`. There is no `#[foreign_key]` attribute on row structs. FK
relationships are declared exclusively on the Database struct — see the
"Defining the Database Schema" section below.

Fields that serve as foreign keys should be annotated `#[indexed]` because
the index is needed for restrict-on-delete checks. But as far as the Table
derive is concerned, they're just indexed fields — it has no concept of
foreign keys.

**Required trait bounds on row structs:**

- `Clone` — for clone-based reads (`get`, `all`, `by_*`).
- The primary key type must implement `Ord + Clone + Bounded + Debug`.
  `Bounded` provides `MIN` and `MAX` values needed for equality queries on
  composite secondary indexes. `Debug` is needed for error messages (formatting
  the key in `DuplicateKey`, `NotFound`, `FkTargetNotFound`, `FkViolation`
  errors).
- Indexed field types must implement `Ord + Clone`. They additionally need
  `Bounded` only if range queries are desired — see the Bounded Trait section.
  FK fields additionally need `Debug` (for `FkTargetNotFound` error messages).

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

    #[indexed]
    pub assignee: Option<CreatureId>,

    #[indexed]
    pub priority: u8,
}
```

The FK relationship is declared on the Database struct, not here — the Task
struct just marks `assignee` as `#[indexed]`. See "Defining the Database Schema"
for how this connects to the `creatures` table, and note the `?` suffix syntax
used to declare optional FKs.

**Behavior:**

- **FK validation on insert/update:** `None` values are skipped — no existence
  check is performed. `Some(id)` values are validated against the referenced
  table as usual. This is handled uniformly via the `FkCheck` trait — see the
  FkCheck Trait section below.
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

**The `fks(...)` attribute:** Foreign key relationships are declared exclusively
on the `#[table]` attribute using the `fks(...)` syntax. This is the ONLY place
FK relationships are declared — there is no corresponding annotation on the row
struct. Each entry maps a field name on the row struct to the target table's
field name on the Database struct:

```rust
#[table(singular = "task", fks(assignee = "creatures"))]
```

This tells the Database derive: "the `assignee` field on the `Task` row struct
is a foreign key referencing the `creatures` table." For multiple FKs:

```rust
#[table(singular = "task", fks(assignee = "creatures", location = "zones"))]
```

**Optional FK syntax (`?` suffix):** When an FK field is `Option<T>` on the row
struct, append `?` to the field name in the `fks(...)` declaration:

```rust
#[table(singular = "task", fks(assignee? = "creatures"))]
```

The `?` suffix tells the Database derive that the `assignee` field on the row
struct is `Option<CreatureId>` rather than bare `CreatureId`. This affects only
the restrict-on-delete code generation direction:

- **Bare FK** (`fks(assignee = "creatures")`): the generated restrict check is
  `self.tasks.count_by_assignee(&id.clone())`, because the index stores
  `(CreatureId, TaskId)` tuples.
- **Optional FK** (`fks(assignee? = "creatures")`): the generated restrict check
  is `self.tasks.count_by_assignee(&Some(id.clone()))`, because the index stores
  `(Option<CreatureId>, TaskId)` tuples.

The `?` syntax is only needed for the restrict-on-delete direction. For the
insert/update direction (checking that the FK target exists), the `FkCheck`
trait already handles both bare and `Option<T>` fields uniformly — the same
generated code works for both cases. See the FkCheck Trait section below.

**Example with both bare and optional FKs:**

```rust
#[derive(Table, Clone, Debug)]
pub struct Task {
    #[primary_key]
    pub id: TaskId,

    #[indexed]
    pub creator: CreatureId,         // bare FK — every task has a creator

    #[indexed]
    pub assignee: Option<CreatureId>, // optional FK — unassigned tasks have None
}

#[derive(Database)]
pub struct MyDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    // creator is bare FK (no ?), assignee is optional FK (has ?)
    #[table(singular = "task", fks(creator = "creatures", assignee? = "creatures"))]
    pub tasks: TaskTable,
}
```

The generated `remove_creature` will check both:
- `self.tasks.count_by_creator(&id.clone())` — bare FK, queries index directly
- `self.tasks.count_by_assignee(&Some(id.clone()))` — optional FK, wraps in `Some`

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
    pub fn upsert_no_fk(&mut self, row: Creature) { ... }

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
`insert_no_fk` returns `Err(DuplicateKey)` if the PK already exists,
`update_no_fk` returns `Err(NotFound)` if the PK is missing, and
`remove_no_fk` returns `Err(NotFound)` if the PK is missing.

**`upsert_no_fk` is infallible** — it returns `()`, not `Result`. Since upsert
handles both the "PK exists" and "PK doesn't exist" cases, it can never fail
with `DuplicateKey` or `NotFound`. And since this is the `_no_fk` variant, there
are no FK checks to fail either. Semantics: if the PK exists, update the row
(maintaining indexes by comparing old and new indexed field values); if the PK
doesn't exist, insert the row (adding to all indexes). The Database-level
`upsert_*` method performs FK validation, then delegates to `upsert_no_fk`.

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
target tables (skipping `None` for `Option<FK>` via the `FkCheck` trait), then
delegates to `insert_no_fk`. Returns `()` on success. Since auto-key is not
supported in v1, the caller always provides the PK explicitly — returning it
would be redundant. If a row with the same PK already exists, returns
`Err(DuplicateKey { ... })`. If a FK field references a nonexistent row, returns
`Err(FkTargetNotFound { ... })`. Use `upsert_*` if you want insert-or-update
semantics.

**Update behavior:** Validates ALL FK fields against the referenced tables (not
just changed ones), then delegates to `update_no_fk`. This is simpler than
diffing FK fields and more correct — it catches stale FK references that may
exist if someone previously used `_no_fk` methods to delete a referenced row
without FK checks. The cost is negligible: one `contains` call per FK field per
update. After FK validation, the method extracts the PK from the struct, looks
up the existing row, compares each indexed field between old and new, updates
only the indexes whose values changed, then replaces the stored row. Returns
`Err(NotFound { ... })` if the PK doesn't exist. Returns
`Err(FkTargetNotFound { ... })` if a FK field references a nonexistent row.

**Upsert behavior:** Validates all FK fields (via `FkCheck` trait), then
delegates to `upsert_no_fk`. If the PK exists, behaves like update (indexes
maintained). If the PK doesn't exist, behaves like insert. Can return
`Err(FkTargetNotFound { ... })` but never `DuplicateKey` or `NotFound`.

**Delete behavior:** Checks ALL tables with FK references pointing at the table
being removed from and collects ALL violations (no short-circuit). For each
inbound FK relationship, counts the number of rows referencing the target PK.
If any counts are nonzero, returns an error with every violation listed. See the
generated code example in the Cross-Derive Information Flow section for the
exact pattern. Returns `Result<RowStruct, Error>` — on success, the caller gets
back the removed row. On failure, the error includes all referencing
tables/fields and their counts:

```rust
Err(Error::FkViolation {
    table: "creatures",
    key: "CreatureId(7)",
    referenced_by: vec![
        ("tasks", "assignee", 3),      // 3 tasks reference this creature
        ("friendships", "target", 1),   // 1 friendship references this creature
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

One `BTreeSet` per `#[indexed]` field. The tuple `(field_value, primary_key)` is
sorted lexicographically — first by field value, then by PK as tiebreaker.

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
the FK field. FK fields are annotated `#[indexed]` on the row struct, which
generates the secondary index that both read queries and FK validation use.

To check whether a PK is still referenced: `count_by_{fk_field}(&pk) > 0`.

For `Option<FK>` fields, the index stores `(Option<FkType>, PK)` tuples. When
checking restrict-on-delete, only `Some(target_pk)` entries are counted — `None`
entries are not references to any row. The generated restrict check looks like:

```rust
// For Option<CreatureId> FK field (declared as fks(assignee? = "creatures")):
self.tasks.count_by_assignee(&Some(creature_id.clone())) > 0

// For bare CreatureId FK field (declared as fks(assignee = "creatures")):
self.tasks.count_by_assignee(&creature_id.clone()) > 0
```

## FkCheck Trait

The Database derive generates FK validation code, but it only sees the
`fks(assignee = "creatures")` metadata — it does not know whether the
`assignee` field on the row struct is `CreatureId` or `Option<CreatureId>`.
Rather than requiring special cases in the macro for bare vs optional FK fields,
we use a helper trait that makes the generated code uniform:

```rust
/// Helper trait for FK validation. Allows the Database derive to generate
/// the same code regardless of whether an FK field is T or Option<T>.
pub trait FkCheck<K> {
    fn check_fk<F: Fn(&K) -> bool>(&self, f: F) -> bool;
}

impl<K> FkCheck<K> for K {
    fn check_fk<F: Fn(&K) -> bool>(&self, f: F) -> bool {
        f(self)
    }
}

impl<K> FkCheck<K> for Option<K> {
    fn check_fk<F: Fn(&K) -> bool>(&self, f: F) -> bool {
        match self {
            None => true,
            Some(k) => f(k),
        }
    }
}
```

**How the Database derive uses it.** The macro generates the same code for every
FK field, regardless of whether it is optional:

```rust
// Generated by #[derive(Database)] for insert_task:
fn insert_task(&mut self, row: Task) -> Result<(), tabulosity::Error> {
    // FK validation — generated from fks(assignee = "creatures").
    // Works for both CreatureId and Option<CreatureId>.
    if !row.assignee.check_fk(|k| self.creatures.contains(k)) {
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

For a bare `CreatureId` field, `check_fk` calls the closure with `self` (the
`CreatureId` value) and returns the closure's result. For an
`Option<CreatureId>` field, `check_fk` returns `true` for `None` (no FK to
validate) and calls the closure with the inner value for `Some`. The macro
doesn't need to know which case it is — it generates identical code either way,
and Rust's trait resolution picks the right impl at compile time.

This trait lives in the `tabulosity` crate (in `table.rs`) and is part of the
public API, since the Database derive's generated code calls it.

**Note:** The `FkCheck` trait handles insert/update validation uniformly for
both `Option` and bare FK fields. The `?` suffix in `fks(assignee? = "creatures")`
is only needed for the restrict-on-delete direction, where the generated code
must know whether to wrap the PK in `Some(...)` when querying the index.

## Serialization

Behind a `serde` feature flag. When enabled, the Table derive generates custom
`Serialize` and `Deserialize` impls on the companion table struct (e.g.,
`CreatureTable`). These impls serialize only the rows (as a `Vec` in PK order)
and skip all indexes. On deserialization, the rows are loaded and indexes are
rebuilt from the row data. This means the serialized format is compact and
implementation-stable — adding or removing indexes does not change the wire
format.

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
   `Option<FK>` fields via the `FkCheck` trait). Report `FkTargetNotFound` for
   each violation. Collect all violations.
5. If any errors (duplicate PKs or FK target-not-found violations) were found,
   return `Err(DeserializeError { errors: [...] })` listing them all — report
   everything so problems can be fixed in one pass.

This means saved data is just rows — compact, human-readable (in JSON), and
indexes are always consistent with the data since they're rebuilt fresh.

## Proc Macro Implementation Notes

### `#[derive(Table)]`

Generates a companion struct `{RowName}Table` with:

- Private internal fields:
  - `rows: BTreeMap<PK, RowStruct>` (the primary store)
  - `idx_{field}: BTreeSet<(FieldType, PK)>` for each `#[indexed]` field
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
  - `upsert_no_fk`: upserts a row — if PK exists, updates (maintaining
    indexes); if PK doesn't exist, inserts (adding to all indexes). Returns
    `()` — infallible, since it handles both cases and performs no FK checks.
  - `remove_no_fk`: removes a row and cleans up indexes. Returns
    `Result<RowStruct, Error>` — `Err(NotFound)` if PK missing. Does not check
    for incoming FK references.
- `rebuild_indexes(&mut self)` — `#[doc(hidden)] pub`. Reconstructs all
  secondary indexes by scanning every row in `self.rows`. Used during
  deserialization.
- **Conditional serde impls** (behind the `serde` feature flag): custom
  `Serialize` impl that serializes `self.rows` values as a `Vec` in PK order;
  custom `Deserialize` impl that deserializes a `Vec<RowStruct>`, populates
  `self.rows`, and calls `rebuild_indexes()`. These impls ensure indexes are
  never part of the serialized format.

The Table derive knows about exactly two attributes: `#[primary_key]` and
`#[indexed]`. It has no concept of foreign keys. Fields that happen to serve as
FKs are just indexed fields from the Table derive's perspective.

**Range methods and `Bounded`:** The macro generates `by_{field}_range`,
`iter_by_{field}_range`, and `count_by_{field}_range` methods only when the
field type implements `Bounded`. This is checked at compile time — the macro
generates the methods unconditionally, but they include a `where FieldType:
Bounded` bound. If the field type doesn't implement `Bounded`, those methods
simply won't be callable. Equality queries (`by_{field}`, `iter_by_{field}`,
`count_by_{field}`) only need `Bounded` on the PK type (which is always
required), so they work for any indexed field regardless of whether the field
type implements `Bounded`.

### `#[derive(Database)]`

Reads the schema struct, verifies every field has `#[table(singular = "...")]`,
and:

1. **Rejects non-table fields.** Any field without `#[table(singular = "...")]`
   is a compile error. This keeps the Database struct focused on relational data.
2. **Reads FK declarations from `fks(...)`.** For each `#[table]` attribute with
   `fks(field = "target_table", ...)` or `fks(field? = "target_table", ...)`,
   the derive records the FK relationships and whether each FK is optional
   (marked with `?`). It uses these to generate FK validation code directly —
   no intermediate methods needed.
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
independently. The key design decision is that they do NOT need to share
information about foreign keys.

Each proc macro in Rust can only read the attributes on the struct it is applied
to. `#[derive(Table)]` on the `Task` struct can read `Task`'s fields and their
attributes, but it cannot see the `MyDb` struct. `#[derive(Database)]` on `MyDb`
can read `MyDb`'s fields and their attributes, but it cannot look inside the
`Task` struct to find its field annotations.

The solution is a clean separation of concerns:

- **The Table derive generates indexes and read/write methods.** It processes
  `#[primary_key]` and `#[indexed]` attributes. It knows nothing about foreign
  keys — an indexed field that happens to be an FK is just an indexed field.

- **The Database derive owns all FK logic.** It reads `fks(...)` declarations
  from `#[table]` attributes and generates all FK validation code: existence
  checks on insert/update (via the `FkCheck` trait), restrict checks on delete
  (via `count_by_*` on the referencing table's index). It also generates the
  `remove_*` restrict checks by inverting the FK map — if `tasks` declares
  `fks(assignee = "creatures")`, then `remove_creature` must check the tasks
  table's `count_by_assignee` index.

There is no DRY violation. FK information is declared in exactly one place: the
`fks(...)` attribute on the Database struct's `#[table]` annotation. The Table
derive doesn't need to know about FKs at all. The user marks FK fields as
`#[indexed]` on the row struct (which they'd want anyway for query access), and
declares the FK relationship once on the Database struct.

**How the generated code fits together:**

**Step 1: The Table derive generates indexes.** When `#[derive(Table)]` runs on
`Task` and sees `#[indexed]` on `assignee`, it generates a
`BTreeSet<(CreatureId, TaskId)>` index (or
`BTreeSet<(Option<CreatureId>, TaskId)>` for optional fields) and the standard
`by_assignee`, `count_by_assignee`, etc. read methods. It does not generate any
FK validation — it doesn't know this field is an FK.

**Step 2: The Database derive generates FK validation.** When
`#[derive(Database)]` runs on `MyDb` and sees
`fks(assignee = "creatures")` on the `tasks` table, it generates:

For insert/update (checking that the FK target exists), using the `FkCheck`
trait for uniform handling of bare and `Option` fields:

```rust
// Generated by #[derive(Database)] for insert_task:
fn insert_task(&mut self, row: Task) -> Result<(), tabulosity::Error> {
    // FK validation — generated from fks(assignee = "creatures").
    // FkCheck trait handles both bare and Option FK fields uniformly.
    if !row.assignee.check_fk(|k| self.creatures.contains(k)) {
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

For update (validates ALL FK fields, not just changed ones):

```rust
// Generated by #[derive(Database)] for update_task:
fn update_task(&mut self, row: Task) -> Result<(), tabulosity::Error> {
    // FK validation — always validates all FK fields.
    if !row.assignee.check_fk(|k| self.creatures.contains(k)) {
        return Err(tabulosity::Error::FkTargetNotFound {
            table: "tasks",
            field: "assignee",
            referenced_table: "creatures",
            key: format!("{:?}", row.assignee),
        });
    }
    self.tasks.update_no_fk(row)
}
```

For upsert (validates FK fields, then delegates to infallible `upsert_no_fk`):

```rust
// Generated by #[derive(Database)] for upsert_task:
fn upsert_task(&mut self, row: Task) -> Result<(), tabulosity::Error> {
    // FK validation — same as insert/update.
    if !row.assignee.check_fk(|k| self.creatures.contains(k)) {
        return Err(tabulosity::Error::FkTargetNotFound {
            table: "tasks",
            field: "assignee",
            referenced_table: "creatures",
            key: format!("{:?}", row.assignee),
        });
    }
    self.tasks.upsert_no_fk(row);
    Ok(())
}
```

For restrict-on-delete (checking that no rows reference the row being removed).
Note: the generated code checks ALL inbound FK relationships and collects ALL
violations. It does NOT short-circuit on the first violation:

```rust
// Generated by #[derive(Database)] for remove_creature.
// Assumes tasks has fks(assignee? = "creatures") — optional FK.
// Also assumes friendships has fks(target = "creatures") — bare FK.
fn remove_creature(&mut self, id: &CreatureId) -> Result<Creature, tabulosity::Error> {
    // Check PK exists first.
    if !self.creatures.contains(id) {
        return Err(tabulosity::Error::NotFound {
            table: "creatures",
            key: format!("{:?}", id),
        });
    }

    // Collect ALL inbound FK violations — do NOT short-circuit.
    let mut violations: Vec<(&'static str, &'static str, usize)> = Vec::new();

    // Check tasks.assignee — optional FK (declared with ?), so wrap in Some.
    let count = self.tasks.count_by_assignee(&Some(id.clone()));
    if count > 0 {
        violations.push(("tasks", "assignee", count));
    }

    // Check friendships.target — bare FK (no ?), so pass id directly.
    let count = self.friendships.count_by_target(id);
    if count > 0 {
        violations.push(("friendships", "target", count));
    }

    // If any violations found, return them ALL.
    if !violations.is_empty() {
        return Err(tabulosity::Error::FkViolation {
            table: "creatures",
            key: format!("{:?}", id),
            referenced_by: violations,
        });
    }

    self.creatures.remove_no_fk(id)
}
```

The `?` suffix on FK declarations controls the restrict-on-delete direction
only:

- `fks(assignee? = "creatures")` generates
  `self.tasks.count_by_assignee(&Some(id.clone()))` — the `?` tells the derive
  to wrap the PK in `Some(...)` because the index stores `Option<CreatureId>`
  values.
- `fks(target = "creatures")` generates
  `self.friendships.count_by_target(id)` — no wrapping, the index stores bare
  `CreatureId` values.

This eliminates the ambiguity from v7 where the Database derive could not
distinguish bare from optional FK fields and relied on the Rust compiler to
catch type mismatches. With `?`, the macro generates correct code directly.

**Step 3: The Rust compiler connects everything.** After both macros have
expanded, the generated code goes through normal Rust compilation — type
checking, borrow checking, name resolution. If `fks(assignee = "creatures")`
names a field that doesn't exist on the row struct, the generated
`row.assignee` doesn't compile. If it names a target table that doesn't exist
on the Database struct, `self.creatures` doesn't compile. If the FK type doesn't
match the target table's PK type, `self.creatures.contains(k)` (where `k` comes
from `row.assignee.check_fk(...)`) produces a type error. The proc macro doesn't
need to validate any of this — the Rust compiler does what it's good at.

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

## Required Trait Bounds Summary

| Type role          | Required traits                    | Notes                              |
|--------------------|------------------------------------|------------------------------------|
| Row struct         | `Clone`                            | For clone-based reads              |
| Primary key        | `Ord + Clone + Bounded + Debug`    | `Bounded` for index scans, `Debug` for error messages |
| Indexed field      | `Ord + Clone`                      | Minimum for equality queries       |
| Indexed + range    | `Ord + Clone + Bounded`            | `Bounded` for range bound translation |
| FK field           | `Ord + Clone + Debug`              | `Debug` for `FkTargetNotFound` error messages; `Ord + Clone` because FK fields are `#[indexed]` |
| FK field + range   | `Ord + Clone + Bounded + Debug`    | All of the above                   |

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
- **Cascade / nullify on delete.** Declared via `fks(...)` syntax on the
  Database struct, e.g., `fks(assignee = "creatures" on_delete cascade)` or
  `fks(assignee = "creatures" on_delete nullify)` (field becomes `Option<FK>`,
  set to `None` on referenced row deletion).
- **Closure-based update (`update_with`).** Something like
  `db.update_creature_with(&id, |c| c.hunger += 10)` that takes a closure
  receiving `&mut RowStruct` and applies it in-place. This avoids the
  clone-modify-update dance that the current API requires:
  ```rust
  // Current pattern — clone, modify, update:
  let mut creature = db.creatures.get(&id).unwrap();
  creature.hunger += 10;
  db.update_creature(creature)?;

  // With update_with — no clone needed:
  db.update_creature_with(&id, |c| c.hunger += 10)?;
  ```
  More efficient for large structs (avoids a full clone) and more ergonomic for
  single-field changes. The method would still need to maintain indexes
  (compare old and new indexed field values after the closure runs) and validate
  FK fields. Implementation complexity is moderate — the main challenge is
  that the closure receives `&mut Row` directly into the BTreeMap value, so
  index maintenance must happen after the closure returns by comparing the row's
  current state against a snapshot of the old indexed values. This is a
  quality-of-life improvement for a future version.
- **Schema evolution.** Tabulosity does not handle schema migration — adding or
  removing fields from row structs across versions is the user's responsibility.
  The recommended approach is to use serde's `#[serde(default)]` for new fields
  (so old serialized data loads with defaults) and `#[serde(deny_unknown_fields)]`
  or similar for strict mode. A future version could provide migration utilities,
  but for now the serde ecosystem handles the common cases well enough.
- **Unique indexes.** `#[indexed(unique)]` — enforced on insert/update.
- **Filtered indexes / partial indexes.** Index only rows matching a predicate.
- **Change tracking.** Tables emit a list of changes (insert/update/delete)
  per tick, useful for event-driven rendering.
