# Tabulosity Compound Spatial Indexes (F-tab-spatial-2)

**Version:** v4

Design document for compound spatial indexes in tabulosity: a non-spatial
prefix (one or more fields) partitioning rows into separate R-trees.
Builds on the simple R-tree index delivered in F-tab-spatial.

For range/radius queries, KNN, and None-set query APIs, see the separate
draft: `tabulosity_spatial_queries.md` (F-tab-spatial-3).

For general nested multi-kind compound indexes (lifting the same-kind
prefix restriction), see `tabulosity_spatial_queries.md` §
"General Nested Multi-Kind Compound Indexes" (F-tab-nested-idx).

---

## Motivation

The primary driver is F-zone-world: each zone has its own local voxel
coordinate system, so all spatial queries must be scoped by zone. A query
like "creatures near position X in zone Z" should search only that zone's
R-tree partition, not the entire world. This requires compound indexes
keyed by `(zone_id, spatial_box)`.

Beyond zones: any future "find things near X within category Y" pattern
benefits from partitioned spatial indexes rather than filtering after a
global spatial query.

## Attribute Grammar

The `fields(...)` syntax in struct-level `#[index(...)]` attributes is
extended to support optional per-field kind keywords after field name
strings. The index-level `kind` parameter sets the default kind for all
unannotated fields.

This is **new syntax** — existing indexes that use only string literals
in `fields(...)` are unaffected, since the per-field keyword is optional
and the default kind remains `btree`.

```rust
// Compound spatial — zone_id partitions into separate R-trees.
// Prefix fields default to btree (the default kind).
#[index(name = "by_zone_pos", fields("zone_id", "pos" spatial))]
// Storage: BTreeMap<ZoneId, SpatialIndex<PK, Point>>

// Hash prefix via index-level kind default:
#[index(name = "by_zone_civ_pos",
        fields("zone_id", "civ_id", "pos" spatial),
        kind = "hash")]
// Storage: InsOrdHashMap<(ZoneId, CivId), SpatialIndex<PK, Point>>

// Per-field override of the kind default:
#[index(name = "by_zone_pos", fields("zone_id" hash, "pos" spatial))]
// Storage: InsOrdHashMap<ZoneId, SpatialIndex<PK, Point>>

// With filter:
#[index(name = "by_zone_pos",
        fields("zone_id", "pos" spatial),
        filter = "is_alive")]

// Single-field spatial — both syntaxes resolve equivalently:
// (Parsed representations differ, but both resolve to the same
// ResolvedIndex: kind = Spatial, single spatial field, no compound_spatial.)
#[index(name = "pos_idx", fields("pos"), kind = "spatial")]
#[index(name = "pos_idx", fields("pos" spatial))]
```

**Precedent:** MongoDB's `createIndex` uses the same per-field type
pattern: `{ category: 1, location: "2dsphere" }`.

## Grammar Rules

1. Each field in `fields(...)` is a string literal optionally followed by
   a kind keyword: `btree`, `hash`, or `spatial`.
2. Fields without a kind keyword inherit the index-level `kind` parameter
   (default: `btree`).
3. At most one field may be `spatial`. It **must** be the last field.
4. All non-spatial prefix fields must resolve to the same kind (after
   applying defaults and per-field overrides). A compile error is emitted
   if prefix fields disagree. (The general nesting extension in
   F-tab-nested-idx lifts this restriction.)
5. The existing `kind = "spatial"` on a single-field index remains valid
   and is equivalent to `fields("field" spatial)`.

**Rejection example for rules 2+4:** `fields("a", "b" hash, "c" spatial)`
— field "a" inherits the default `btree`, field "b" is explicitly `hash`.
The prefix fields disagree (`btree` vs `hash`), so this is a compile
error. To fix: either annotate "a" as `hash` too, or set `kind = "hash"`
at the index level so both inherit `hash`.

## Parsing Changes (`parse.rs`)

### Per-Field Kind in `fields(...)`

