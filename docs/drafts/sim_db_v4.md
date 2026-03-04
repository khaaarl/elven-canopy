# Tabulosity — In-Memory Relational Store (Design Draft v4)

A lightweight, typed, in-memory relational database library for Rust. Designed
for game simulations and other domains where you want relational integrity
(primary keys, indexes, foreign keys) without the weight of SQL, an external
database engine, or ORM impedance mismatch.

Changes from v3: table mutation methods renamed to `_unchecked` suffix
(`insert_unchecked`, `update_unchecked`, `remove_unchecked`) to clearly signal
they bypass FK validation; Database struct restricted to table fields only (no
non-table fields allowed — wrap in a separate struct if needed); unified
`tabulosity::Error` enum replaces per-variant error types; cross-derive
information flow explained (TableMeta trait + post-expansion type checking);
ordering guarantees documented (`all`/`iter_all` return PK order); duplicate PK
detection on deserialize.

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
│       └── error.rs         # tabulosity::Error enum
├── tabulosity_derive/       # Proc macro crate (must be separate)
│   └── src/
│       └── lib.rs           # #[derive(Table)], #[derive(Database)], #[derive(Bounded)]
└── Cargo.toml               # Workspace
```

The proc macro crate (`tabulosity_derive`) is a compile-time dependency only.
Users interact with `tabulosity` and its derive macros.

## Unified Error Type

All fallible write operations return `Result<T, tabulosity::Error>`. A single
enum covers every failure mode:

```rust
/// All errors returned by tabulosity write operations.
#[derive(Debug, Clone)]
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

    /// Attempted to remove or update a row that is still referenced by
    /// foreign keys in other tables (restrict semantics).
    FkViolation {
        table: &'static str,
        key: String,
        /// Each entry: (referencing table name, FK field name, count of references).
        referenced_by: Vec<(&'static str, &'static str, usize)>,
    },
}
```

This works cleanly with `?` — callers can propagate errors without converting
between different error types:

```rust
fn relocate_creature(db: &mut MyDb, id: CreatureId, new_pos: VoxelCoord) -> Result<(), tabulosity::Error> {
    let mut creature = db.creatures.get(id).ok_or_else(|| tabulosity::Error::NotFound {
        table: "creatures",
        key: format!("{:?}", id),
    })?;
    creature.position = new_pos;
    db.update_creature(creature)?;
    Ok(())
}
```

## Read/Write Split

The central architectural decision is a clean split between reads and writes:

- **Reads go through the table structs directly.** `db.creatures.get(id)`,
  `db.creatures.all()`, `db.creatures.by_species(Species::Elf)`, etc. These are
  `&self` methods on the individual table struct.
- **Writes go through the Database struct.** `db.insert_creature(c)`,
  `db.update_creature(c)`, `db.remove_creature(id)`, `db.upsert_creature(c)`.
  These take `&mut self` on the Database, which lets them perform FK validation
  across tables before committing the mutation.

The table structs are public fields on the Database (for reads), but their
direct mutation methods use the `_unchecked` suffix to signal they bypass FK
validation — see the `_unchecked` Methods section below.

**Why this split?** Reads are the hot path in most simulations — you read far
more than you write. Routing reads through the table structs keeps the API
natural (`db.creatures.get(id)` rather than `db.get_creature(id)`). Writes need
the Database context for FK validation, so routing them through `&mut self` on
the Database is both correct and natural.

### Borrow Checker Strategy

The read/write split has a real limitation that must be understood clearly.

**Clone-based reads are the primary API.** Methods like `get`, `all`, `by_*`
return owned data (`Option<T>`, `Vec<T>`). The borrow is released as soon as
the method returns, so you can freely interleave cloned reads with writes:

```rust
// This works — clone releases the borrow before the write.
let creature = db.creatures.get(id).unwrap();
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
  so `by_assignee(None)` efficiently finds all unassigned tasks.

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
let unassigned: Vec<Task> = db.tasks.by_assignee(None);

// Removing the creature checks only Some(creature_id) entries —
// the None entry on TaskId(1) does not block deletion.
db.remove_creature(creature_id)?;  // Error: TaskId(2) still references it
```

### Defining the Database Schema

```rust
use tabulosity::Database;

#[derive(Database)]
pub struct MyDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "task")]
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
`state.db.creatures.get(id)`, writes through `state.db.insert_creature(c)`.

`CreatureTable` and `TaskTable` are generated by `#[derive(Table)]` on the
row structs. The `Database` derive:

1. Finds all fields and verifies each has a `#[table(singular = "...")]`
   annotation. Any unannotated field is a compile error.
