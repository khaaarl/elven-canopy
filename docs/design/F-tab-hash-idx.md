# F-tab-hash-idx: Hash-Based Indexes in Tabulosity

**Status:** Draft
**Depends on:** F-tab-ordered-idx (done)
**Related:** F-tab-indexmap-fork

## Motivation

All Tabulosity indexes are currently `BTreeSet`-backed, giving O(log n) lookup
on every query. For exact-match queries (the overwhelmingly common case in the
sim), O(1) hash lookup would be a significant improvement — especially on
hot-path queries like "get creature by ID" or "get task by assignee."

However, Tabulosity has a hard determinism constraint: iteration order must be
identical across platforms, Rust versions, and process invocations. Standard
`HashMap`/`HashSet` fail this. The `InsOrdHashMap` data structure (from
F-tab-ordered-idx) solves this by providing O(1) lookup with deterministic
insertion-order iteration.

This feature integrates `InsOrdHashMap` into the Tabulosity derive macro,
enabling hash-based indexes on table fields and hash-based primary key storage.

## Design Overview

Three new capabilities:

1. **Hash-based secondary indexes** — `#[indexed(hash)]` and compound
   `#[index(..., kind = "hash")]`
2. **Hash-based primary key storage** — `#[table(primary_storage = "hash")]`
   to replace the `BTreeMap<PK, Row>` with `InsOrdHashMap<PK, Row>`
3. **`OneOrMany<PK, Collection>`** — an enum that optimizes the common case of
   single-entry index groups

## 1. Attribute Syntax

### Field-level (simple indexes)

```rust
#[indexed(hash)]              // non-unique hash index
#[indexed(hash, unique)]      // unique hash index
```

These mirror the existing `#[indexed]` and `#[indexed(unique)]` syntax.

**Parsing grammar:** The current `#[indexed(...)]` parser accepts a single
identifier. This changes to a comma-separated list of identifiers:

```
indexed_args := ident ("," ident)*
```

Accepted identifiers: `hash`, `unique`. Order-independent (`#[indexed(unique, hash)]`
is the same as `#[indexed(hash, unique)]`). Duplicates are a compile error.
`#[indexed(btree)]` is accepted as an explicit no-op (same as `#[indexed]`).
`#[indexed(btree, hash)]` is a compile error — pick one kind.

### Struct-level (compound indexes)

```rust
#[index(name = "by_species_age", fields("species", "age"), kind = "hash")]
#[index(name = "by_species_age", fields("species", "age"), kind = "hash", unique)]
```

The `kind` key is new. Omitting `kind` (or `kind = "btree"`) preserves the
existing BTreeSet behavior. `filter` composes with `kind` as expected.

### Table-level (primary storage)

```rust
#[table(primary_storage = "hash")]
```

Switches the `rows` field from `BTreeMap<PK, Row>` to `InsOrdHashMap<PK, Row>`.
Effects:

- `get` / `get_mut` / `contains` become O(1) instead of O(log n)
- `insert` / `remove` become O(1) amortized
- `iter_all` / `keys` return rows in **insertion order** rather than PK-sorted order
- `QueryOpts::ASC` / `DESC` on primary-key queries still work but require a
  post-sort (O(n log n)) rather than being free

The `#[table(...)]` attribute is new for the `Table` derive; currently there
are no struct-level table configuration attributes. It is parsed in `parse.rs`
alongside the existing struct-level `#[primary_key(...)]` and `#[index(...)]`
attribute parsing, from the struct's `attrs` list. Validation:

- Only one `#[table(...)]` attribute allowed (duplicate is a compile error)
- Only recognized key is `primary_storage` with values `"hash"` or `"btree"`
- Unknown keys → compile error with helpful message
- `#[table(primary_storage = "btree")]` is an explicit no-op (same as omitting)

**Name overlap with Database derive:** The `Database` derive already uses
`#[table(singular = "...", fks(...), ...)]` on **fields** of the Database
struct. Our new `#[table(primary_storage = "...")]` is on the **struct itself**
that derives `Table`. These are different derive contexts and different
attribute positions (struct vs field), so there is no actual collision. The
`Table` derive ignores Database-level `#[table(...)]` attributes on fields,
and the `Database` derive never sees struct-level attributes on the Table
struct. The shared name is acceptable since the semantics are "configure this
table" in both cases.

## 2. Index Storage Types

### Unique hash index