Currently, `IndexDeclParsed::parse` consumes `fields(...)` as a sequence
of `LitStr` tokens separated by commas (lines 337-343). The grammar is
extended to: after each `LitStr`, optionally consume an `Ident` if one
follows (before the comma or closing paren). The identifier must be one
of `btree`, `hash`, or `spatial`.

```
fields_list = field { "," field }
field       = LitStr [ Ident ]    // Ident = "btree" | "hash" | "spatial"
```

In syn terms, after parsing the `LitStr`, peek for an `Ident` (not a
comma or EOF). If present, parse it and validate it's a known kind. This
is unambiguous — field names are string literals, so a bare identifier
after a string can only be a kind keyword. The existing comma consumption
after each field remains unchanged — the optional ident is consumed
between the `LitStr` and the comma check.

### Data Representation

`IndexDecl` currently stores `fields: Vec<String>`. This becomes:

```rust
pub struct IndexFieldDecl {
    pub name: String,
    /// Per-field kind override. None = inherit from index-level `kind`.
    pub kind_override: Option<IndexKind>,
}

pub struct IndexDecl {
    pub name: String,
    pub fields: Vec<IndexFieldDecl>,  // was Vec<String>
    pub filter: Option<String>,
    pub unique: bool,
    pub kind: IndexKind,  // index-level default
}
// Migration note: the following call sites iterate `IndexDecl.fields`
// as strings and must change to access `IndexFieldDecl.name`:
//   - `resolve_indexes` (table.rs ~line 880): maps field names to (Ident, Type)
//   - `validate_index_decl` (table.rs ~line 922): iterates field names for existence/PK checks
//   - `parse_index_attrs` (parse.rs ~line 298): copies `decl.fields` from parsed to IndexDecl
```

### Resolution to `ResolvedIndex`

`ResolvedIndex` currently stores all fields uniformly. For compound
spatial indexes, the resolved form splits into prefix fields and a
spatial tail:

```rust
struct ResolvedIndex {
    name: String,
    fields: Vec<(Ident, Type)>,  // ALL fields (prefix + spatial), for field_changed_check
    filter: Option<String>,
    is_unique: bool,
    kind: IndexKind,  // overall index kind

    // NEW: compound spatial support. None for non-compound-spatial indexes.
    compound_spatial: Option<CompoundSpatialInfo>,
}

struct CompoundSpatialInfo {
    /// Prefix field indices into `fields` (all non-spatial fields).
    /// Indices rather than (Ident, Type) pairs avoid duplicating data already
    /// in `ResolvedIndex.fields`, and the `fields` vec is needed intact for
    /// `field_changed_check` which operates over all fields together.
    prefix_field_indices: Vec<usize>,
    /// The spatial field index into `fields` (always the last).
    spatial_field_index: usize,
    /// Resolved kind for the prefix map (btree or hash).
    prefix_kind: IndexKind,
}
```

Resolution logic (in `resolve_indexes` or equivalent):
1. For each `IndexFieldDecl`, resolve its effective kind:
   `kind_override.unwrap_or(index_level_kind)`.
2. Find the spatial field (if any). If present, validate it's last.
3. Collect prefix fields. Validate they all have the same resolved kind.
4. Build `CompoundSpatialInfo` if there's a spatial field AND prefix
   fields exist.
5. Set `kind = Spatial` whenever a spatial field is detected — whether
   the index is single-field spatial or compound spatial. This ensures
   downstream codegen dispatches correctly (all spatial code paths
   branch on `idx.kind == Spatial`).

Single-field spatial indexes (`fields("pos" spatial)` or
`kind = "spatial"` with one field) set `compound_spatial = None` and
`kind = Spatial`, following the existing codepath unchanged.

### Existing Error to Relax

`parse.rs` line 398-401 currently rejects multi-field spatial indexes:

```rust
if kind == IndexKind::Spatial && fields.len() > 1 {
    return Err(input.error(
        "spatial indexes must have exactly one field; compound spatial indexes are not supported",
    ));
}
```

This error applies only when the index-level `kind` is `Spatial` (the
legacy syntax). With per-field kinds, validation moves to the resolution
phase: the error becomes "more than one spatial field" or "spatial field
not last", checked after per-field kinds are resolved. When
`kind = "spatial"` is set at the index level and there are multiple
fields, all fields inherit `spatial`, which triggers the "at most one
spatial field" validation in the resolution phase. The parse-time error
is therefore safely replaced by resolution-time validation.