2. Resolves foreign keys: `Task` declares `#[foreign_key(references = "creatures")]`
   on its `assignee` field. The derive matches `"creatures"` to the `creatures`
   field on `MyDb`, generating code like `self.creatures.contains(&task.assignee)`
   for FK validation.
3. Generates `new()` — creates all tables (empty). Since every field is a table,
   this is trivial and requires no `Default` bounds on anything.
4. Generates safe write methods (`insert_*`, `update_*`, `remove_*`, `upsert_*`)
   on the Database. These perform FK validation before delegating to the table's
   `_unchecked` methods.
5. Generates deserialization that rebuilds indexes and validates FKs.

**Singularization:** The `singular` attribute provides the name stem for
generated write methods. `#[table(singular = "creature")]` produces
`insert_creature`, `update_creature`, `remove_creature`, `upsert_creature`.
This avoids the proc macro needing to do English pluralization (which is
error-prone for words like "species", "data", "indices", etc.).

### `_unchecked` Methods on Table Structs

The `#[derive(Table)]` macro generates mutation methods directly on the
companion table struct, named with an `_unchecked` suffix:

```rust
// These are generated on CreatureTable:
impl CreatureTable {
    pub fn insert_unchecked(&mut self, row: Creature) -> Result<CreatureId, tabulosity::Error> { ... }
    pub fn update_unchecked(&mut self, row: Creature) -> Result<(), tabulosity::Error> { ... }
    pub fn remove_unchecked(&mut self, id: &CreatureId) -> Result<Creature, tabulosity::Error> { ... }
}
```

The `_unchecked` suffix is idiomatic Rust — just like `String::from_utf8_unchecked`
or `slice::get_unchecked`, it immediately signals "this skips safety checks."
In this case, the skipped checks are FK validation: these methods maintain
indexes correctly but do not verify that FK fields reference existing rows in
other tables, and do not check whether other tables still reference the row
being removed.

The `_unchecked` methods still return `Result` for non-FK errors:
`insert_unchecked` returns `Err(DuplicateKey)` if the PK already exists, and
`update_unchecked` / `remove_unchecked` return `Err(NotFound)` if the PK is
missing.

**When to use `_unchecked`:** Prefer the Database-level methods (`insert_creature`,
`update_creature`, `remove_creature`) for normal use — they perform FK validation
and keep your data consistent. The `_unchecked` methods exist for:

- **Bulk loading:** When populating a database from a trusted source (e.g.,
  deserialization), you can insert rows without per-row FK checks and validate
  all FKs in one pass at the end.
- **Internal use by the Database derive:** The generated Database-level methods
  perform FK validation themselves, then delegate to the `_unchecked` methods
  for the actual mutation.
- **Performance-critical paths:** When you can prove FK integrity is maintained
  by other means, `_unchecked` avoids redundant lookups.

### Writes (Through Database) — The Safe API

All normal mutations go through the Database struct, which takes `&mut self`:

```rust
let mut db = MyDb::new();

// Insert — caller provides explicit PK. Returns the PK on success.
let id = db.insert_creature(Creature {
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

// Update — PK extracted from struct, indexes updated, FK fields validated.
let mut creature = db.creatures.get(id).unwrap();
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
let removed: Creature = db.remove_creature(creature_id)?;
```

**Insert behavior:** Validates that all FK fields reference existing rows in the
target tables (skipping `None` for `Option<FK>`), then delegates to
`insert_unchecked`. Returns the primary key on success. If a row with the same
PK already exists, returns `Err(DuplicateKey { ... })`. Use `upsert_*` if you
want insert-or-update semantics.

**Update behavior:** Validates any changed FK fields against the referenced
tables, then delegates to `update_unchecked`. Extracts the PK from the struct,
looks up the existing row, compares each indexed field between old and new,
updates only the indexes whose values changed, then replaces the stored row.
Returns `Err(NotFound { ... })` if the PK doesn't exist.

**Upsert behavior:** If the PK exists, behaves like update (indexes maintained,
FK fields validated). If the PK doesn't exist, behaves like insert.

**Delete behavior:** Checks that no other table's FK index references the row
being deleted (restrict semantics), then delegates to `remove_unchecked`.
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

| Method                    | Returns                          | Behavior                              |
|---------------------------|----------------------------------|---------------------------------------|
| `insert_{row}(r)`        | `Result<PK, Error>`              | Insert new row. FK validated.         |
| `update_{row}(r)`        | `Result<(), Error>`              | Update existing row. FK validated.    |
| `upsert_{row}(r)`        | `Result<(), Error>`              | Insert or update. FK validated.       |
| `remove_{row}(pk)`       | `Result<RowStruct, Error>`       | Delete row. FK restrict checked.      |