```
idx_{name}: InsOrdHashMap<FieldType, PK>
```

One entry per unique field value, mapping directly to the primary key.
`contains_key` for uniqueness checks, `get` for queries. Straightforward.

### Non-unique hash index

```
idx_{name}: InsOrdHashMap<FieldType, OneOrMany<PK, Inner>>
```

For simple indexes, `FieldType` is the field's type. For compound hash
indexes on fields `(A, B)`, `FieldType` is the tuple `(A, B)` — the full
compound key is hashed together. This is structurally different from BTree
compound indexes, which use `BTreeSet<(F1, F2, ..., PK)>` with PK appended.
Hash compound indexes separate key from value: the hash key is `(F1, F2, ...)`
and the value side holds only PKs.

Where `Inner` depends on the table's primary storage kind:

| Primary storage | Inner type               | Removal within group |
|-----------------|--------------------------|----------------------|
| `btree` (default) | `BTreeSet<PK>`        | O(log g)             |
| `hash`          | `InsOrdHashMap<PK, ()>`  | O(1) amortized       |

(Where g = group size, i.e., number of rows sharing the same indexed field value.)

### Mixed BTree + hash indexes on the same field

A field can have both a BTree index and a hash index simultaneously. They
must have **different names** — two indexes with the same name is a compile
error regardless of kind. Since field-level `#[indexed]` and
`#[indexed(hash)]` both auto-generate names from the field, having both on
the same field would collide. Instead, declare one via `#[indexed]` (or
`#[indexed(hash)]`) and the other via a struct-level
`#[index(name = "...", fields("field"), kind = "hash")]` (or `kind = "btree"`)
with an explicit distinct name.

The derive macro generates separate index fields for each
(`idx_{btree_name}: BTreeSet<...>` and `idx_{hash_name}: InsOrdHashMap<...>`).
Both are maintained on every insert/update/remove — doubling index
maintenance cost for that field. Query methods are generated for each index
independently; the caller chooses which to use based on the query pattern
(range → BTree query method, exact → hash query method).

For serialization, the BTree index is skipped+rebuilt as usual, while the
hash index is serialized directly to preserve insertion order. Both coexist
naturally since they have different names and different storage types.

### The `OneOrMany` enum

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneOrMany<V, Many> {
    One(V),
    Many(Many),
}
```

This lives in `tabulosity/src/one_or_many.rs` and is re-exported from
`tabulosity/src/lib.rs` (alongside `InsOrdHashMap`). `RemoveResult` is
co-located in the same file. Derives are needed since generated table
structs derive `Debug` and `Clone`. It optimizes the common case where an
indexed field value maps to a single row — no collection overhead at all, just
an inline PK. When a second row arrives with the same field value, it promotes
to the full collection.

**Promotion:** On insert, if `One(existing_pk)` and `new_pk != existing_pk`,
create `Many(collection)` containing both. If `new_pk == existing_pk`, it's a
no-op (idempotent insert). This equality check is a required correctness
invariant — without it, a duplicate PK would erroneously promote to `Many`.

**Demotion:** On remove, if `Many(collection)` drops to exactly one element,
demote to `One(remaining_pk)`. This avoids carrying collection overhead for
groups that were temporarily >1 but returned to 1. The cost is one check on
every remove, which is negligible.

**Demotion to zero:** If `One(pk)` and that PK is removed, the caller (the
generated index maintenance code) removes the entire entry from the outer
`InsOrdHashMap`. `OneOrMany` itself signals this via a `remove` method
returning:

```rust
pub enum RemoveResult {
    /// The value was found and removed; the OneOrMany is now empty.
    /// Caller should remove the entry from the outer map.
    Empty,
    /// The value was found and removed; the OneOrMany still has entries.
    Removed,
    /// The value was not found (no-op). Should not happen in correct
    /// index maintenance — debug_assert in generated code.
    NotFound,
}
```

**Duplicate insert into `Many`:** Delegated to the inner collection.
`BTreeSet::insert` is idempotent. `InsOrdHashMap::insert` with the same key
overwrites the value (which is `()`, so effectively idempotent).

**Iteration:** `OneOrMany` exposes an iterator that yields either the single
value or iterates the inner collection.

**`len()`:** Returns 1 for `One`, delegates to `inner.len()` for `Many`.
Needed by `count_by_*` query methods to count matching rows without
materializing a `Vec`.

**`contains()`:** Returns true if the value is present. For `One`, a simple
equality check. For `Many`, delegates to the inner collection's lookup.
Useful for debug assertions and index integrity checks.

**Why not always use the collection?** For near-unique indexes (the common
case), every key maps to one row. A `BTreeSet` with one element is 24+ bytes
of heap allocation; an `InsOrdHashMap` with one element is even more.
`One(PK)` is just the PK inline (typically 4-8 bytes). For a table with 10K
rows and a near-unique index, this saves ~240KB of heap allocations.

## 3. Query Methods

Hash indexes generate the same method signatures as BTree indexes:
`by_{name}`, `iter_by_{name}`, `count_by_{name}`. The `impl IntoQuery<T>`
parameters accept the same types. The difference is in the generated body.

### Exact match (all fields `QueryBound::Exact`)

```rust
// Unique:
self.idx_{name}.get(&field_val).and_then(|pk| self.rows.get(pk))

