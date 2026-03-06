# modify_unchecked -- Design Draft v1

Closure-based in-place mutation that bypasses index maintenance for
performance, with debug-build safety checks.

## Motivation

The sim's hottest inner loop is the creature heartbeat, which decrements
`food` and `rest` on every creature every `heartbeat_interval` ticks. With
tabulosity's current API, mutating a non-indexed payload field requires:

```rust
let mut creature = db.creatures.get(&id).unwrap().clone();
creature.food -= decay;
db.update_creature(creature)?;
```

This clones the entire row, runs index diff logic (comparing old vs new
indexed field values), and checks FK constraints -- all for a field that
appears in zero indexes and zero FK relationships. `modify_unchecked`
eliminates this overhead for the common case of mutating payload-only fields.

## Design

### Table-level method

```rust
pub fn modify_unchecked(
    &mut self,
    pk: &PK,
    f: impl FnOnce(&mut Row),
) -> Result<(), Error>
```

**Behavior:**
1. Look up `&mut Row` in `self.rows` via `BTreeMap::get_mut`.
2. If not found, return `Error::NotFound`.
3. In `#[cfg(debug_assertions)]` builds only: snapshot the PK and all
   indexed field values (clone just those fields, not the whole row).
4. Call `f(&mut row)`.
5. In `#[cfg(debug_assertions)]` builds only: assert that the PK and all
   indexed field values are unchanged. The assertion message names the
   specific field that changed.
6. Return `Ok(())`.

**Release build cost:** `BTreeMap::get_mut` + closure invocation. No
index maintenance, no cloning, no FK checks.

**Debug build cost:** Additionally clones the PK and each indexed field
(not the whole row), then compares after the closure.

### Database-level method

```rust
pub fn modify_unchecked_{singular}(
    &mut self,
    pk: &PK,
    f: impl FnOnce(&mut Row),
) -> Result<(), Error>
```

Delegates directly to the table's `modify_unchecked`. No FK re-check is
needed because:

- The PK cannot change (enforced by debug assert).
- FK field changes are the caller's responsibility and a bug if they
  happen -- but this is a conscious tradeoff for performance/ergonomics.
  The `_unchecked` suffix signals this contract.

### Name rationale

`modify_unchecked` follows Rust naming convention (like `get_unchecked`,
`from_utf8_unchecked`) for "safety invariant is the caller's
responsibility." The `_unchecked` suffix clearly signals that index and
FK maintenance is bypassed. At the Database level the same name is used:
`modify_unchecked_{singular}`.

### Why not `get_mut`?

A raw `&mut Row` return would give tabulosity no hook to run before/after
checks. The closure style enables:
- Debug-build snapshots before the closure runs.
- Debug-build assertions after the closure returns.
- Future extensions (e.g., dirty-bit tracking for change notifications)
  without API changes.

### Filtered index limitation

If a mutation changes a field used in a `filter = "..."` predicate, the
row might need to enter or leave a filtered index. This is NOT checked
even in debug builds -- the filter function would need to be re-evaluated,
which has unbounded cost and would defeat the purpose of `_unchecked`.

**Document this as a known limitation:** if you are changing fields used
in filter predicates, use `update()` (or the future `update_with()`)
instead.

## Generated Code Sketch

Given this table definition from the tests:

```rust
#[derive(Table, Clone, Debug)]
struct Creature {
    #[primary_key]
    id: CreatureId,
    #[indexed]
    species: Species,
    name: String,
    food: i64,
    rest: i64,
}
```

The derive macro would generate:

```rust
impl CreatureTable {
    pub fn modify_unchecked(
        &mut self,
        pk: &CreatureId,
        f: impl FnOnce(&mut Creature),
    ) -> Result<(), tabulosity::Error> {
        let row = match self.rows.get_mut(pk) {
            Some(r) => r,
            None => {
                return Err(tabulosity::Error::NotFound {
                    table: "creatures",
                    key: format!("{:?}", pk),
                });
            }
        };

        // Debug-only: snapshot PK + indexed fields before mutation.
        #[cfg(debug_assertions)]
        let __snap_id = row.id.clone();
        #[cfg(debug_assertions)]
        let __snap_species = row.species.clone();

        f(row);

        // Debug-only: verify PK + indexed fields unchanged.
        #[cfg(debug_assertions)]
        {
            debug_assert!(
                row.id == __snap_id,
                "modify_unchecked: primary key field `id` was changed \
                 (from {:?} to {:?}); use update() instead",
                __snap_id, row.id,
            );
            debug_assert!(
                row.species == __snap_species,
                "modify_unchecked: indexed field `species` was changed \
                 (from {:?} to {:?}); use update() instead",
                __snap_species, row.species,
            );
        }

        Ok(())
    }
}
```