All methods return `Result<T, tabulosity::Error>`, making `?` propagation
straightforward.

### Reads (Through Table Structs)

Reads go directly through the table fields on the Database:

```rust
// Primary key lookup — clone
let creature: Option<Creature> = db.creatures.get(creature_id);

// Primary key lookup — reference (perf optimization, borrows table)
let creature: Option<&Creature> = db.creatures.get_ref(creature_id);

// Existence check — lightweight, no cloning
let exists: bool = db.creatures.contains(creature_id);

// Row count
let total: usize = db.creatures.len();
let empty: bool = db.creatures.is_empty();

// All rows (returned in primary key order)
let all: Vec<Creature> = db.creatures.all();
for creature in db.creatures.iter_all() { /* &Creature, in PK order */ }

// Secondary index — equality
let elves: Vec<Creature> = db.creatures.by_species(Species::Elf);
for elf in db.creatures.iter_by_species(Species::Elf) { /* &Creature */ }

// Secondary index — range (only if field type implements Bounded)
let hungry: Vec<Creature> = db.creatures.by_hunger_range(50..);
for c in db.creatures.iter_by_hunger_range(..=20) { /* &Creature */ }

// Count (cheap — index scan only, no row lookups or cloning)
let n: usize = db.creatures.count_by_species(Species::Elf);
let m: usize = db.creatures.count_by_hunger_range(50..);
```

**Read method naming convention:**

| Pattern                      | Returns               | Notes                          |
|------------------------------|-----------------------|--------------------------------|
| `get(pk)`                    | `Option<T>`           | Clone by primary key           |
| `get_ref(pk)`                | `Option<&T>`          | Borrow by primary key          |
| `contains(pk)`               | `bool`                | Existence check, no cloning    |
| `len()`                      | `usize`               | Total row count                |
| `is_empty()`                 | `bool`                | Whether table has zero rows    |
| `all()`                      | `Vec<T>`              | Clone all rows, PK order       |
| `iter_all()`                 | `impl Iterator<&T>`   | Iterate all rows, PK order     |
| `by_field(val)`              | `Vec<T>`              | Clone matching rows            |
| `iter_by_field(val)`         | `impl Iterator<&T>`   | Iterate matching rows          |
| `by_field_range(r)`          | `Vec<T>`              | Clone rows in range            |
| `iter_by_field_range(r)`     | `impl Iterator<&T>`   | Iterate rows in range          |
| `count_by_field(val)`        | `usize`               | Count matching rows            |
| `count_by_field_range(r)`    | `usize`               | Count rows in range            |

**Ordering guarantee:** `all()` and `iter_all()` return rows in primary key
order. This is a property of the current BTree-based implementation — the
backing `BTreeMap` iterates in key order. Future alternative storage backends
(e.g., hash-based) would have unspecified order.

Clone-based reads (`get`, `all`, `by_*`) are safe to interleave with writes.
Iterator/reference-based reads (`get_ref`, `iter_all`, `iter_by_*`) borrow
the table and prevent mutation until released — use them for read-only scans.
See the Borrow Checker Strategy section above for details and examples.

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

To check whether a PK is still referenced: `count_by_{fk_field}(pk) > 0`.

For `Option<FK>` fields, the index stores `(Option<FkType>, PK)` tuples. When
checking restrict-on-delete, only `Some(target_pk)` entries are counted — `None`
entries are not references to any row. The generated restrict check looks like:

```rust
// For Option<CreatureId> FK field:
self.tasks.count_by_assignee(Some(creature_id)) > 0
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
   if two rows in the vec have the same PK, report an error rather than silently
   overwriting. Duplicates in serialized data indicate corruption or a bug in
   the serializer, and silently dropping rows would make such problems very hard
   to diagnose. All duplicate PKs across all tables are collected and reported
   together (don't fail on the first one).
3. Rebuild all secondary indexes by scanning every row.
4. Validate all FK constraints: for every FK field in every row, verify the
   referenced PK exists in the target table (skipping `None` values for
   `Option<FK>` fields). Collect all violations.
5. If any errors (duplicate PKs or FK violations) were found, return an error
   listing them all — report everything so problems can be fixed in one pass.

This means saved data is just rows — compact, human-readable (in JSON), and
indexes are always consistent with the data since they're rebuilt fresh.

## Proc Macro Implementation Notes

### `#[derive(Table)]`

Generates a companion struct `{RowName}Table` with:

