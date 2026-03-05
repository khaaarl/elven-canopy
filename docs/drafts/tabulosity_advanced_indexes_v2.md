# Tabulosity — Compound and Filtered Indexes (Design Draft v2)

This is v2 of the compound/filtered indexes design. The major change from v1:
**tracked bounds replace the `Bounded` trait** in all index machinery. Instead
of requiring static `MIN`/`MAX` constants on every type participating in an
index, each unique type gets a runtime `Option<(T, T)>` pair on the companion
struct that tracks the lowest and highest values ever seen. This enables
`String`, custom enums, and any `Ord + Clone` type to participate in compound
indexes and as primary keys without implementing `Bounded`.

## Goals

- **Multi-field indexes.** A single BTreeSet storing composite tuples, enabling
  efficient lookups by leading field prefix and range scans on any prefix.
- **Predicate-based filtering.** Only rows matching a user-defined predicate are
  included in the index. Useful for "active tasks", "living creatures", or any
  subset query that would otherwise scan the full table.
- **Composable.** A single `#[index(...)]` attribute can declare a compound
  index, a filtered index, or both. No separate mechanisms.
- **Unified query API.** One query method per index, with each field parameter
  accepting exact values, ranges, or `Any` via an `IntoQuery` trait. No
  proliferation of `_prefix` / `_range` / `_exact` method variants.
- **Leftmost prefix semantics.** Same mental model as database compound indexes:
  leading exact/range fields use the index efficiently; once `Any` appears,
  remaining fields are post-filtered.
- **No `Bounded` requirement.** Field types and PK types need only `Ord + Clone`.
  Tracked bounds handle the rest at runtime.

## Non-Goals (v2)