// Non-unique:
self.idx_{name}.get(&field_val)
    .map(|one_or_many| one_or_many.iter().filter_map(|pk| self.rows.get(pk)))
```

This is O(1) for the index lookup + O(1) or O(log n) per PK lookup depending
on primary storage. This is the hot path and the main reason to use hash
indexes.

### MatchAll (all fields `QueryBound::MatchAll`)

Iterate all entries in the `InsOrdHashMap`. For non-unique indexes, flatten the
`OneOrMany` values. Look up each PK in `rows`.

Order: insertion order of field values in the index, then within each group:
PK-sorted (BTree primary) or insertion-ordered (hash primary).

### Partial match (compound indexes, mix of Exact and MatchAll)

Compound hash indexes are supported (via
`#[index(name = "...", fields("a", "b"), kind = "hash")]`). When not all
fields are exact, the query degrades:

For a compound hash index keyed on `(A, B)`:
- `by_idx(exact_a, exact_b)` → hash lookup on `(a, b)`, O(1)
- `by_idx(exact_a, MatchAll)` → **iterate all entries, filter** where first
  tuple element matches. This is O(n) in the index size — the hash can't help
  with prefix queries.
- `by_idx(MatchAll, exact_b)` → same, iterate + filter.

This is a fundamental trade-off of hash vs. BTree. BTree compound indexes
support efficient prefix scans; hash compound indexes do not. Users who need
efficient prefix queries should use BTree compound indexes (or have both — see
Section 2, "Mixed BTree + hash indexes on the same field"). The generated code
handles partial matches correctly but without the performance benefit of
hashing.

### QueryOpts (ASC/DESC/offset) interaction

Hash indexes do not support ordering. The `order` field of `QueryOpts` is
**ignored** for hash index queries — results come back in insertion order
(of the index), always. This matches how real databases handle hash indexes:
ordering is not a supported operation.

**Offset** is still applied: skip the first N results in insertion order.

If a caller needs ordered results from a hash index, they sort the returned
`Vec` themselves. This makes the cost explicit rather than hiding an O(n log n)
sort inside every query call.

### `modify_each_by_*` methods

These combine query logic with in-place mutation. For hash indexes, the
query portion uses the same dispatch as `by_*` (exact → get, MatchAll →
iterate, partial → iterate+filter, range → panic). The mutation and
debug-assertion logic is unchanged — it operates on `(&PK, &mut Row)` pairs
regardless of how they were found.

### Range queries → panic

If any field's `QueryBound` is `Range { .. }`, the generated code panics:

```
panic!("range queries are not supported on hash index `{name}`; use Exact or MatchAll")
```

This is a programming error. The type system can't prevent it because `by_*`
methods use `impl IntoQuery<T>`, and `IntoQuery` is implemented for range
types universally. A runtime panic with a clear message is the pragmatic
choice.

**Alternative considered:** A separate `IntoHashQuery` trait that excludes
range types. Rejected because it would double the trait surface, require
users to know which trait to use, and break the uniform `by_*` API.

## 4. Index Maintenance

### Insert

```rust
// Unique hash:
self.idx_{name}.insert(field_val.clone(), pk.clone());

// Non-unique hash:
match self.idx_{name}.get_mut(&field_val) {
    Some(one_or_many) => one_or_many.insert(pk.clone()),  // may promote One→Many
    None => { self.idx_{name}.insert(field_val.clone(), OneOrMany::One(pk.clone())); }
}
```