**Error message quality:** The resolution phase should detect when all
fields inherited `spatial` from the index-level `kind` and produce a
targeted error: `"index-level kind = \"spatial\" cannot be used with
multiple fields; use per-field spatial annotation on the spatial field
instead"` — rather than the generic "at most one spatial field" message,
which would be confusing when the user wrote `kind = "spatial"` intending
a compound spatial index.

## Storage Structure

For a compound spatial index with prefix fields `(F1, F2, ...)` and
spatial field `S`:

```
// BTree prefix (default):
idx_{name}: BTreeMap<(F1, F2, ...), SpatialIndex<PK, Point>>
idx_{name}_none: BTreeMap<(F1, F2, ...), NoneSet<PK>>

// Hash prefix:
idx_{name}: InsOrdHashMap<(F1, F2, ...), SpatialIndex<PK, Point>>
idx_{name}_none: InsOrdHashMap<(F1, F2, ...), NoneSet<PK>>
```

`NoneSet<PK>` is shorthand used throughout this document for the
primary-storage-dependent none-set type: `BTreeSet<PK>` for btree
primary storage or `InsOrdHashMap<PK, ()>` for hash primary storage
(matching the existing single-field spatial convention). This type does
not exist in the codebase — the codegen emits the concrete type directly.

**Single-prefix-field tuple elision:** With a single prefix field, the
tuple wrapper is elided: `BTreeMap<F1, SpatialIndex<PK, Point>>`. This
follows the existing pattern in `gen_hash_key_expr` (line 211 of
`table.rs`), which already elides tuples for single-field hash keys.
The same elision must apply everywhere a prefix key is constructed or
destructured: field declarations, insert/update/remove expressions,
query method parameters, and partition lookup expressions.

**Lazy partition management:** R-tree partitions are created on first
insert into that partition. When a partition's R-tree and none-set are
both empty (after removes), the partition entry is removed from the
outer map. This avoids accumulating stale empty partitions. Partition
cleanup must be checked after *every* remove operation — including the
`true→false` branch of filtered updates where the remove fires but the
insert doesn't.

## Option Handling on Prefix Fields

If a prefix field is `Option<T>`, then `Option<T>` is used directly as
the map key. `None` is a valid BTreeMap/HashMap key, so rows with a
`None` prefix naturally end up in a `None`-keyed partition with their own
R-tree. No special casing required — the existing Option-as-key semantics
handle it.

If the spatial field is `Option<T>`, the existing none-set mechanism
applies per-partition: each partition has its own none-set for rows where
the spatial field is `None`.

## Filter Interaction

Filters on compound spatial indexes work identically to single-field
spatial indexes. The filter predicate gates whether a row enters any
index structure at all. The four-branch insert/update logic
(`old_passes × new_passes`) applies unchanged — the only difference is
that insert/remove operations target a specific partition's R-tree or
none-set based on the prefix key values.

## Generated Query Methods

For an index named `by_zone_pos` with prefix field `zone_id: ZoneId` and
spatial field `pos: Option<VoxelBox>`:

```rust
/// Returns cloned rows in the zone whose spatial key intersects the envelope.
/// Results sorted by PK.
fn intersecting_by_zone_pos(&self, zone_id: &ZoneId, envelope: &VoxelBox) -> Vec<Row>

/// Iterator variant — avoids cloning the full result set.
fn iter_intersecting_by_zone_pos(
    &self, zone_id: &ZoneId, envelope: &VoxelBox
) -> Box<dyn Iterator<Item = &Row> + '_>

/// Count variant — no allocation.
fn count_intersecting_by_zone_pos(&self, zone_id: &ZoneId, envelope: &VoxelBox) -> usize
```

For multi-field prefixes, each prefix field is a separate parameter:

```rust
fn intersecting_by_zone_civ_pos(
    &self, zone_id: &ZoneId, civ_id: &CivId, envelope: &VoxelBox
) -> Vec<Row>
```

