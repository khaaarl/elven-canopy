# sim_db_v9: Implementation Notes and Errata

Addendum to the sim_db design series (v1–v8, now superseded by
`docs/drafts/tabulosity_design_reference.md`). Documents design clarifications and ambiguities
found during the v1 implementation review. These are not design changes — they
describe the current behavior of the shipped code and note areas for future
improvement.

## 2a. Range methods do not require `Bounded` on field type

The v8 doc (§ "Range methods and `Bounded`") states that generated range query
methods include a `where FieldType: Bounded` bound, so they simply won't be
callable for non-`Bounded` types like `String`.

**Current behavior (divergence):** The implementation generates range methods
without an explicit `where FieldType: Bounded` bound. The `Bounded` trait is
only used for the *primary key type* in `map_start_bound`/`map_end_bound`
(constructing composite `(Field, PK)` bounds). The field type itself only needs
`Ord + Clone`, which it already has from the `#[indexed]` requirement.

**Effect:** Range queries work on any `Ord + Clone` indexed field, including
`String`. This is strictly more permissive than the v8 spec. In practice this
is fine — there is no technical reason to restrict range queries to `Bounded`
types, and `String` range queries (e.g., `by_name_range("A".."M")`) are useful.

**Decision:** Keep the current behavior. The v8 spec was overly restrictive.

## 4a. FK checks short-circuit on insert/update

The v8 doc specifies that `remove_*` collects ALL inbound FK violations before
returning an error (no short-circuit). It is silent on the behavior of
`insert_*`/`update_*`/`upsert_*` for outbound FK validation.

**Current behavior (intentional):** Insert, update, and upsert short-circuit on
the first FK target miss. If a row has two FK fields and the first fails
validation, the second is not checked — the error reports only the first missing
target.

**Rationale:** The asymmetry is deliberate. For outbound FK validation
(insert/update), the caller needs to fix the first problem before retrying
anyway, so reporting all failures adds no value. For inbound FK validation
(remove), the caller needs the full picture — knowing all referencing rows is
necessary to decide whether to delete dependents, reassign them, or abort.

## 4b. DeserializeError wrapped in serde error

The v8 doc implies `DeserializeError` is the direct return type from database
deserialization, but in practice it is wrapped via `serde::de::Error::custom()`.
This makes `DeserializeError` inaccessible as a typed value from callers — they
receive a `serde` error whose message happens to contain the `DeserializeError`
display string.

**Current behavior:** Tests work around this by checking error message strings
(e.g., asserting the message contains `"foreign key"` or `"duplicate"`).

**Why:** The `Deserialize` trait requires returning `D::Error`, not an arbitrary
custom type. There is no way to return `DeserializeError` directly through
serde's deserialization pathway.

**Potential future improvement:** Add a separate `from_value` entrypoint (or
similar) that bypasses serde's error type and returns
`Result<Db, DeserializeError>` directly. This would let callers match on
specific error variants without string inspection. The serde `Deserialize` impl
would remain as-is for compatibility with generic deserialization frameworks.

## 4c. Row type name derived by string trimming

The `Database` derive macro obtains the row type name by stripping the `"Table"`
suffix from the field's type name. For example, a field typed `CreatureTable`
produces generated code referencing `Creature` as the row type.

**Current behavior:** This works by convention but is not validated at macro
expansion time. A field typed `CreatureTbl` would silently generate code
referencing `CreatureTbl` (without stripping), which would fail at compile time
with a confusing "type not found" error pointing at generated code rather than
at the naming mismatch.

**Convention (must follow):** Table companion structs MUST be named
`{RowName}Table`. This is how the derive macro discovers the row type.

**Potential future improvement:** Add an explicit `row = "..."` attribute to
`#[table(...)]`, e.g. `#[table(row = "Creature")]`. This would make the
association explicit, remove the naming convention requirement, and produce a
clear error if the attribute is missing. The current suffix-stripping behavior
could remain as a fallback default.