For filtered indexes, guarded by `if filter_fn(&row)` as with BTree indexes.

### Update

Same structure as BTree: remove old entry, insert new entry, but only if the
indexed field value actually changed (or filter status changed). The removal
uses the old field value to find the `OneOrMany` group, then removes the PK
from within it (possibly demoting Many→One or removing the entry entirely).

### Remove

Look up the field value in the outer map, remove the PK from the `OneOrMany`
group. Demote or remove as needed.

### Unique checks

For unique hash indexes, the check before insert is simply:

```rust
if self.idx_{name}.contains_key(&field_val) {
    return Err(Error::DuplicateIndex { ... });
}
```

No range scanning needed. For updates, only check if the field value changed,
and skip if the existing entry is the row being updated.

## 5. Bounds Tracking

Hash indexes do **not** generate `_bounds_{type}` fields. Bounds tracking
exists to enable dynamic range queries on BTree indexes without requiring
compile-time `MIN`/`MAX` constants. Hash indexes don't support range queries,
so bounds are unnecessary.

If a field type appears in both a hash index and a BTree index, bounds are
still generated (driven by the BTree index's needs).

## 6. Primary Storage

### `InsOrdHashMap<PK, Row>` as `rows`

When `#[table(primary_storage = "hash")]` is set:

- `rows: InsOrdHashMap<PK, Row>` instead of `BTreeMap<PK, Row>`
- All `rows.get(pk)` calls remain identical (both types have `get`)
- `rows.insert(pk, row)` semantics are the same (returns old value if present)
- `rows.remove(pk)` semantics are the same (returns removed value)
- `rows.iter()` yields `(&PK, &Row)` in insertion order (not PK-sorted)
- `rows.len()`, `rows.is_empty()`, `rows.contains_key()` — same API

### Behavioral differences

| Behavior                     | BTreeMap (default)     | InsOrdHashMap               |
|------------------------------|------------------------|-----------------------------|
| `get` / `contains_key`       | O(log n)               | O(1)                        |
| `insert` / `remove`          | O(log n)               | O(1) amortized              |
| `iter_all` order             | PK-sorted              | Insertion order             |
| `keys` order                 | PK-sorted              | Insertion order             |
| `iter_by_{pk}(exact)`        | O(log n)               | O(1)                        |
| `iter_by_{pk}(range)`        | O(log n + k)           | Not supported (panic)       |

**Interaction with `modify_unchecked_range`:** This method currently takes a
`RangeBounds<PK>` and iterates `rows.range_mut(...)`. `InsOrdHashMap` does not
support range iteration. For hash primary storage, `modify_unchecked_range`
is **not generated** — it's a compile error to call it. Range-based operations
are a mismatch with hash storage. Users should use `modify_unchecked_all`
with a filter closure, or `modify_unchecked` on individual PKs.

**Interaction with `modify_unchecked_all`:** Currently implemented by
delegating to `modify_unchecked_range(..)`. For hash primary storage, this
is generated to directly call `rows.iter_mut()` instead, since
`InsOrdHashMap` supports `iter_mut()` natively and the range delegation
is unavailable. The generated doc comment for `modify_unchecked_all` is
adjusted accordingly (no reference to `modify_unchecked_range` when it
doesn't exist).

**Interaction with PK auto-increment:** `next_id` tracking is independent of
storage type. It's a separate counter field, not derived from `rows` ordering.
No change needed.

### Why make primary storage opt-in?

- PK-sorted iteration is a useful default (e.g., for deterministic replay).
  Hash primary changes `iter_all` semantics in a way that could silently break
  downstream assumptions.
- Range queries on the primary key (`by_pk(10..20)`) stop working.
- The overhead of O(log n) vs O(1) for PK lookup is modest for typical table
  sizes (log2(10000) ~ 13 comparisons). The benefit is real but not critical
  for most tables.
- Tables where O(1) PK lookup matters are the ones the user should explicitly
  opt into.

## 7. Serialization (Serde)

### Current behavior (BTree indexes)

The generated serde impls are **fully custom** (hand-written `Serialize` /
`Deserialize` impls in the proc macro, not `#[derive(Serialize)]`). They do
not use `#[serde(skip)]` — there are no derive attributes at all.

**Current format:**
- Plain tables: serialize as a JSON array of rows (`[row, row, ...]`)
- Auto-increment tables: `{"next_id": N, "rows": [row, ...]}`
- Non-PK auto tables: `{"next_<field>": N, "rows": [row, ...]}`

On serialize, only `rows.values()` is emitted. Indexes are omitted entirely.
On deserialize, rows are inserted into an empty table, then
`post_deser_rebuild_indexes()` reconstructs all indexes from the rows. BTree index
insertion order is irrelevant since `BTreeSet` ordering is determined by
`Ord`, not insertion history.

### Hash secondary indexes

Hash secondary indexes must be **serialized directly**. Unlike BTree indexes,
their insertion order is semantic state — rebuilding from `rows` would erase
the original insertion history and impose `rows` iteration order instead.

**Format change:** Tables with hash indexes change from a flat row array to
a struct with explicit index data. The new serialization format:

```json
// Plain table, no hash indexes (unchanged):
[row, row, ...]

// Plain table WITH hash indexes:
{"rows": [row, ...], "idx_species": [...], "idx_name": [...]}

// Auto-increment table WITH hash indexes:
{"next_id": N, "rows": [row, ...], "idx_species": [...]}
```

Each hash index serializes as an ordered sequence of `(K, V)` pairs (live
entries only, in insertion order, tombstones excluded). On deserialize,
entries are inserted in that order, reconstructing the original insertion
order.

**This is a breaking format change** for tables that add hash indexes. Tables
without hash indexes keep the exact same format. This is acceptable since
hash indexes are a new feature — no existing serialized data uses them.

**Codegen impact:** The three serde codegen functions (`gen_serde_impls_plain`,
`gen_serde_impls_auto`, `gen_serde_impls_nonpk_auto`) all need a "has hash
indexes?" branch. When hash indexes are present, they emit struct-style
serialization that includes each hash index field alongside `rows` (and
`next_id` / `next_<field>` if applicable).

**Required serde impls:**

- **`InsOrdHashMap<K, V>`**: serialize as `Vec<(K, V)>` in insertion order,
  deserialize by inserting in sequence order. This is a new impl on the
  `InsOrdHashMap` type itself (guarded by a `serde` feature flag).
  Deserialization uses `InsOrdHashMap::with_capacity(len)` and inserts each
  pair in order. Also implement `FromIterator<(K, V)>` on `InsOrdHashMap`
  to support idiomatic construction from iterators.

- **`InsOrdHashMap<K, ()>` special case**: when used as the inner collection
  for non-unique hash indexes with hash primary storage, the `()` values
  carry no information. Serialize as `Vec<K>` (just keys in insertion order),
  deserialize by wrapping each key as `(k, ())` and using `FromIterator`.
  Handled via inline codegen helpers in the proc macro (not a separate type).

- **`OneOrMany<V, Many>`**: always serialize as a **sequence**. `One(pk)`
  serializes as `[pk]`, `Many(collection)` serializes as
  `[pk, pk, ...]` (the collection's iteration order). On deserialize:
  single-element sequence → `One`, multi-element → `Many`. This is
  unambiguous because `Many` with one element should have been demoted
  to `One` (the demotion invariant guarantees this). Custom serde impl,
  not derive-based, since the inner `Many` type varies.

- **`BTreeSet`**: already has serde support (stdlib). No work needed.

### `post_deser_rebuild_indexes()` behavior

`post_deser_rebuild_indexes()` is a public `#[doc(hidden)]` method called by the
generated `Deserialize` impl after inserting all rows.

For hash indexes, `post_deser_rebuild_indexes()` must **not** overwrite deserialized
state — doing so would erase the serialized insertion order. The approach:

- The generated `Deserialize` impl deserializes hash index fields directly
  from the serialized data. After all rows and hash indexes are loaded, it
  calls `post_deser_rebuild_indexes()`.
- `post_deser_rebuild_indexes()` rebuilds **only BTree indexes** and recomputes tracked
  bounds. Hash indexes are left untouched.
- A separate `manual_rebuild_all_indexes()` method rebuilds **everything** including
  hash indexes (recomputing insertion order from `rows` iteration order).
  This is the manual escape hatch.

**Deserialization ordering:** The generated `Deserialize` impl for tables
with hash indexes proceeds in this order:

1. Deserialize the struct fields from the serialized data: `rows` (as
   `Vec<Row>`), `next_id` (if auto-increment), and each hash index field.
2. Insert rows into the `rows` storage (`BTreeMap` or `InsOrdHashMap`)
   **without index maintenance** — the rows are inserted directly into the
   storage, not through the table's `insert()` method. This matches the
   current behavior (the deser impl does `table.rows.insert(pk, row)`).
3. Assign the deserialized hash index fields directly to the table's index
   fields.
4. Call `post_deser_rebuild_indexes()` to reconstruct BTree indexes and tracked bounds
   from the now-populated `rows`.

This avoids double-building hash indexes (once via insert maintenance, once
from deserialized data).

The `Database`-level `Deserialize` impls (which deserialize each table and
call `post_deser_rebuild_indexes()` on them) are unaffected — they call the same method,
which now simply skips hash indexes.

### Hash primary storage

When `rows` is `InsOrdHashMap<PK, Row>`:

**Serialize:** Emit rows as an ordered sequence of `(PK, Row)` pairs in
insertion order (via the `InsOrdHashMap` serde impl). This replaces the
current `rows.values()` serialization — PKs are now explicit in the
serialized data rather than extracted from each row on deserialize.

**Deserialize:** Reconstruct the `InsOrdHashMap` by inserting `(PK, Row)`
pairs in the serialized sequence order. This preserves the original
insertion order. The deserialization path creates an empty `InsOrdHashMap`,
then inserts each pair in order (via explicit loop, not `FromIterator`,
so we can validate).

**Validation during deser:**
- Duplicate PKs in the serialized sequence → error (same as current BTreeMap
  deser behavior).
- PK-row consistency: the serialized PK must match the PK extracted from the
  row (via the same `extract_key` logic used for BTreeMap deser). Mismatch →
  error. This prevents corrupted data where the explicit PK disagrees with
  the row's PK fields.

**Implication:** Two `InsOrdHashMap`-primary tables with identical rows but
different insertion histories will serialize differently (different ordering).
This is intentional — insertion order is part of the table's semantic state
when hash primary storage is chosen.

**Format difference:** BTreeMap-primary tables serialize rows as `[row, ...]`
(PK extracted from each row). InsOrdHashMap-primary tables serialize rows as
`[(pk, row), ...]` (PK explicit, order significant). This difference is
handled in the generated serde codegen.

## 8. Trait Bounds

### Hash indexes require `Hash + Eq`

BTree indexes require `Ord` on indexed field types (for `BTreeSet` ordering).
Hash indexes require `Hash + Eq` (for `InsOrdHashMap` hashing).

The derive macro adds the appropriate bounds to the generated `impl` blocks:

- BTree index on field of type `T` → `T: Ord + Clone`
- Hash index on field of type `T` → `T: Hash + Eq + Clone`
- Both on the same field → `T: Ord + Hash + Clone` (since `Eq` is implied by `Ord`)

### Hash primary storage requires `Hash + Eq` on PK

Currently, PKs need `Ord + Clone`. With hash primary, they additionally need
`Hash + Eq`. The derive macro adds these bounds when `primary_storage = "hash"`.

### Compile error quality

If a user puts `#[indexed(hash)]` on a field whose type doesn't implement
`Hash`, the Rust compiler will emit an error pointing at the generated code,
not the user's attribute. This is a known limitation of proc macros — the
error message will mention `Hash` and the field type, which should be
sufficient for diagnosis. No special error handling in the macro is needed
for v1; if this proves confusing in practice, we can add a `static_assert`
style check later.

## 9. `post_deser_rebuild_indexes` and `manual_rebuild_all_indexes`

The existing `rebuild_indexes()` method is **renamed** to
`post_deser_rebuild_indexes()` to clarify its specific purpose. All
existing call sites (generated `Deserialize` impls, tests, user code)
must be updated. This is a breaking change to the generated API, but the
method is `#[doc(hidden)]` and primarily called by generated code, so
the blast radius is small. Tests that call it directly will need updating.

**`post_deser_rebuild_indexes()`** — called post-deserialization. Clears and
rebuilds only BTree indexes and tracked bounds from `rows`. Hash indexes are
**not touched** — they were deserialized directly and have correct state.

**`manual_rebuild_all_indexes()`** — new method, manual escape hatch. `pub`
(not `#[doc(hidden)]` — this is an intentional user-facing API for when you
need to recompute all indexes from scratch). **Clears all indexes first**
(BTree indexes are cleared as usual; hash indexes are replaced with
fresh empty `InsOrdHashMap`s), then rebuilds everything from `rows`.
Hash index insertion order after this call reflects `rows` iteration order
(PK-sorted for BTreeMap primary, insertion-ordered for InsOrdHashMap
primary). The `OneOrMany` promotion logic applies during rebuild (first
PK for a field value creates `One`, subsequent PKs promote to `Many`).
Tracked bounds are also reset and recomputed.

If a table has no hash indexes, `manual_rebuild_all_indexes()` is identical to
`post_deser_rebuild_indexes()`.

**Capacity hint:** During rebuild, hash indexes are initialized with
`InsOrdHashMap::with_capacity(self.rows.len())` to avoid rehashing.

## 10. `modify_unchecked` and Debug Assertions

No changes to the assertion logic. The debug-build assertions snapshot indexed
field values before the closure and verify they're unchanged after. This works
identically regardless of whether the index is BTree or hash — the assertion
checks field equality, not index structure.

Generated doc comments on `iter_all`, `keys`, and similar methods are adjusted
for hash primary storage: "insertion order" instead of "primary key order."

## 11. Resolved Design Decisions

**Q1: Compound hash indexes.** Supported, with the documented trade-off that
partial matches (any field `MatchAll`) degrade to a full scan. The user knows
what they're asking for. See Section 3 (Partial match).

**Q2: Mixed index kinds on the same field.** Supported. A field can have both
a BTree index and a hash index simultaneously (e.g., BTree for range queries,
hash for exact lookups). The two indexes must have **different names** — if a
field has `#[indexed]` (BTree, auto-named) and also needs a hash index, the
hash index must be declared as a struct-level `#[index(name = "...", ...)]`
with an explicit distinct name. Two indexes with the same name is a compile
error regardless of kind. Both indexes are maintained on insert/update/remove,
doubling index maintenance cost for that field — acceptable since the user
explicitly opted in.

**Q3: Default primary storage for tables with hash indexes.** No implication.
`#[indexed(hash)]` does not change primary storage. The two are orthogonal —
the user explicitly opts into each. Hash indexes with BTreeMap primary give
O(1) field→PK then O(log n) PK→Row, which is still a win over O(log n) + O(log n).

**Q4: `modify_unchecked_range` with hash primary.** Not generated — compile
error. See Section 6.

## 12. Implementation Plan

1. **Prerequisites on `InsOrdHashMap`** — before any derive macro work:
   - Implement `FromIterator<(K, V)>` for `InsOrdHashMap`. This doesn't
     exist yet and is needed for idiomatic construction during
     deserialization and rebuild.
   - Implement serde `Serialize`/`Deserialize` for `InsOrdHashMap<K, V>`
     (behind `serde` feature flag). Serialize as `Vec<(K, V)>` in insertion
     order; deserialize via `FromIterator`.

2. **`OneOrMany<V, Many>`** — new type in `tabulosity/src/one_or_many.rs`
   with insert/remove/iter/len. Uses two concrete type params:
   `OneOrMany<V, BTreeSet<V>>` (BTree primary) and
   `OneOrMany<V, InsOrdHashMap<V, ()>>` (hash primary). No trait abstraction
   needed — the derive macro knows which concrete type to emit based on
   `primary_storage`.

3. **Parse changes** — extend `parse.rs` (new function for `#[table(...)]`):
   - Add `IndexKind` enum (`BTree` | `Hash`) to crate.
   - `ParsedField` gains `index_kind: IndexKind` (from `#[indexed(hash)]`).
   - `IndexDecl` gains `kind: IndexKind` (from `kind = "hash"` in `#[index]`).
   - `ResolvedIndex` in `table.rs` gains `kind: IndexKind`.
   - New `PrimaryStorageKind` enum (`BTree` | `Hash`), parsed from
     `#[table(primary_storage = "...")]` and threaded through codegen.
   - Add `parse_table_attr()` function for struct-level `#[table(...)]`.

4. **Codegen: index field types** — emit `InsOrdHashMap<..., OneOrMany<PK, Inner>>`
   for non-unique hash indexes, `InsOrdHashMap<..., PK>` for unique hash indexes.

5. **Codegen: primary storage** — emit `InsOrdHashMap<PK, Row>` when opted in.
   Adjust all `rows.*` call sites in generated code.

6. **Codegen: index maintenance** — insert/update/remove for hash indexes.

7. **Codegen: query methods** — hash-specific match cascade (exact → get,
   MatchAll → iterate, partial → iterate+filter, range → panic).

8. **Codegen: unique checks** — `contains_key` for hash unique indexes.

9. **Codegen: bounds tracking** — skip for hash indexes.

10. **Codegen: post_deser_rebuild_indexes / manual_rebuild_all_indexes** —
    `post_deser_rebuild_indexes()` skips hash indexes (they're deserialized directly).
    `manual_rebuild_all_indexes()` clears hash indexes first, then rebuilds
    everything including hash indexes from `rows`, using
    `InsOrdHashMap::with_capacity(self.rows.len())` to avoid rehashing.

11. **Serde codegen** — substantial changes to generated serialization:
    - Modify all three serde codegen functions (`gen_serde_impls_plain`,
      `gen_serde_impls_auto`, `gen_serde_impls_nonpk_auto`) to include
      hash index fields in serialization when present, and deserialize
      them directly (not via rebuild).
    - `InsOrdHashMap<K, ()>` special case: the generated serde code
      serializes these fields using a helper that emits `Vec<K>` (keys
      only, in insertion order) and deserializes by wrapping each key as
      `(k, ())` via `FromIterator`. This is inline codegen in the proc
      macro (a `serialize_with` / `deserialize_with` style helper emitted
      alongside the table's serde impl), not a separate type or module
      in the tabulosity crate.
    - `OneOrMany` serde impl (always-sequence format).
    - Hash primary storage: rows serialize in insertion order as
      `Vec<(PK, Row)>` via the `InsOrdHashMap` serde impl. Deserialize
      reconstructs the map in that order, preserving insertion order.

12. **Tests** — comprehensive coverage per the test-driven workflow.
    Must include insertion-order-survives-serde-roundtrip tests: insert
    rows in a specific order, serialize, deserialize, verify hash index
    iteration order is preserved exactly. Also test that
    `post_deser_rebuild_indexes()` does NOT clobber deserialized hash indexes, while
    `manual_rebuild_all_indexes()` does.

## 13. Codegen Branching Scope

The `ResolvedIndex` struct (in `table.rs`) gains a `kind: IndexKind` field.
Nearly every codegen function must branch on this:

- `gen_idx_field_decls` — emit InsOrdHashMap vs BTreeSet
- `gen_idx_field_inits` — `InsOrdHashMap::new()` vs `BTreeSet::new()`
- `gen_all_idx_insert` — OneOrMany promotion vs BTreeSet insert
- `gen_all_idx_update` — hash remove+insert vs BTreeSet remove+insert
- `gen_all_idx_remove` — OneOrMany demotion vs BTreeSet remove
- `gen_unique_checks_insert` — `contains_key` vs range scan
- `gen_unique_checks_update` — same
- `gen_rebuild_body` — InsOrdHashMap insertion vs BTreeSet insertion
- `gen_all_query_methods` — hash dispatch vs match cascade
- `gen_all_modify_each_methods` — hash query dispatch + mutation
- `gen_modify_unchecked` — no change (assertions only)
- `collect_unique_tracked_types` — skip hash index types
- `gen_serde_impls_plain` / `gen_serde_impls_auto` / `gen_serde_impls_nonpk_auto`
  — include hash index fields in serialization format. These functions
  currently take only table/row/PK info; they will need the list of
  resolved hash indexes as a new parameter to emit the correct fields.

Similarly, `primary_storage` kind affects:
- `rows` field type
- `modify_unchecked_range` (not generated for hash) / `modify_unchecked_all`
- Generated doc comments ("insertion order" vs "primary key order")
- `iter_all` / `keys` — `BTreeMap::iter` and `InsOrdHashMap::iter` return
  different concrete types, but generated methods return `impl Iterator`
  or collect into `Vec`, so the concrete type doesn't leak. Confirm this
  holds for all generated iterator-returning methods during implementation.

This is a large surface area. The implementation plan orders the work to
minimize partially-working intermediate states: parse first, then storage
types, then maintenance, then queries.

## 14. Determinism Note

`InsOrdHashMap` uses `std::collections::HashMap` internally with the default
`RandomState` hasher, which has per-process random seeds. This does **not**
affect determinism — iteration always goes through the ordered vec, never
the HashMap. Performance characteristics (collision rates, probe sequences)
may vary across runs, but observable behavior is identical. This is consistent
with the existing `InsOrdHashMap` design from F-tab-ordered-idx.