If the prefix key has no matching partition in the outer map, all methods
return empty results / zero count. Because only filter-passing rows are
inserted into the R-tree, query methods do not need to re-check the
filter predicate.

**Note on envelope type:** The envelope parameter type is derived from
the spatial field's `SpatialKey` implementation (via `MaybeSpatialKey`),
not a concrete type. The examples above use `VoxelBox` for illustration;
in generated code, the type is
`<<FieldType as MaybeSpatialKey>::Key as SpatialKey>::Key` (i.e.,
whatever the unwrapped spatial key type is).

### Query Codegen Dispatch

`ResolvedIndex.kind` is `Spatial` for compound spatial indexes (same as
single-field spatial). The existing `gen_query_methods` dispatches to
`gen_spatial_query_methods` when `idx.kind == Spatial`. Within
`gen_spatial_query_methods` (and all other per-index codegen functions:
`gen_idx_field_decls`, `gen_idx_field_inits`, `gen_idx_insert`,
`gen_idx_update`, `gen_idx_remove`, `gen_rebuild_body`), the compound
vs. single-field codepath is selected by branching on
`idx.compound_spatial.is_some()`.

### Query Codegen Pattern

The generated query methods follow a lookup-partition-then-delegate
pattern. Pseudocode for `intersecting_by_zone_pos`:

```rust
pub fn intersecting_by_zone_pos(&self, zone_id: &ZoneId, __envelope: &EnvelopeTy) -> Vec<Row> {
    let __partition = match self.idx_by_zone_pos.get(zone_id) {
        Some(p) => p,
        None => return Vec::new(),  // no partition = no results
    };
    let __pks: Vec<PK> = __partition.intersecting(__envelope);
    __pks.into_iter()
        .filter_map(|__pk| self.rows.get(&__pk).cloned())
        .collect()
}
```

The `iter_intersecting_` and `count_intersecting_` variants follow the
same partition-lookup prefix. For `count_`, the partition's
`count_intersecting` is called directly (no allocation). For `iter_`,
the PKs are collected then mapped to row references (same pattern as the
existing single-field spatial `iter_intersecting_`, line 2325-2330 of
`table.rs`).

For multi-field prefixes, the partition lookup uses a tuple key:
`self.idx_by_zone_civ_pos.get(&(*zone_id, *civ_id))` (with appropriate
cloning). Single-field prefixes use the bare key, following the same
elision as `gen_hash_key_expr`.

**Note on pseudocode paths:** Pseudocode in this document omits
`::tabulosity::` prefixes for readability (e.g., `SpatialIndex::new()`
instead of `::tabulosity::SpatialIndex::new()`). The actual generated
code must use fully-qualified paths, as existing codegen does.

## Index Maintenance Codegen

### Field Declarations (`gen_idx_field_decls`)

When `compound_spatial` is `Some`, instead of emitting a single
`SpatialIndex<PK, Point>`, emit:

```rust
// BTree prefix:
idx_{name}: BTreeMap<PrefixKeyTy, SpatialIndex<PK, Point>>
idx_{name}_none: BTreeMap<PrefixKeyTy, NoneSet<PK>>

// Hash prefix:
idx_{name}: InsOrdHashMap<PrefixKeyTy, SpatialIndex<PK, Point>>
idx_{name}_none: InsOrdHashMap<PrefixKeyTy, NoneSet<PK>>
```

Where `PrefixKeyTy` is the single prefix field type (elided tuple) or
`(F1, F2, ...)` for multiple prefix fields.

**Initialization in `new()`:** `gen_idx_field_inits` initializes both
outer maps to empty (`BTreeMap::new()` or `InsOrdHashMap::new()`
depending on prefix kind). Partitions are lazy — no R-trees or none-sets
are created until the first insert into a given prefix key.

### Insert (`gen_idx_insert`)

For compound spatial, the generated insert code:

