# Tabulosity Spatial Queries & Nested Indexes

**Status: Early draft — designs below are inchoate and need significant
refinement before implementation.** This document captures initial
thinking and open questions. Many details are TBD.

For compound spatial indexes (the immediately actionable design), see the
separate draft: `tabulosity_compound_spatial.md` (F-tab-spatial-2).

---

## Range / Radius Queries (F-tab-spatial-3)

Query methods beyond simple intersection: find all entries within a
distance threshold from a query point.

### Distance Metrics (TBD)

Caller-choosable integer distance metrics:

| Metric | Formula (3D) | Properties |
|--------|-------------|------------|
| Chebyshev | `max(\|dx\|, \|dy\|, \|dz\|)` | Cube iso-surfaces align with R-tree AABBs — tightest pruning. Recommended default. |
| Manhattan | `\|dx\| + \|dy\| + \|dz\|` | Diamond iso-surfaces. |
| Squared Euclidean | `dx² + dy² + dz²` | Avoids sqrt (stays in integers). Sphere iso-surfaces. |

All metrics use `i64` arithmetic to avoid floating-point non-determinism.
Squared Euclidean returns `i64` distance values (squared), so callers
compare against squared thresholds.

**Deterministic ordering:** Results ordered by `(distance, PK)`. Ties on
distance (common with integer coordinates) are broken by PK.

### Open Questions

- **Distance from what to what?** R-tree entries are AABBs, not points.
  Need to define point-to-AABB distance semantics (e.g., distance to
  nearest point of the AABB? center of the AABB?). This affects both the
  API contract and the implementation.
- **Overflow.** Squared Euclidean with `i64` can overflow for large
  coordinate ranges. Need to either document coordinate limits or use
  `i128` for intermediate calculations.
- **`DistanceMetric` type.** Where does this enum live? How does it
  interact with codegen — is it a runtime parameter, or do we generate
  separate methods per metric?
- **API shape.** Rough sketch (types are illustrative, not final):

```rust
fn within_distance_of_pos(
    &self,
    center: &impl SpatialPoint,
    max_distance: i64,
    metric: DistanceMetric,
) -> Vec<(Row, i64)>  // (row, distance) pairs, sorted by (distance, PK)
```

### Implementation Sketch

Use the R-tree's intersection query with a conservatively expanded AABB
(the Chebyshev bounding box of the distance threshold), then post-filter
by exact distance. This leverages the R-tree's spatial pruning while
computing exact distances only for candidates.

---

## KNN Queries (F-tab-spatial-3)

K-nearest-neighbor lookups: "find the K closest entries to point P."

### Approach (TBD)

rstar supports KNN natively via `nearest_neighbor_iter()`, but this
requires implementing `PointDistance` on our `RTreeEntry` wrapper (which
we currently don't), and rstar uses `f64` distances internally.

**Option A: Use rstar's native KNN.**
- Requires `PointDistance` impl on `RTreeEntry`.
- f64 internal distances mean non-deterministic ordering for equidistant
  entries. Would need over-fetching + integer recomputation + re-sorting
  by `(integer_distance, PK)`.
- The "safe margin" for how much to over-fetch is unclear.

**Option B: AABB-expansion approach.**
- Start with a small AABB around the query point, intersect, expand if
  fewer than K results.
- Simpler, fully integer, fully deterministic.
- Less efficient for sparse data (may need multiple expansion rounds).

**Option C: Hybrid.**
- Use rstar's KNN for candidate generation, recompute integer distances,
  sort deterministically.

### Open Questions

- Which approach to use.
- Same point-to-AABB distance question as range queries.
- `i32` overflow in rstar's internal `PointDistance` calculations (our
  coordinates are `i32`; squaring and summing can overflow).

### API Sketch (Illustrative)

```rust
fn nearest_to_pos(
    &self,
    center: &impl SpatialPoint,
    k: usize,
    metric: DistanceMetric,
) -> Vec<(Row, i64)>  // up to K results, sorted by (distance, PK)
```

---

## None-Set Query API (F-tab-spatial-3)

F-tab-spatial stores `None`-keyed PKs in a side container but exposes no
query methods for them.

### Proposed Methods

Using Rust's `None` idiom (not SQL's "null"):

```rust
/// Iterate all rows whose spatial field is None.
fn none_pos(&self) -> Vec<Row>
fn iter_none_pos(&self) -> Box<dyn Iterator<Item = &Row> + '_>
fn count_none_pos(&self) -> usize
```

For compound spatial indexes, scoped by prefix:

```rust
fn none_by_zone_pos(&self, zone_id: &ZoneId) -> Vec<Row>
```

And an unscoped variant that iterates across all partitions:

```rust
fn all_none_pos(&self) -> Vec<Row>
```

### Open Questions

- Cost model for `all_none_pos()` across many partitions — is this a
  footgun that should be gated or documented?
- Should `none_` methods be generated unconditionally, or only when the
  spatial field is `Option<T>`?

---

## General Nested Multi-Kind Compound Indexes (F-tab-nested-idx)

### Vision

The compound spatial index (F-tab-spatial-2) restricts all non-spatial
prefix fields to the same kind. The general nesting extension lifts this
restriction: each "kind boundary" in the field list creates a new nesting
level.

```rust
#[index(name = "deep", fields("a" btree, "b" hash, "c" btree, "d" btree))]
// Storage: BTreeMap<A, InsOrdHashMap<B, BTreeMap<(C, D), BTreeSet<PK>>>>
```

The rule: walk the field list left to right. Consecutive fields with the
same kind are grouped into a single level (tupled if multiple). A kind
change creates a new nesting level.

```
fields("a" btree, "b" btree, "c" hash, "d" btree, "e" spatial)
  → BTreeMap<(A, B), InsOrdHashMap<C, BTreeMap<D, SpatialIndex<PK, Point>>>>
```

### Design Considerations

**Codegen complexity:** Each nesting level adds a layer of map
lookup/insert/remove in the generated code. The derive macro would need
to handle arbitrary nesting depth, likely via recursive codegen or a loop
over nesting levels.

**Query method signatures:** Each level that is a map (not just a tuple
grouping within a level) becomes a parameter in the query method. Tupled
fields within a level are separate parameters:

```rust
// For fields("a" btree, "b" btree, "c" hash, "d" btree, "e" spatial)
fn intersecting_deep(&self, a: &A, b: &B, c: &C, d: &D, envelope: &E) -> Vec<Row>
```

**Partial-prefix queries:** With general nesting, you could potentially
query at any nesting boundary — e.g., "all entries with btree key (A, B)"
regardless of hash key C. Whether to support partial-prefix queries is a
design decision for this item. The simplest initial version requires all
prefix parameters.

**Lazy partition management:** Same principle as compound spatial — each
map level creates/removes entries lazily. Nested empty entries propagate
cleanup upward.

**Priority:** This is a longer-term enhancement. The compound spatial
index (F-tab-spatial-2) covers the immediate need (zone-scoped spatial
queries) and the grammar is forward-compatible with general nesting. No
known blocking use case yet — a concrete motivating example would help
sharpen the design.