- `rows: BTreeMap<PK, RowStruct>` (the primary store)
- `idx_{field}: BTreeSet<(FieldType, PK)>` for each `#[indexed]` field
  (including fields with `#[foreign_key]`, which implies `#[indexed]`)
- All read methods — these are `pub`:
  - `get`, `get_ref`, `contains`, `len`, `is_empty`
  - `all`, `iter_all`
  - `by_{field}`, `iter_by_{field}`, `count_by_{field}` for each indexed field
  - `by_{field}_range`, `iter_by_{field}_range`, `count_by_{field}_range` for
    each indexed field whose type implements `Bounded`
- Mutation methods with `_unchecked` suffix — these are `pub`:
  - `insert_unchecked`: inserts a row and updates all indexes. Returns
    `Err(DuplicateKey)` if PK exists. Does not validate FK fields.
  - `update_unchecked`: updates a row and maintains indexes. Returns
    `Err(NotFound)` if PK missing. Does not validate FK fields.
  - `remove_unchecked`: removes a row and cleans up indexes. Returns
    `Err(NotFound)` if PK missing. Does not check for incoming FK references.
- `rebuild_indexes()` for deserialization
- A `TableMeta` trait impl encoding metadata (see Cross-Derive Information Flow)
- A `validate_fks` method for post-load FK validation

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

### `#[derive(Database)]`

Reads the schema struct, verifies every field has `#[table(singular = "...")]`,
and:

1. **Rejects non-table fields.** Any field without `#[table(singular = "...")]`
   is a compile error. This keeps the Database struct focused on relational data.
2. **Resolves FKs.** For each table with `#[foreign_key(references = "...")]`
   fields, matches the `references` string to a Database field name. If the
   string doesn't match any table field, it's a compile-time error.
3. **Generates `new()`.** Creates all tables (empty). Since every field is a
   table and every table starts empty, this requires no trait bounds or user
   input.
4. **Generates safe write methods.** For each table with singular name `foo`,
   generates `insert_foo`, `update_foo`, `upsert_foo`, and `remove_foo` methods
   on the Database. These methods perform FK validation (checking referenced
   tables for insert/update, checking referencing tables for remove), then
   delegate to the table's `_unchecked` methods for the actual mutation.
5. **Generates deserialization** that rebuilds indexes, detects duplicate PKs,
   and validates FKs.

### Cross-Derive Information Flow

The Table derive and Database derive are separate proc macros that run
independently. They need to share information — for example, the Database derive
needs to know which fields in a Task are foreign keys and which tables they
reference. Here is how this works:

**Step 1: The Table derive generates a `TableMeta` trait impl.** When
`#[derive(Table)]` runs on the `Task` struct, it generates the `TaskTable`
companion type and also implements a `TableMeta` trait on it. This trait
encodes metadata about the table as associated types and methods:

```rust
// Generated by #[derive(Table)] on Task:
impl TableMeta for TaskTable {
    type PK = TaskId;
    type Row = Task;

    fn fk_fields() -> &'static [FkFieldInfo] {
        &[FkFieldInfo {
            field_name: "assignee",
            references: "creatures",
        }]
    }

    fn validate_fks(&self, /* references to other tables */) -> Vec<tabulosity::Error> {
        // Check every row's assignee exists in the creatures table
        ...
    }
}
```

**Step 2: The Database derive generates code that uses `TableMeta`.** When
`#[derive(Database)]` runs on `MyDb`, it sees the field names and the
`#[table(singular = "...")]` attributes. It generates code that calls methods
from the `TableMeta` trait — for example, to validate FKs on insert, it
generates something like `self.creatures.contains(&task.assignee)`.

**Step 3: The Rust compiler connects everything.** After both macros have
expanded, the generated code goes through normal Rust compilation — type
checking, trait resolution, borrow checking. If a `references = "creatures"`
string doesn't match a real field, or the FK type doesn't match the referenced
table's PK type, the compiler reports a regular type error on the generated
code. The proc macros themselves never need to resolve types or inspect other
structs — they just generate code that the compiler then validates.

This is the key insight: **the two macros communicate through generated Rust
code, not through direct macro-to-macro communication.** The Table derive
produces trait impls, the Database derive produces code that uses those impls,
and the Rust compiler makes sure everything fits together. Each macro only
needs to read the tokens on the struct it's applied to.

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

- A `Bounded` trait defined in the `tabulosity` crate.
- Blanket impls for common types (`u8`, `u16`, `u32`, `u64`, `i8`, `i16`,
  `i32`, `i64`, `bool`, etc.).
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
  from v1 to keep the initial implementation simple.
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