For compound indexes (`#[index(name = "...", fields("a", "b"))]`), all
fields mentioned in any index are snapshotted. Each field is snapshotted
once even if it appears in multiple indexes (deduplicated by field name,
same as bounds widening).

At the Database level:

```rust
impl GameDb {
    pub fn modify_unchecked_creature(
        &mut self,
        pk: &CreatureId,
        f: impl FnOnce(&mut Creature),
    ) -> Result<(), tabulosity::Error> {
        self.creatures.modify_unchecked(pk, f)
    }
}
```

## Usage Examples

### Decrementing food/rest on heartbeat

```rust
// Before (current API):
let mut creature = db.creatures.get(&cid).unwrap().clone();
creature.food = (creature.food - food_decay).max(0);
creature.rest = (creature.rest - rest_decay).max(0);
db.update_creature(creature)?;

// After:
db.modify_unchecked_creature(&cid, |c| {
    c.food = (c.food - food_decay).max(0);
    c.rest = (c.rest - rest_decay).max(0);
})?;
```

### Updating creature position (non-indexed)

```rust
// Assuming position is not indexed:
db.modify_unchecked_creature(&cid, |c| {
    c.current_node = Some(next_node);
    c.move_from = c.position;
    c.move_to = next_pos;
    c.move_start_tick = now;
    c.move_end_tick = now + travel_ticks;
    c.position = next_pos;
})?;
```

### Changing task progress

```rust
db.modify_unchecked_task(&tid, |t| {
    t.progress += 1.0;
})?;
```

### Anti-pattern: changing an indexed field (caught in debug)

```rust
// This will debug_assert! in debug builds:
db.modify_unchecked_creature(&cid, |c| {
    c.species = Species::Capybara;  // BUG: species is indexed!
});
```

## Test Plan

### Table-level tests

1. **modify_unchecked on existing row succeeds** -- insert a row, call
   `modify_unchecked` to change a non-indexed field, verify the change
   persists via `get`.

2. **modify_unchecked on missing row returns NotFound** -- call on an
   empty table, assert `Error::NotFound`.

3. **Debug build catches PK change** -- use `std::panic::catch_unwind`
   to call `modify_unchecked` with a closure that changes the PK field.
   Assert the panic message contains `"primary key"`. Gate with
   `#[cfg(debug_assertions)]`.

4. **Debug build catches indexed field change** -- same approach, but
   change an `#[indexed]` field. Assert the panic message contains the
   field name.

5. **Debug build catches compound index field change** -- change a field
   that appears only in a `#[index(...)]` compound index. Assert the
   panic message contains the field name.

6. **Non-indexed field changes are fine** -- modify a field that is not
   the PK and not in any index. No panic, value updated correctly.

7. **Multiple modifications** -- call `modify_unchecked` multiple times
   on the same row, verify all changes accumulate.

### Database-level tests

8. **Database delegation works** -- insert via `insert_{singular}`, then
   `modify_unchecked_{singular}`, verify the change.

9. **NotFound propagated through database** -- call on a missing PK,
   assert `Error::NotFound`.

### Index integrity tests

10. **Indexes remain valid after modify_unchecked** -- insert rows with
    indexed fields, `modify_unchecked` non-indexed fields, then query via
    index methods. Verify index queries still return correct results
    (proving the indexes were not corrupted).

## Relationship to F-tab-update-with

`modify_unchecked` and `update_with` (F-tab-update-with) are
complementary:

| | `modify_unchecked` | `update_with` |
|---|---|---|
| Index maintenance | None (caller's responsibility) | Automatic |
| FK re-check | None | Yes |
| Debug safety | PK + indexed field assertions | Full correctness |
| Performance | Fastest possible | Slightly slower than raw update |
| Use case | Hot-path payload mutations | General-purpose mutation |

Both should be implemented. `modify_unchecked` is simpler (no index
diffing logic) and can land first. `update_with` can follow and would
internally snapshot, mutate, then run the existing index update logic.

## Implementation Notes

### Proc macro changes (tabulosity_derive/src/table.rs)

Add a new method generation step alongside the existing mutation methods.
The method needs:

- The PK field ident and type (already available).
- All indexed field idents and types, deduplicated by field name. This
  is similar to the bounds widening logic -- collect from resolved
  indexes, dedup by field ident string.
- Conditional compilation via `#[cfg(debug_assertions)]` in the
  generated code.

### Proc macro changes (tabulosity_derive/src/database.rs)

Add one `modify_unchecked_{singular}` method per table that delegates to
the table method. This is a one-liner per table, similar to how the
existing write methods are generated but without FK checks.