- Unique compound indexes (enforce uniqueness on compound key).
- Zero-copy queries (avoiding clones when constructing composite tuple bounds).
- Inline closures in attributes (Rust doesn't support them).
- Joins or cross-table compound indexes.

## Tracked Bounds

### Motivation

Compound index queries need MIN/MAX-like sentinel values to construct BTreeSet
range bounds. For example, a prefix query `by_foo(&a, Any)` on a
`BTreeSet<(A, B, PK)>` needs to scan from `(a, B_min, PK_min)` to
`(a, B_max, PK_max)`. In v1, this required `B: Bounded` and `PK: Bounded` —
types with static `MIN`/`MAX` constants. This excluded `String`, custom enums
without manual `Bounded` impls, and any type without a known static range.

Tracked bounds solve this by recording the actual minimum and maximum values
seen at runtime. For the purpose of index range scans, the tracked min/max are
sufficient: every value in the BTreeSet was inserted at some point, so the
tracked bounds are guaranteed to cover all values present in the index.

### Mechanism

Every unique type that appears in any index position (including the PK as the
tiebreaker) gets ONE tracked bounds pair on the companion struct:

```rust
_bounds_{type_name}: Option<(T, T)>
```

Where `{type_name}` is a stable, sanitized form of the type (e.g., `u8`,
`option_creature_id`, `string`). The naming scheme ensures that if multiple
fields or indexes use the same type, they share one tracked bounds pair.

**Key rules:**

- **Per unique type, not per field or per index.** If two `u8` fields exist in
  different indexes, there is one `_bounds_u8: Option<(u8, u8)>`.
- **`Option<CreatureId>` and `CreatureId` are different types** and get separate
  tracked bounds.
- **`Option<(T, T)>`**: `None` means no values have ever been seen (empty table
  or no inserts yet). `Some((min, max))` contains the smallest and largest
  values ever observed.
- **On every insert/update**, each field value is compared against its type's
  tracked bounds and widened if necessary. This is O(1) per field per mutation.
- **Tracked bounds never shrink.** They only grow outward ("ever seen"
  semantics). Removing a row does NOT cause the bounds to contract, even if
  the removed row held the current min or max. This is safe because the bounds
  are only used to construct range scan endpoints — a wider-than-necessary range
  simply means the BTreeSet range scan may start/end slightly beyond the actual
  data, which is harmless (the scan finds no entries in the gap).
- **On deserialization**, `rebuild_indexes` recomputes tracked bounds from all
  current row data (effectively a fresh "ever seen" starting from the current
  data).
- **When tracked bounds are `None`** (no rows ever inserted), queries that need
  bounds (prefix scans, `Any`) return empty immediately — there is nothing in
  the index.

### Generated Code

For a table with indexes involving types `u8`, `Option<CreatureId>`, and
`TaskId` (PK):

```rust
pub struct TaskTable {
    rows: BTreeMap<TaskId, Task>,
    // Simple indexes...
    idx_assignee: BTreeSet<(Option<CreatureId>, TaskId)>,
    // Compound indexes...
    idx_active_priority: BTreeSet<(Option<CreatureId>, u8, TaskId)>,
    // Tracked bounds — one per unique type across all indexes
    _bounds_u8: Option<(u8, u8)>,
    _bounds_option_creature_id: Option<(Option<CreatureId>, Option<CreatureId>)>,
    _bounds_task_id: Option<(TaskId, TaskId)>,
}
```

### Widening on Mutation

Generated code for insert (and the insert path of upsert):

```rust
// Widen tracked bounds for each field value and the PK.
match &mut self._bounds_option_creature_id {
    Some((lo, hi)) => {
        if row.assignee < *lo { *lo = row.assignee.clone(); }
        if row.assignee > *hi { *hi = row.assignee.clone(); }
    }
    None => {
        self._bounds_option_creature_id = Some((row.assignee.clone(), row.assignee.clone()));
    }
}
match &mut self._bounds_u8 {
    Some((lo, hi)) => {
        if row.priority < *lo { *lo = row.priority.clone(); }
        if row.priority > *hi { *hi = row.priority.clone(); }
    }
    None => {
        self._bounds_u8 = Some((row.priority.clone(), row.priority.clone()));
    }
}
match &mut self._bounds_task_id {
    Some((lo, hi)) => {
        if row.id < *lo { *lo = row.id.clone(); }
        if row.id > *hi { *hi = row.id.clone(); }
    }
    None => {
        self._bounds_task_id = Some((row.id.clone(), row.id.clone()));
    }
}
```

For update, the same widening runs on the new row's values. Since bounds never
shrink, we only need to check the new values — the old values are already
reflected in the bounds.

### Rebuild on Deserialization

```rust
fn rebuild_indexes(&mut self) {
    // Reset tracked bounds.
    self._bounds_u8 = None;
    self._bounds_option_creature_id = None;
    self._bounds_task_id = None;

    // Rebuild indexes and recompute tracked bounds from current data.
    self.idx_assignee.clear();
    self.idx_active_priority.clear();
    for (pk, row) in &self.rows {
        // Widen tracked bounds (same logic as insert).
        // ...

        // Rebuild simple indexes.
        self.idx_assignee.insert((row.assignee.clone(), pk.clone()));

        // Rebuild compound/filtered indexes.
        if Task::is_active(row) {
            self.idx_active_priority.insert((
                row.assignee.clone(),
                row.priority,
                pk.clone(),
            ));
        }
    }
}
```

### Type Name Sanitization

The proc macro must produce a stable, valid Rust identifier from each type for
the `_bounds_{type_name}` field. Rules:

- Primitive types map directly: `u8` -> `_bounds_u8`, `bool` -> `_bounds_bool`.
- Newtypes use their name in snake_case: `TaskId` -> `_bounds_task_id`,
  `CreatureId` -> `_bounds_creature_id`.
- `Option<T>` -> `_bounds_option_{sanitized_T}`: `Option<CreatureId>` ->
  `_bounds_option_creature_id`.
- Path segments are joined with `_`: `crate::types::ZoneId` -> `_bounds_zone_id`
  (use the last segment only).

The proc macro collects all unique types across all index fields and the PK,
deduplicates them, and generates one `_bounds_*` field per unique type.

## Attribute Syntax

### The `#[index(...)]` Struct-Level Attribute

Compound and filtered indexes are declared on the struct deriving `Table`, not
on individual fields:

```rust
#[derive(Table, Clone, Debug)]
#[index(name = "active_priority", fields("assignee", "priority"), filter = "Task::is_active")]
struct Task {
    #[primary_key]
    id: TaskId,
    #[indexed]                          // simple single-field index (sugar, see below)
    assignee: Option<CreatureId>,
    #[indexed]
    status: TaskStatus,
    priority: u8,
}

impl Task {
    fn is_active(&self) -> bool {
        matches!(self.status, TaskStatus::Pending | TaskStatus::InProgress)
    }
}
```

**Parameters:**

- `name` (required): identifier used for the generated query methods and the
  internal index field. Produces `by_{name}`, `iter_by_{name}`, `count_by_{name}`
  methods and an `idx_{name}` field on the companion table struct.
- `fields(...)` (required): ordered list of field names forming the composite
  key. Can be 1 field (filtered-only index), 2+ fields (compound index), or 2+
  fields with a filter (compound + filtered).
- `filter` (optional): path to a function `fn(&Row) -> bool`. If present, only
  rows where the filter returns true are stored in the index.

Multiple `#[index(...)]` attributes can appear on a single struct. A field can
appear in multiple compound indexes and/or have its own `#[indexed]`. The
indexes are completely independent — each has its own BTreeSet storage.

**Naming convention:** Since generated methods are `by_{name}`, avoid names
that start with `by_` to prevent stutter (e.g., name the index
`"active_priority"` not `"active_by_priority"`, producing `by_active_priority`
not `by_active_by_priority`).

### `#[indexed]` as Sugar

The field-level `#[indexed]` attribute is sugar for a struct-level
`#[index(...)]` declaration. Specifically, `#[indexed]` on a field named
`assignee` generates code that behaves identically to:

```rust
#[index(name = "assignee", fields("assignee"))]
```

Same tracked bounds mechanism, same generated code structure, same query methods
(`by_assignee`, `iter_by_assignee`, `count_by_assignee`). The sugar form exists
for convenience — most single-field indexes don't need the full attribute syntax.

This means the existing simple index code paths (using `map_start_bound` /
`map_end_bound` with `Bounded`) are replaced by the new uniform tracked bounds
mechanism. The `by_{field}_range`, `iter_by_{field}_range`, and
`count_by_{field}_range` methods generated by v8's `#[indexed]` are subsumed by
the unified `IntoQuery`-based API — `by_assignee(3..7)` replaces
`by_assignee_range(3..7)`.

Having `#[indexed]` on a field and that field appearing in an `#[index(...)]`
are independent — they produce separate indexes with separate storage.

**Example — a field in both a simple index and a compound index:**

```rust
#[derive(Table, Clone, Debug)]
#[index(name = "species_hunger", fields("species", "hunger"))]
struct Creature {
    #[primary_key]
    id: CreatureId,
    #[indexed]                  // simple index: idx_species: BTreeSet<(Species, CreatureId)>
    species: Species,           // needs Ord + Clone only
    #[indexed]                  // simple index: idx_hunger: BTreeSet<(u32, CreatureId)>
    hunger: u32,
}
```

This struct gets three indexes:
- `idx_species` — single-field, from `#[indexed]`
- `idx_hunger` — single-field, from `#[indexed]`
- `idx_species_hunger` — compound, from `#[index(...)]`

The single-field indexes support `by_species`, `iter_by_species`,
`count_by_species` with `IntoQuery`-based parameters. The compound index
supports `by_species_hunger` with the same unified query API.

### Filtered-Only Index (Single Field + Filter)

A filter with a single field creates a filtered index — useful when you
frequently query a subset:

```rust
#[derive(Table, Clone, Debug)]
#[index(name = "active_assignee", fields("assignee"), filter = "Task::is_active")]
struct Task {
    #[primary_key]
    id: TaskId,
    #[indexed]
    assignee: Option<CreatureId>,
    status: TaskStatus,
    priority: u8,
}
```

`by_active_assignee(&Some(creature_id))` returns only active tasks assigned to
that creature, scanning a smaller index than the full `by_assignee` simple index.
`count_by_active_assignee(&Some(creature_id))` counts them without cloning.

## The `IntoQuery` Trait

Instead of generating separate `_prefix`, `_range`, and `_exact` method
variants for each field combination, compound indexes have ONE query method per
index. Each field parameter accepts anything implementing `IntoQuery<FieldType>`.

### Core Types

```rust
/// Unit struct that matches all values for a field in a compound query.
pub struct Any;

/// The resolved form of a query parameter for a single field.
pub enum QueryBound<T> {
    /// Match exactly this value.
    Exact(T),
    /// Match values in this range.
    Range { start: Bound<T>, end: Bound<T> },
    /// Match all values (no constraint on this field).
    Any,
}

/// Trait for converting query parameters into QueryBound.
pub trait IntoQuery<T> {
    fn into_query(self) -> QueryBound<T>;
}
```

### Blanket Implementations

```rust
// Exact match from a reference — the most common case.
impl<T: Clone> IntoQuery<T> for &T {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Exact(self.clone())
    }
}

// Range types -> QueryBound::Range with appropriate bounds.
impl<T: Clone> IntoQuery<T> for Range<T> {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: Bound::Included(self.start),
            end: Bound::Excluded(self.end),
        }
    }
}

impl<T: Clone> IntoQuery<T> for RangeInclusive<T> {
    fn into_query(self) -> QueryBound<T> {
        let (start, end) = self.into_inner();
        QueryBound::Range {
            start: Bound::Included(start),
            end: Bound::Included(end),
        }
    }
}

impl<T: Clone> IntoQuery<T> for RangeFrom<T> {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: Bound::Included(self.start),
            end: Bound::Unbounded,
        }
    }
}

impl<T: Clone> IntoQuery<T> for RangeTo<T> {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: Bound::Unbounded,
            end: Bound::Excluded(self.end),
        }
    }
}

impl<T: Clone> IntoQuery<T> for RangeToInclusive<T> {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: Bound::Unbounded,
            end: Bound::Included(self.end),
        }
    }
}

impl<T> IntoQuery<T> for RangeFull {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Any
    }
}

// The Any unit struct — ergonomic shorthand for "no constraint".
impl<T> IntoQuery<T> for Any {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Any
    }
}
```

### Usage Examples

```rust
// Exact match on both fields.
table.by_species_hunger(&Species::Elf, &30)

// Exact match on first field, range on second.
table.by_species_hunger(&Species::Elf, 50..100)

// Prefix query — exact first field, all values for second.
table.by_species_hunger(&Species::Elf, Any)

// Range on first field, all values for second.
// Note: range ordering depends on the Ord derive (variant declaration order).
table.by_species_hunger(Species::Elf..Species::Capybara, Any)

// All rows in the index (useful for filtered indexes — returns only matching rows).
table.by_active_priority(Any, Any)

// Skip on first field, exact on second — works but O(n) post-filter.
table.by_species_hunger(Any, &30)
```

`RangeFull` (`..`) also works anywhere `Any` does, since both map to
`QueryBound::Any`. The `Any` struct exists for readability — `table.by_foo(Any, &3)`
reads more clearly than `table.by_foo(.., &3)`.

## Generated Methods

Each `#[index(name = "foo", fields("a", "b"))]` generates three methods on the
companion table struct:

```rust
impl TaskTable {
    /// Returns cloned rows matching the query (releases borrow immediately).
    pub fn by_foo(
        &self,
        a: impl IntoQuery<AType>,
        b: impl IntoQuery<BType>,
    ) -> Vec<Task> { ... }

    /// Returns an iterator over references to matching rows (borrows table).
    pub fn iter_by_foo(
        &self,
        a: impl IntoQuery<AType>,
        b: impl IntoQuery<BType>,
    ) -> impl Iterator<Item = &Task> { ... }

    /// Returns the count of matching rows (no cloning, no row lookups).
    pub fn count_by_foo(
        &self,
        a: impl IntoQuery<AType>,
        b: impl IntoQuery<BType>,
    ) -> usize { ... }
}
```

These follow the same naming convention as simple indexes (`by_*`, `iter_by_*`,
`count_by_*`), but with `impl IntoQuery<T>` parameters instead of `&T` or range
bounds.

**`iter_by_*` lifetime note:** The returned iterator borrows `self` (for the
BTreeSet and the rows BTreeMap). When post-filtering is needed, the filter
closure must use `move` to take ownership of the resolved `QueryBound` values,
which are local variables. The `by_*` method (returning `Vec`) calls
`.collect()` inside, so the iterator doesn't escape and `move` is optional.
The `count_by_*` method similarly consumes the iterator immediately.

**`count_by_*` implementation note:** The count method iterates over index
entries (composite tuples) and post-filters on tuple fields directly — it never
looks up the full row. This means `count_by_*` has no cloning overhead
regardless of the query pattern. For filtered indexes, all entries in the index
already passed the filter, so no re-evaluation is needed.

## Internal Storage

### Compound Index Structure

For a compound index on fields (F1, F2) with primary key PK:

```
idx_foo: BTreeSet<(F1Type, F2Type, PKType)>
```

For N fields:

```
idx_foo: BTreeSet<(F1Type, F2Type, ..., FNType, PKType)>
```

The PK is always appended as the final tuple element, serving as a tiebreaker
for rows that share the same compound key values. This is the same pattern as
simple indexes (`BTreeSet<(FieldType, PK)>`), extended to N fields.

### Filtered Index Structure

A filtered index uses the same BTreeSet type. It simply contains fewer entries —
only rows where the filter predicate returns true. The filter does not change the
storage type, only the set of rows indexed.

### Example — Generated Companion Struct

For the `Task` struct with `#[indexed]` on `assignee` and `status`, and
`#[index(name = "active_priority", fields("assignee", "priority"), filter = "Task::is_active")]`:

```rust
// Generated by #[derive(Table)]:
pub struct TaskTable {
    rows: BTreeMap<TaskId, Task>,
    // Simple indexes (from #[indexed], which is sugar for single-field #[index])
    idx_assignee: BTreeSet<(Option<CreatureId>, TaskId)>,
    idx_status: BTreeSet<(TaskStatus, TaskId)>,
    // Compound index (from #[index(...)])
    idx_active_priority: BTreeSet<(Option<CreatureId>, u8, TaskId)>,
    // Tracked bounds — one per unique type across all indexes
    _bounds_u8: Option<(u8, u8)>,
    _bounds_option_creature_id: Option<(Option<CreatureId>, Option<CreatureId>)>,
    _bounds_task_id: Option<(TaskId, TaskId)>,
    _bounds_task_status: Option<(TaskStatus, TaskStatus)>,
}
```

Note: `TaskStatus` appears in `idx_status` (from `#[indexed]`), so it gets its
own tracked bounds. `Option<CreatureId>` appears in both `idx_assignee` and
`idx_active_priority`, but gets only ONE tracked bounds pair (shared by type).

## Query Implementation — Leftmost Prefix Rule

The BTreeSet stores composite tuples sorted lexicographically. Query efficiency
depends on which fields have concrete constraints (Exact or Range) vs. `Any`:

- **Leading Exact fields** lock in specific values, narrowing the BTreeSet range.
- **A Range field** after leading Exact fields produces a bounded index scan.
- **Once `Any` appears**, that field and all subsequent fields are unconstrained
  in the index scan. If later fields have concrete constraints, they become
  post-filters applied to each row after retrieval.

This matches how database compound indexes work. The leftmost prefix of concrete
constraints determines the efficiency of the scan.

### Efficiency Table

For a 2-field compound index `(A, B, PK)`:

| Query                     | A constraint | B constraint | Behavior                     |
|---------------------------|-------------|-------------|-------------------------------|
| `by_foo(&a, &b)`         | Exact       | Exact       | Point lookup, O(log n)        |
| `by_foo(&a, 1..5)`       | Exact       | Range       | Range scan, O(log n + k)      |
| `by_foo(&a, Any)`         | Exact       | Any         | Prefix scan, O(log n + k)     |
| `by_foo(1..5, &b)`       | Range       | Exact       | Range scan + post-filter      |
| `by_foo(1..5, Any)`       | Range       | Any         | Range scan, O(log n + k)      |
| `by_foo(Any, &b)`         | Any         | Exact       | Full index scan + post-filter |
| `by_foo(Any, Any)`         | Any         | Any         | Full index scan               |

Users who need fast lookups on a non-leading field should add a separate
`#[indexed]` on that field or define another compound index with a different
field order.

### Composite Bound Construction

The query implementation constructs BTreeSet range bounds from the resolved
`QueryBound` values, using tracked bounds instead of static `Bounded::MIN/MAX`.

**Leading Exact fields** contribute fixed values to both the start and end bound
of the composite tuple. **A Range or Any field** contributes its bounds (or
tracked min/max for `Any` and `Unbounded`). **Trailing fields after `Any`** use
tracked min/max for the index scan, then results are post-filtered.

When tracked bounds are `None` for any type needed in a query that requires
bounds (i.e., the table has never seen a value of that type), the query returns
empty immediately.

Example — `by_foo(&some_assignee, 3..=7)` on
`idx_active_priority: BTreeSet<(Option<CreatureId>, u8, TaskId)>`:

```rust
// Tracked bounds provide the PK range.
let (pk_min, pk_max) = self._bounds_task_id.as_ref()?;  // None -> return empty

// Composite start bound:
Included((Some(creature_1), 3, pk_min.clone()))

// Composite end bound:
Included((Some(creature_1), 7, pk_max.clone()))
```

Example — `by_foo(&some_assignee, Any)` (prefix query):

```rust
let (u8_min, u8_max) = self._bounds_u8.as_ref()?;
let (pk_min, pk_max) = self._bounds_task_id.as_ref()?;

// Composite start bound:
Included((Some(creature_1), u8_min.clone(), pk_min.clone()))

// Composite end bound:
Included((Some(creature_1), u8_max.clone(), pk_max.clone()))
```

Example — `by_foo(Any, &3)` (non-leading exact — post-filter):

```rust
// Scan the entire index (no bounds needed for range construction):
self.idx_active_priority.iter()

// Then post-filter each result:
.filter(|(_, priority, _)| *priority == 3)
```

### Range Followed by Non-Any

When a Range field is followed by a non-Any field, the trailing field cannot
narrow the index scan — it becomes a post-filter. This is because the BTreeSet
range covers all values of the range field, and within each value, all
combinations of subsequent fields.

Example — `by_foo(1..5, &b)` on `(A, B, PK)`:

```rust
let (b_min, _) = self._bounds_b_type.as_ref()?;
let (pk_min, _) = self._bounds_pk_type.as_ref()?;

// Index scan: A in [1, 5), all B, all PK
let start = Included((1, b_min.clone(), pk_min.clone()));
let end = Excluded((5, b_min.clone(), pk_min.clone()));

// Post-filter: keep only rows where B == b
.filter(|(_, b_val, _)| *b_val == b)
```

Example — `by_foo(1..=5, &b)` on `(A, B, PK)` (inclusive end):

```rust
let (b_min, b_max) = self._bounds_b_type.as_ref()?;
let (pk_min, pk_max) = self._bounds_pk_type.as_ref()?;

// Index scan: A in [1, 5], all B, all PK
let start = Included((1, b_min.clone(), pk_min.clone()));
let end = Included((5, b_max.clone(), pk_max.clone()));

// Post-filter: keep only rows where B == b
.filter(|(_, b_val, _)| *b_val == b)
```

This is correct but less efficient than having B as the leading field. Users
should order compound index fields to match their most common query patterns.

### Composite Bound Construction — Formal Rules

The rules for constructing composite bounds follow a consistent pattern,
generalized to N+1 tuples (N fields + PK). The proc macro generates inline
bound construction since the tuple arity varies per index.

**Key insight:** When constructing composite BTreeSet bounds, the goal is to
produce a tuple range that includes exactly the rows that could match the
leading concrete constraints, then post-filter the rest.

**Notation:** `tracked_min(T)` and `tracked_max(T)` refer to the first and
second elements of the `_bounds_{type}` pair for type `T`. If the bounds are
`None`, the query returns empty immediately.

**Start bound rules** (for each field position left-to-right):

| Field query         | Start bound contribution                    |
|---------------------|---------------------------------------------|
| `Exact(v)`          | `v.clone()` (locked)                        |
| `Range(Included(v))`| `v.clone()` with `Included` wrapper         |
| `Range(Excluded(v))`| `v.clone()` with `Excluded` wrapper         |
| `Range(Unbounded)`  | Use `tracked_min(F)`                        |
| `Any`               | Use `tracked_min(F)`                        |

**End bound rules** (for each field position left-to-right):

| Field query         | End bound contribution                      |
|---------------------|---------------------------------------------|
| `Exact(v)`          | `v.clone()` (locked)                        |
| `Range(Included(v))`| `v.clone()` with `Included` wrapper         |
| `Range(Excluded(v))`| `v.clone()` with `Excluded` wrapper         |
| `Range(Unbounded)`  | Use `tracked_max(F)`                        |
| `Any`               | Use `tracked_max(F)`                        |

**Bound wrapper rules for the composite tuple:**

The outermost `Bound<(...)>` wrapper (i.e., `Included(tuple)` vs
`Excluded(tuple)`) is determined by the **first non-Exact field**:

- All fields Exact: wrapper is `Included` (both start and end).
- First non-Exact has `Included` start -> composite start is `Included`.
- First non-Exact has `Excluded` start -> composite start is `Excluded`.
- Same logic for end bound.
- `Any` fields: start wrapper is `Included` (with tracked_min), end wrapper is
  `Included` (with tracked_max).

**Trailing field padding after the boundary field:**

- Start bound trailing fields: `tracked_min(F)` for each field, then
  `tracked_min(PK)`.
- End bound trailing fields: `tracked_max(F)` for each field, then
  `tracked_max(PK)`.

This ensures the composite range captures all possible trailing combinations.

**The `Excluded` bound subtlety:** For `Excluded(v)` on a non-trailing field,
the composite bound must use the opposite padding to exclude correctly:

- `Excluded` **start** bound on field i: pad trailing fields with
  `tracked_max`, not `tracked_min`. Because
  `Excluded((5, tracked_max(F), tracked_max(PK)))` as a start bound means
  "start after the very last tuple beginning with 5", which is equivalent to
  "start from 6 and above" (assuming integer types).
- `Excluded` **end** bound on field i: pad trailing fields with
  `tracked_min`, not `tracked_max`. Because
  `Excluded((5, tracked_min(F), tracked_min(PK)))` as an end bound means
  "end before the very first tuple beginning with 5", which excludes all
  tuples starting with 5.

**Complete examples:**

`by_foo(1..5, &b)` on `(A, B, PK)`:
```rust
let (b_min, _) = self._bounds_b_type.as_ref()?;
let (pk_min, _) = self._bounds_pk_type.as_ref()?;

start: Included((1, b_min.clone(), pk_min.clone()))  // A=1 inclusive, pad trailing with tracked_min
end:   Excluded((5, b_min.clone(), pk_min.clone()))  // A=5 excluded, pad trailing with tracked_min
// post-filter: B == b
```

`by_foo(1..=5, &b)` on `(A, B, PK)`:
```rust
let (b_min, b_max) = self._bounds_b_type.as_ref()?;
let (pk_min, pk_max) = self._bounds_pk_type.as_ref()?;

start: Included((1, b_min.clone(), pk_min.clone()))  // A=1 inclusive, pad trailing with tracked_min
end:   Included((5, b_max.clone(), pk_max.clone()))  // A=5 inclusive, pad trailing with tracked_max
// post-filter: B == b
```

`by_foo(&a, 3..7)` on `(A, B, PK)`:
```rust
let (pk_min, _) = self._bounds_pk_type.as_ref()?;

start: Included((a, 3, pk_min.clone()))  // A=a exact, B=3 inclusive
end:   Excluded((a, 7, pk_min.clone()))  // A=a exact, B=7 excluded, pad PK with tracked_min
```

`by_foo(&a, 3..=7)` on `(A, B, PK)`:
```rust
let (pk_min, pk_max) = self._bounds_pk_type.as_ref()?;

start: Included((a, 3, pk_min.clone()))  // A=a exact, B=3 inclusive
end:   Included((a, 7, pk_max.clone()))  // A=a exact, B=7 inclusive, pad PK with tracked_max
```

### Helper Functions

The existing `map_start_bound` and `map_end_bound` helpers (which require
`PK: Bounded`) will be retired when this is implemented. They are replaced by
inline bound construction in the generated code, using tracked bounds.

A post-filter helper `in_bounds` is needed for checking whether a value falls
within a `QueryBound::Range`:

```rust
/// Returns true if `val` falls within the given bounds.
#[doc(hidden)]
pub fn in_bounds<T: Ord>(val: &T, start: &Bound<T>, end: &Bound<T>) -> bool {
    let start_ok = match start {
        Bound::Included(s) => val >= s,
        Bound::Excluded(s) => val > s,
        Bound::Unbounded => true,
    };
    let end_ok = match end {
        Bound::Included(e) => val <= e,
        Bound::Excluded(e) => val < e,
        Bound::Unbounded => true,
    };
    start_ok && end_ok
}
```

This will live in `tabulosity/src/table.rs` alongside the existing helpers
(which remain available for backward compatibility but are no longer used by
generated index code).

### Generated Query Code (Sketch)

For `#[index(name = "active_priority", fields("assignee", "priority"), filter = "Task::is_active")]`:

```rust
pub fn by_active_priority(
    &self,
    assignee: impl IntoQuery<Option<CreatureId>>,
    priority: impl IntoQuery<u8>,
) -> Vec<Task> {
    let a = assignee.into_query();
    let b = priority.into_query();

    match (&a, &b) {
        // Both Exact — point lookup (no tracked bounds needed for scan)
        (QueryBound::Exact(a_val), QueryBound::Exact(b_val)) => {
            // Still need PK bounds for the tiebreaker range.
            let Some((pk_min, pk_max)) = &self._bounds_task_id else {
                return Vec::new();
            };
            let start = (a_val.clone(), b_val.clone(), pk_min.clone());
            let end = (a_val.clone(), b_val.clone(), pk_max.clone());
            self.idx_active_priority
                .range(start..=end)
                .map(|(_, _, pk)| self.rows[pk].clone())
                .collect()
        }

        // Exact + Range — bounded index scan
        (QueryBound::Exact(a_val), QueryBound::Range { start: bs, end: be }) => {
            let Some((pk_min, pk_max)) = &self._bounds_task_id else {
                return Vec::new();
            };
            // For Unbounded, we need u8 tracked bounds too.
            let u8_bounds = &self._bounds_u8;

            let composite_start = match bs {
                Bound::Included(v) => Bound::Included((a_val.clone(), v.clone(), pk_min.clone())),
                Bound::Excluded(v) => Bound::Excluded((a_val.clone(), v.clone(), pk_max.clone())),
                Bound::Unbounded => {
                    let Some((u8_min, _)) = u8_bounds else {
                        return Vec::new();
                    };
                    Bound::Included((a_val.clone(), u8_min.clone(), pk_min.clone()))
                }
            };
            let composite_end = match be {
                Bound::Included(v) => Bound::Included((a_val.clone(), v.clone(), pk_max.clone())),
                Bound::Excluded(v) => Bound::Excluded((a_val.clone(), v.clone(), pk_min.clone())),
                Bound::Unbounded => {
                    let Some((_, u8_max)) = u8_bounds else {
                        return Vec::new();
                    };
                    Bound::Included((a_val.clone(), u8_max.clone(), pk_max.clone()))
                }
            };
            self.idx_active_priority
                .range((composite_start, composite_end))
                .map(|(_, _, pk)| self.rows[pk].clone())
                .collect()
        }

        // Exact + Any — prefix scan
        (QueryBound::Exact(a_val), QueryBound::Any) => {
            let Some((u8_min, u8_max)) = &self._bounds_u8 else {
                return Vec::new();
            };
            let Some((pk_min, pk_max)) = &self._bounds_task_id else {
                return Vec::new();
            };
            let start = (a_val.clone(), u8_min.clone(), pk_min.clone());
            let end = (a_val.clone(), u8_max.clone(), pk_max.clone());
            self.idx_active_priority
                .range(start..=end)
                .map(|(_, _, pk)| self.rows[pk].clone())
                .collect()
        }

        // Any on first field — scan everything, post-filter second field
        (QueryBound::Any, _) => {
            self.idx_active_priority
                .iter()
                .filter(|(_, b_val, _)| match &b {
                    QueryBound::Exact(v) => b_val == v,
                    QueryBound::Range { start, end } => in_bounds(b_val, start, end),
                    QueryBound::Any => true,
                })
                .map(|(_, _, pk)| self.rows[pk].clone())
                .collect()
        }

        // Range on first field — scan range, post-filter second field
        (QueryBound::Range { start: as_, end: ae }, _) => {
            let Some((u8_min, u8_max)) = &self._bounds_u8 else {
                return Vec::new();
            };
            let Some((pk_min, pk_max)) = &self._bounds_task_id else {
                return Vec::new();
            };
            let oc_bounds = &self._bounds_option_creature_id;

            let composite_start = match as_ {
                Bound::Included(v) => {
                    Bound::Included((v.clone(), u8_min.clone(), pk_min.clone()))
                }
                Bound::Excluded(v) => {
                    Bound::Excluded((v.clone(), u8_max.clone(), pk_max.clone()))
                }
                Bound::Unbounded => {
                    let Some((oc_min, _)) = oc_bounds else {
                        return Vec::new();
                    };
                    Bound::Included((oc_min.clone(), u8_min.clone(), pk_min.clone()))
                }
            };
            let composite_end = match ae {
                Bound::Included(v) => {
                    Bound::Included((v.clone(), u8_max.clone(), pk_max.clone()))
                }
                Bound::Excluded(v) => {
                    Bound::Excluded((v.clone(), u8_min.clone(), pk_min.clone()))
                }
                Bound::Unbounded => {
                    let Some((_, oc_max)) = oc_bounds else {
                        return Vec::new();
                    };
                    Bound::Included((oc_max.clone(), u8_max.clone(), pk_max.clone()))
                }
            };
            self.idx_active_priority
                .range((composite_start, composite_end))
                .filter(move |(_, b_val, _)| match &b {
                    QueryBound::Exact(v) => b_val == v,
                    QueryBound::Range { start, end } => in_bounds(b_val, start, end),
                    QueryBound::Any => true,
                })
                .map(|(_, _, pk)| self.rows[pk].clone())
                .collect()
        }
    }
}
```

Note the `move` keyword on the post-filter closures — this is required so that
the closure takes ownership of the `QueryBound` values, which are local
variables. For `by_*` (returning `Vec`), the `move` is technically optional
since `.collect()` consumes the iterator immediately. For `iter_by_*` (returning
`impl Iterator`), `move` is mandatory because the iterator outlives the
function scope.

This is a sketch — the actual generated code will follow the same structure but
be produced by the proc macro. The key insight is that the dispatch is a match
on the leading query bound, with post-filtering for any trailing fields that
cannot be served by the index range.

### Codegen Strategy for N Fields

For N-field indexes, the proc macro generates a cascade of N+1 code paths
(linear in N, not exponential). The "boundary position" is the first field
that is not `Exact`:

- **Position 0 (first field is Range or Any):** Range/full scan on field 0,
  post-filter fields 1..N.
- **Position 1 (field 0 Exact, field 1 is Range or Any):** Lock field 0 in
  bounds, Range/full scan on field 1, post-filter fields 2..N.
- **Position k (fields 0..k-1 Exact, field k is Range or Any):** Lock fields
  0..k-1 in bounds, Range/full scan on field k, post-filter fields k+1..N.
- **Position N (all fields Exact):** Point lookup.

The dispatch match walks the fields left to right:

```rust
// N=3 index on fields (zone, floor, priority) with PK
match (&q_zone, &q_floor, &q_priority) {
    // All Exact — point lookup
    (Exact(z), Exact(f), Exact(p)) => {
        let Some((pk_min, pk_max)) = &self._bounds_task_id else {
            return Vec::new();
        };
        // Bounds: (z, f, p, pk_min..pk_max)
    }

    // zone Exact, floor Exact, priority Range/Any — scan on priority
    (Exact(z), Exact(f), _) => {
        // Bounds: (z, f, tracked_min/range..tracked_max/range, pk_min..pk_max)
        // No post-filter needed (priority is the last field)
    }

    // zone Exact, floor Range/Any — scan on floor, post-filter priority
    (Exact(z), _, _) => {
        // Bounds: (z, tracked_min/range..tracked_max/range, tracked_min..tracked_max, pk_min..pk_max)
        // Post-filter: priority
    }

    // zone Range/Any — scan on zone, post-filter floor and priority
    (_, _, _) => {
        // Bounds: (tracked_min/range..tracked_max/range, tracked_min..tracked_max, tracked_min..tracked_max, pk_min..pk_max)
        // Post-filter: floor and priority
    }
}
```

Each arm constructs bounds for the leading Exact+boundary fields and
post-filters the trailing fields. The number of arms is N+1 (one per possible
boundary position), and each arm's post-filter checks at most N-k fields.
Total complexity: O(N) arms, each O(1) to generate.

## Filtered Index Maintenance

The filter function is called during every mutation to keep the filtered index
in sync with the row data. The filter is a pure function `fn(&Row) -> bool`,
referenced by path in the attribute.

### Insert

```rust
// Generated insert maintenance for a filtered index:
if Task::is_active(&row) {
    self.idx_active_priority.insert((
        row.assignee.clone(),
        row.priority,
        row.id.clone(),
    ));
}
```

Only add the entry if the new row passes the filter. Unfiltered compound indexes
(no `filter` attribute) unconditionally insert, same as simple `#[indexed]`.

### Update

The update path must handle four cases, because the filter result can change
between the old and new row:

```rust
// Generated update maintenance for a filtered index:
let old_passes = Task::is_active(&old_row);
let new_passes = Task::is_active(&new_row);

match (old_passes, new_passes) {
    (true, true) => {
        // Both pass — update the index entry if indexed fields changed.
        if old_row.assignee != new_row.assignee || old_row.priority != new_row.priority {
            self.idx_active_priority.remove(&(
                old_row.assignee.clone(),
                old_row.priority,
                pk.clone(),
            ));
            self.idx_active_priority.insert((
                new_row.assignee.clone(),
                new_row.priority,
                pk.clone(),
            ));
        }
    }
    (true, false) => {
        // Was in the index, no longer qualifies — remove.
        self.idx_active_priority.remove(&(
            old_row.assignee.clone(),
            old_row.priority,
            pk.clone(),
        ));
    }
    (false, true) => {
        // Was not in the index, now qualifies — add.
        self.idx_active_priority.insert((
            new_row.assignee.clone(),
            new_row.priority,
            pk.clone(),
        ));
    }
    (false, false) => {
        // Neither old nor new qualifies — no-op.
    }
}
```

The `(true, true)` case includes an optimization: only remove+insert if the
indexed field values actually changed. For unfiltered compound indexes, this
simplifies to just the field-change check (same optimization as simple indexes
in v8).

### Upsert

The upsert path combines insert and update logic:

- **Insert path** (PK doesn't exist): same as Insert above.
- **Update path** (PK exists): same as Update above.

### Remove

```rust
// Generated remove maintenance for a filtered index:
self.idx_active_priority.remove(&(
    row.assignee.clone(),
    row.priority,
    row.id.clone(),
));
```

Remove unconditionally. `BTreeSet::remove` on a missing key is a no-op, so
there is no need to call the filter first. This is simpler and likely faster
than checking the filter and then conditionally removing.

### Rebuild Indexes

Called during deserialization and whenever indexes need to be reconstructed from
row data:

```rust
// Generated rebuild_indexes (filtered compound index portion):
self.idx_active_priority.clear();
for (pk, row) in &self.rows {
    if Task::is_active(row) {
        self.idx_active_priority.insert((
            row.assignee.clone(),
            row.priority,
            pk.clone(),
        ));
    }
}
```

Unfiltered compound indexes omit the `if` check and insert every row
unconditionally.

### Tracked Bounds for Filtered Indexes

Tracked bounds reflect ALL rows in the table, not just rows that pass a
particular filter. Since bounds are per-type (not per-index), this is
automatic — an insert always updates the type's bounds regardless of which
filtered indexes the row qualifies for. This is correct because the bounds
must cover all possible values that could appear in ANY index, and filtering
only reduces the set of entries, not the range of possible values.

## Required Trait Bounds

### On Field Types in Compound Indexes

- `Ord + Clone` — that's it. `Ord` is required for BTreeSet storage and for
  post-filter comparisons (`Ord` implies `Eq` implies `PartialEq`, so the `==`
  checks in post-filter code are covered). `Clone` is needed to construct
  composite tuple entries and to widen tracked bounds.

No `Bounded` trait is required. Tracked bounds handle the need for MIN/MAX-like
values at runtime.

### On Primary Key Types

- `Ord + Clone` — same requirements as field types, no special treatment.
  The PK participates as the tiebreaker in every index tuple and gets its own
  tracked bounds pair, just like any other type.

### Types That Can Participate in Compound Indexes

Any type implementing `Ord + Clone`:

- Primitives (`u8`, `u32`, `i64`, `bool`, etc.).
- Newtypes wrapping primitives (`CreatureId(u32)`).
- `Option<T>` where `T: Ord + Clone`.
- Unit-variant enums with `#[derive(Ord)]`.
- `String` and other dynamically-sized `Ord` types.
- Any custom type with `Ord + Clone`.

### The `Bounded` Trait

The `Bounded` trait still exists in the crate for users who want it, but
NOTHING in the index machinery depends on it. The existing
`map_start_bound`/`map_end_bound` helpers (which require `PK: Bounded`) will
be retired when this is implemented. They remain available for backward
compatibility but are no longer used by generated code.

### On the Filter Function

- Must be `fn(&Row) -> bool` — a bare function, not a closure.
- Must be a path that resolves at the point where the Table derive expansion
  compiles. The macro emits a call to the path (e.g., `Task::is_active(&row)`);
  the Rust compiler validates it.
- Both inherent methods (e.g., `filter = "Task::is_active"` where `is_active`
  is `fn(&self) -> bool` on `Task`) and standalone functions (e.g.,
  `filter = "is_active"` where `is_active` is `fn(&Task) -> bool`) work,
  because the macro emits UFCS syntax: `path(&row)`.
- The path resolves in the module where the struct is defined, so `use` imports
  in that module are visible. Fully qualified paths (e.g.,
  `filter = "crate::filters::is_active"`) also work.
- Closures cannot be referenced in attributes (Rust language limitation). The
  `filter = "path"` string syntax is the only viable option without a custom DSL.

## Compile-Time Error Handling

The Table derive macro validates compound index declarations and emits
compile-time errors for:

- **Missing `name`:** `#[index(fields("a", "b"))]` without a name.
- **Missing `fields`:** `#[index(name = "foo")]` without fields.
- **Unknown field name:** `fields("nonexistent")` referencing a field that
  doesn't exist on the struct.
- **Duplicate index name:** Two `#[index]` attributes with the same name, or
  a name that collides with an auto-generated simple index name (e.g.,
  `name = "species"` when the struct has `#[indexed] species: Species`).
- **Empty fields list:** `fields()` with no field names.
- **Invalid index name:** A name that is not a valid Rust identifier (e.g.,
  a Rust keyword like `type`, or a name containing special characters). The
  generated methods (`by_{name}`, `iter_by_{name}`) and field (`idx_{name}`)
  must be valid identifiers.

The `filter` path is NOT validated by the macro itself. The macro generates a
call to the path (e.g., `Task::is_active(&row)`), and the Rust compiler
validates that the path resolves to a function with the correct signature. This
gives better error messages than the macro could produce — the compiler points
at the exact call site with the expected type.

## Interaction with `#[derive(Database)]`

### No Database Derive Changes Needed

The Database derive calls table-level mutation methods (`insert_no_fk`,
`update_no_fk`, `remove_no_fk`, `upsert_no_fk`, `rebuild_indexes`). These
methods already maintain ALL indexes on the table — both simple `#[indexed]`
and compound `#[index(...)]`. The Database derive does not need to know about
compound or filtered indexes; the Table derive handles them transparently.

### FK Restrict-on-Delete Requires Simple Indexes

The Database derive's `remove_*` methods enforce restrict-on-delete by calling
`count_by_{fk_field}` on referencing tables. This method is generated by the
simple `#[indexed]` attribute on the FK field.

**Important:** Compound indexes do NOT generate `count_by_{field}` methods for
individual fields. A compound index on `(assignee, priority)` generates
`count_by_assignee_priority(a, b)`, not `count_by_assignee(a)`.

Therefore, if a field is used as a foreign key AND appears in a compound index,
it must ALSO have its own `#[indexed]` attribute for FK restrict-on-delete to
work. This is already the natural pattern (the examples in this doc show FK
fields with both `#[indexed]` and compound indexes), but it's worth stating
explicitly: **compound indexes do not replace simple indexes for FK purposes.**

## Serde Behavior

Compound and filtered indexes are derived data, consistent with simple indexes.
They are NOT serialized. Tracked bounds are also NOT serialized — they are
recomputed from row data during `rebuild_indexes`.

On deserialization, `rebuild_indexes()` reconstructs all indexes from the row
data, calling filter functions for filtered indexes, and recomputing tracked
bounds from scratch. This means:

- The serialized format does not change when compound/filtered indexes are
  added or removed.
- Tracked bounds after deserialization reflect the current row data exactly
  (not historical "ever seen" values from before serialization).
- Filter functions must be deterministic — given the same row data, they must
  return the same result. This is inherent in the design (filter functions are
  pure `fn(&Row) -> bool`), but worth stating explicitly.

## N-Field Compound Indexes

The design supports arbitrary N. All of the above generalizes naturally:

**Storage:** `BTreeSet<(F1, F2, ..., FN, PK)>`.

**Query method:** Takes N `impl IntoQuery<Fi>` parameters.

**Tracked bounds:** Each unique type across all N fields + PK gets one
`_bounds_{type}` pair. The proc macro deduplicates types at compile time.

**Composite bound construction:** Follows the formal rules in "Composite Bound
Construction — Formal Rules" above. For each leading Exact/Range field,
contribute to the composite tuple bounds. Once `Any` or Range-followed-by-
non-Any is encountered, pad trailing fields with `tracked_min`/`tracked_max`
and switch to post-filtering for remaining fields.

**Codegen complexity:** Linear in N. The codegen strategy described in "Codegen
Strategy for N Fields" above produces N+1 match arms, one per boundary
position. Each arm's post-filter checks at most N-k fields. Not exponential.

**Example — 3-field index:**

```rust
#[derive(Table, Clone, Debug)]
#[index(name = "loc", fields("zone", "floor", "priority"))]
struct Task {
    #[primary_key]
    id: TaskId,
    zone: ZoneId,       // must be Ord + Clone
    floor: u8,
    priority: u8,
}
```

Storage: `idx_loc: BTreeSet<(ZoneId, u8, u8, TaskId)>`.

Tracked bounds: `_bounds_zone_id`, `_bounds_u8`, `_bounds_task_id`. Note that
`floor: u8` and `priority: u8` share the same `_bounds_u8` pair.

```rust
// All tasks in zone 1, floor 3, priority 5 — point lookup.
table.by_loc(&ZoneId(1), &3, &5)

// All tasks in zone 1, floor 3 — prefix scan on first two fields.
table.by_loc(&ZoneId(1), &3, Any)

// All tasks in zone 1 — prefix scan on first field only.
table.by_loc(&ZoneId(1), Any, Any)

// All tasks in zone 1, any floor, priority 5 — scans zone prefix, post-filters priority.
table.by_loc(&ZoneId(1), Any, &5)
```

## Future Work

- **Unique compound indexes.** `#[index(name = "...", fields(...), unique)]` —
  enforce uniqueness on the compound key (excluding the PK tiebreaker). Insert
  and update would check for existing entries with the same compound key and
  return a new `DuplicateCompoundKey` error variant.

- **Zero-copy queries.** Currently each `Exact` or `Range` value is cloned to
  construct the composite tuple bound. For `Copy` types (all game ID newtypes)
  this is free. For `String` it is one allocation per query. A zero-copy
  approach using custom `Borrow` impls on tuples is theoretically possible but
  adds significant complexity. Deferred until profiling shows it matters.

- **`modify` / `update_with` for compound-indexed tables.** The closure-based
  mutation API would need to call filter functions and maintain compound indexes
  during in-place updates. The design is the same as the Update maintenance
  described above, just triggered from within a closure rather than from an
  explicit `update_*` call.

- **Index-only queries.** For count and existence checks, the current design
  already avoids row lookups (`count_by_*` scans the BTreeSet directly). A
  future extension could return projected tuples of just the indexed fields
  without looking up the full row, useful for reporting.

- **Bounded trait cleanup.** Once all generated code uses tracked bounds, the
  `Bounded` trait, `derive(Bounded)`, `map_start_bound`, and `map_end_bound`
  could be deprecated or removed in a future major version. They remain for
  now for backward compatibility with any user code that depends on them.