```rust
// (inside filter guard if applicable)
let __prefix_key = row.zone_id.clone();  // or tuple for multi-prefix
let __partition = self.idx_{name}
    .entry(__prefix_key.clone())
    .or_insert_with(|| SpatialIndex::new());
match MaybeSpatialKey::as_spatial(&row.pos) {
    Some(__key) => {
        __partition.insert(__key, pk.clone());
    }
    None => {
        self.idx_{name}_none
            .entry(__prefix_key)
            // NoneSet = BTreeSet or InsOrdHashMap per primary_storage (see Storage Structure)
            .or_insert_with(|| NoneSet::new())
            .insert(pk.clone());
    }
}
```

### Update (`gen_idx_update`)

The existing `gen_idx_update_with_stmts` helper uses a single
`field_changed_check` and unified remove+insert statements. For compound
spatial, the update needs a three-way check because the prefix key and
spatial field can change independently. This conflicts with the outer
`field_changed_check` that `gen_idx_update_with_stmts` would apply, so
compound spatial update codegen **bypasses `gen_idx_update_with_stmts`
entirely** and implements the full filter four-branch logic itself.

The complete generated update code for a filtered compound spatial index:

```rust
{
    let old_passes = filter_fn(&old_row);
    let new_passes = filter_fn(&row);
    match (old_passes, new_passes) {
        (true, true) => {
            let __prefix_changed = old_row.zone_id != row.zone_id;
            let __spatial_changed = old_row.pos != row.pos;

            if __prefix_changed || __spatial_changed {
                // Remove from old partition
                let __old_prefix = old_row.zone_id.clone();
                match MaybeSpatialKey::as_spatial(&old_row.pos) {
                    Some(__key) => {
                        if let Some(__p) = self.idx_{name}.get_mut(&__old_prefix) {
                            __p.remove(__key, &pk);
                        }
                    }
                    None => {
                        if let Some(__ns) = self.idx_{name}_none.get_mut(&__old_prefix) {
                            __ns.remove(&pk);
                        }
                    }
                }
                // Partition cleanup targets the old prefix key only. The new
                // prefix key was just populated by the insert below, so it
                // never needs cleanup in the update path.
                // Optimization: when !__prefix_changed, the old and new
                // prefix are identical, so the insert below guarantees the
                // partition is non-empty — cleanup can be skipped entirely.
                // The codegen may gate the cleanup block on __prefix_changed.
                let __rtree_empty = self.idx_{name}
                    .get(&__old_prefix).map_or(true, |p| p.is_empty());
                let __none_empty = self.idx_{name}_none
                    .get(&__old_prefix).map_or(true, |s| s.is_empty());
                if __rtree_empty && __none_empty {
                    self.idx_{name}.remove(&__old_prefix);
                    self.idx_{name}_none.remove(&__old_prefix);
                }

                // Insert into new partition
                let __new_prefix = row.zone_id.clone();
                let __partition = self.idx_{name}
                    .entry(__new_prefix.clone())
                    .or_insert_with(|| SpatialIndex::new());
                match MaybeSpatialKey::as_spatial(&row.pos) {
                    Some(__key) => { __partition.insert(__key, pk.clone()); }
                    None => {
                        self.idx_{name}_none
                            .entry(__new_prefix)
                            .or_insert_with(|| NoneSet::new())
                            .insert(pk.clone());
                    }
                }
            }
        }
        (true, false) => {
            // Row leaving the filter — remove only, with partition cleanup.
            let __old_prefix = old_row.zone_id.clone();
            match MaybeSpatialKey::as_spatial(&old_row.pos) {
                Some(__key) => {
                    if let Some(__p) = self.idx_{name}.get_mut(&__old_prefix) {
                        __p.remove(__key, &pk);
                    }
                }
                None => {
                    if let Some(__ns) = self.idx_{name}_none.get_mut(&__old_prefix) {
                        __ns.remove(&pk);
                    }
                }
            }
            let __rtree_empty = self.idx_{name}
                .get(&__old_prefix).map_or(true, |p| p.is_empty());
            let __none_empty = self.idx_{name}_none
                .get(&__old_prefix).map_or(true, |s| s.is_empty());
            if __rtree_empty && __none_empty {
                self.idx_{name}.remove(&__old_prefix);
                self.idx_{name}_none.remove(&__old_prefix);
            }
        }
        (false, true) => {
            // Row entering the filter — insert only.
            let __new_prefix = row.zone_id.clone();
            let __partition = self.idx_{name}
                .entry(__new_prefix.clone())
                .or_insert_with(|| SpatialIndex::new());
            match MaybeSpatialKey::as_spatial(&row.pos) {
                Some(__key) => { __partition.insert(__key, pk.clone()); }
                None => {
                    self.idx_{name}_none
                        .entry(__new_prefix)
                        .or_insert_with(|| NoneSet::new())
                        .insert(pk.clone());
                }
            }
        }
        (false, false) => {}
    }
}
```

