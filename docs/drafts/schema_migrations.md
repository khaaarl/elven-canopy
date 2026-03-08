# Schema Migrations (Draft)

Early design notes for F-tab-schema-evol (custom migrations) and F-tab-schema-ver
(versioning fundamentals). Incomplete — captures discussion so far, not a finalized
design.

## Problem

Tabulosity databases are serialized directly via serde. When the schema changes
(new tables, removed columns, renamed fields, restructured tables), old save files
can't deserialize into the new types. We need a migration system that:

- Supports multiple serialization formats (JSON now, possibly bincode/msgpack later)
- Handles both simple additive changes and complex structural changes
- Doesn't slow down the common case (no migrations needed)

## Deserialization Pipeline

```
bytes
  → format decoder (JSON / bincode / msgpack)
  → check schema version
  → if version < current:
      → SchemaSnapshot (if any migration in the chain requires it)
        OR typed deserialization (if all pending migrations are typed)
      → apply migration chain: v3→v4→v5→...→current
  → typed deserialization (if not already done)
  → FK validation
  → ready
```

## Two Tiers of Migration

### Tier 1: Typed migrations

Operate on the current Rust structs after deserialization but before FK validation.
Good for:
- Populating a new field from existing data (e.g., compute `total_cost` from `items`)
- Adjusting values (e.g., rescaling a stat after rebalancing)
- Removing orphaned rows

Limitations: the old data must fit into the current struct layout. A field that was
renamed or moved to a different table can't be handled here because serde won't
know where to put the old data.

### Tier 2: SchemaSnapshot migrations

A `SchemaSnapshot` is a format-agnostic intermediate representation:

```rust
struct SchemaSnapshot {
    version: u32,
    tables: BTreeMap<String, TableSnapshot>,
}

struct TableSnapshot {
    rows: Vec<BTreeMap<String, serde_json::Value>>,
    // possibly: next_auto_id for auto-increment tables
}
```

The snapshot is constructed from whatever serialization format was used (JSON,
bincode, etc.) and provides a uniform untyped view. Migrations can:
- Rename tables or fields
- Split one table into two
- Merge two tables into one
- Move fields between tables
- Transform value types

After all snapshot-level migrations run, the snapshot is deserialized into the
current typed structs via a `from_snapshot()` method.

This path is slower (extra allocation, string-keyed lookups) and should only be
used when a migration in the chain actually requires it. If all migrations between
the save version and current version are tier-1, the snapshot step is skipped
entirely.

## Schema Version

The version number lives on the `Database` derive. Tabulosity tracks it and
includes it in serialized output. On deserialization, the version is checked
first (before any table data is parsed if possible, or at least before
validation).

## Open Questions

- Should the version number be per-database or per-table? Per-database seems
  simpler and sufficient — individual tables don't evolve independently in
  practice.
- How are migrations registered? A trait with a method per version step?
  A Vec of closures? A macro?
- Should SchemaSnapshot use `serde_json::Value` as the value type, or something
  more abstract? Using `serde_json::Value` ties it to JSON conceptually even
  if the input format was different. An enum like `SnapshotValue { Int(i64),
  Float(f64), Str(String), Bool(bool), Null, Array(...), Map(...) }` would
  be more principled but more work.
- Testing strategy: golden-file saves from each schema version? Auto-generated
  migration tests?