For unfiltered compound spatial indexes, the generated code is just the
`(true, true)` body (the change-detection + remove + cleanup + insert
block) without the filter match wrapper.

This mirrors the structure of `gen_idx_update_with_stmts` but with the
compound spatial change-detection and partition logic inlined, avoiding
the conflicting layering of two independent change-check mechanisms.

### Remove (`gen_idx_remove`)

```rust
let __prefix = row.zone_id.clone();
match MaybeSpatialKey::as_spatial(&row.pos) {
    Some(__key) => {
        if let Some(__partition) = self.idx_{name}.get_mut(&__prefix) {
            __partition.remove(__key, &pk);
        }
    }
    None => {
        if let Some(__none_set) = self.idx_{name}_none.get_mut(&__prefix) {
            __none_set.remove(&pk);
        }
    }
}
// Partition cleanup
let __rtree_empty = self.idx_{name}
    .get(&__prefix).map_or(true, |p| p.is_empty());
let __none_empty = self.idx_{name}_none
    .get(&__prefix).map_or(true, |s| s.is_empty());
if __rtree_empty && __none_empty {
    self.idx_{name}.remove(&__prefix);
    self.idx_{name}_none.remove(&__prefix);
}
```

### Rebuild (`gen_rebuild_body`)

Clear the outer maps, then iterate all rows with get-or-create-partition:

```rust
self.idx_{name}.clear();
self.idx_{name}_none.clear();
for (pk, row) in &self.rows {
    // (filter guard if applicable)
    let __prefix = row.zone_id.clone();
    match MaybeSpatialKey::as_spatial(&row.pos) {
        Some(__key) => {
            self.idx_{name}
                .entry(__prefix)
                .or_insert_with(|| SpatialIndex::new())
                .insert(__key, pk.clone());
        }
        None => {
            self.idx_{name}_none
                .entry(__prefix)
                .or_insert_with(|| NoneSet::new())
                .insert(pk.clone());
        }
    }
}
```

## `modify_unchecked` Safety

All fields in a compound spatial index — both prefix fields and the
spatial field — are "indexed fields" for the purpose of
`modify_unchecked` debug assertions. The existing codegen in
`gen_modify_unchecked` (line 1812-1821 of `table.rs`) already collects
all fields from all indexes into `indexed_fields`, so prefix fields will
be included automatically with no special handling needed.

## Bounds Tracking

`collect_unique_tracked_types` (table.rs lines 985-1017) skips Hash and
Spatial indexes entirely — neither kind needs `_bounds_<suffix>` fields
for `MatchAll` / range queries. For compound spatial indexes, the
overall `kind` is `Spatial`, so they are also skipped by the existing
check. However, compound spatial indexes with a **btree** prefix need
bounds tracking for the prefix field types, since the outer `BTreeMap`
supports ordered iteration and range queries on the prefix key.

The fix: in `collect_unique_tracked_types`, when `idx.kind == Spatial`
and `compound_spatial.is_some()` with `prefix_kind == BTree`, iterate
the prefix fields (via `prefix_field_indices`) and add their types to
the tracked set, following the same `type_suffix` dedup pattern as
regular btree indexes. Hash-prefix compound spatial indexes do not
participate in bounds tracking, matching existing hash index behavior.

## Serde

Compound spatial indexes are transient like single-field spatial indexes
(`serde(skip)` equivalent). Rebuilt from row data on deserialization via
`rebuild_all_indexes()`.

## Compile-Time Validation

- Spatial field must be last in the field list.
- At most one spatial field per index.
- Spatial field cannot be a PK field (existing constraint).
- Spatial field type must implement `SpatialKey` (or `Option<T: SpatialKey>`).
- All prefix fields must resolve to the same kind.
- Compound spatial indexes cannot be `unique` (existing constraint for spatial).
- Prefix field trait bounds (`Ord + Clone` for btree, `Hash + Eq + Clone`
  for hash) are not explicitly checked by the derive macro — Rust's type
  system catches violations at the generated code level, producing errors
  at the struct definition site.

## Changelog

### v2

- Fixed update codegen cleanup bug: none-set removal was incorrectly gated on R-tree partition existence; restructured to handle R-tree and none-set removal independently, matching the remove codegen pattern (HIGH #1).
- Added migration note for `IndexDecl.fields` type change from `Vec<String>` to `Vec<IndexFieldDecl>`, listing affected consumers (MEDIUM #2).
- Added rationale for `CompoundSpatialInfo` storing field indices rather than `(Ident, Type)` pairs (MEDIUM #3).
- Clarified that the parse-time multi-field spatial error is safely replaced by resolution-time validation, covering the `kind = "spatial"` + multiple fields case (MEDIUM #4).
- Added `new()` initialization note for compound spatial field declarations (MEDIUM #5).
- Added note that query methods do not re-check filter predicates since only filter-passing rows enter the R-tree (LOW #6).
- Defined `NoneSet<PK>` explicitly as document shorthand, noting it is not a real codebase type (LOW #7).
- Clarified that single-field spatial syntax equivalence is at the resolved level, not the parsed level (LOW #8).
- Added note that pseudocode omits `::tabulosity::` path prefixes for readability (LOW #9).

### v3

- Added "Bounds Tracking" section: compound spatial indexes with btree prefixes participate in bounds tracking for prefix field types via `collect_unique_tracked_types` (HIGH #1).
- Rewrote update codegen to show compound spatial bypassing `gen_idx_update_with_stmts` entirely, implementing the full four-branch filter logic itself with all branches shown explicitly (HIGH #2).
- Added inline comment in update pseudocode clarifying that partition cleanup targets the old prefix key only (MEDIUM #3).
- Added "Query Codegen Dispatch" subsection specifying that `ResolvedIndex.kind` remains `Spatial` for compound spatial indexes and all per-index codegen functions branch on `compound_spatial.is_some()` (MEDIUM #4).
- Enumerated specific call sites affected by `IndexDecl.fields` type change: `resolve_indexes`, `validate_index_decl`, `parse_index_attrs` (MEDIUM #5).
- Made `gen_idx_field_inits` explicit in the initialization note, clarifying lazy partition semantics (MEDIUM #6).
- Added error message quality note for `kind = "spatial"` + multiple fields case, recommending a targeted error pointing users to per-field annotation syntax (LOW #7).
- Replaced `BTreeSet::new()` with `NoneSet::new()` in insert and rebuild pseudocode, matching the document's `NoneSet<PK>` shorthand (LOW #8).
- Skipped LOW #9 (InsOrdHashMap import path) — the review itself noted no action needed.

### v4

- Removed `try_parse_compound_pk` from migration note — it parses `#[primary_key]` into `CompoundPkParsed`, not `IndexDecl`, so the `IndexDecl.fields` type change does not affect it (MEDIUM #1).
- Added explicit step 5 in resolution logic: set `kind = Spatial` whenever a spatial field is detected, whether single-field or compound, ensuring downstream codegen dispatch is correct (MEDIUM #2).
- Added optimization note in update `(true, true)` branch: partition cleanup can be skipped when `!__prefix_changed` since the insert guarantees the partition is non-empty (LOW #3).
- Added sentence clarifying comma consumption is unchanged after optional ident in `fields(...)` parsing (LOW #4).
- Skipped LOW #5 (fragile line references) — the review itself noted no action needed.
- Added inline `NoneSet` reminder comment at first usage in insert pseudocode (LOW #6).
