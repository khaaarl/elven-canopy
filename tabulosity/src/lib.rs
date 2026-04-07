//! Tabulosity — a lightweight, typed, in-memory relational database library.
//!
//! Provides typed tables with primary keys, automatic secondary indexes
//! (simple, compound, filtered, unique, and spatial via R-tree), auto-increment
//! primary keys, cross-table foreign key integrity, `modify_unchecked`
//! closure-based in-place mutation (bypasses index maintenance, with
//! debug-build safety), `QueryOpts` for ordering (asc/desc) and offset
//! (skip N) on all query methods, and `modify_each_by_*` query-driven batch
//! mutation with the same debug-build safety checks.
//! All internal data structures use ordered collections (`BTreeMap`/`BTreeSet`
//! or `InsOrdHashMap`) for deterministic iteration order.
//!
//! # Determinism constraint
//!
//! Tabulosity is intentionally fully deterministic: all internal data structures
//! use `BTreeMap`/`BTreeSet`, and all generated (derived) code emits only
//! ordered collections. This is a hard requirement for its primary consumer,
//! `elven_canopy_sim`, which must produce identical results given the same seed
//! for lockstep multiplayer and replay verification. **`HashMap` and `HashSet`
//! must never be iterated** in library code, generated code, or test helpers —
//! hash ordering varies across platforms, Rust versions, and process invocations.
//! The one exception is `InsOrdHashMap`, which uses a `HashMap` internally for
//! O(1) lookup but never iterates it — all iteration goes through an ordered vec.
//!
//! # Crate layout
//!
//! - `error.rs` — `Error` enum and `DeserializeError` for write failures.
//! - `ins_ord_hash_map.rs` — `InsOrdHashMap<K, V>`, an insertion-ordered hash
//!   map with deterministic iteration via tombstone-skip vec. O(1) lookup and
//!   removal with automatic compaction.
//! - `one_or_many.rs` — `OneOrMany<V, Many>` enum for non-unique hash index
//!   groups, optimizing the common single-entry case. `RemoveResult` for
//!   signaling empty/removed/not-found.
//! - `spatial.rs` — `SpatialKey` trait, `SpatialPoint` marker, `MaybeSpatialKey`
//!   dispatch trait, and `SpatialIndex` R-tree wrapper (doc-hidden, used by
//!   generated code). Backed by the `rstar` crate.
//! - `table.rs` — `Bounded` trait, `FkCheck` trait, `IntoQuery`/`QueryBound`/
//!   `MatchAll` query types, `QueryOrder`/`QueryOpts` for ordering and offset,
//!   `in_bounds` helper, and range bound helpers used by generated code.
//! - `crc.rs` — `Crc32State` hasher and `CrcFeed` trait for per-row CRC32
//!   checksumming. Used by `#[table(checksummed)]` tables for incremental
//!   table-level XOR-aggregated checksums (desync detection).
//!
//! The companion proc macro crate `tabulosity_derive` provides
//! `#[derive(Bounded)]`, `#[derive(Table)]`, `#[derive(CrcFeed)]`, and
//! `#[derive(Database)]`.
//! This crate re-exports those derives so users only need `use tabulosity::*`.

pub mod crc;
mod error;
mod ins_ord_hash_map;
mod one_or_many;
mod spatial;
mod table;

pub use crc::{Crc32State, CrcFeed, crc32_of};
pub use error::{DeserializeError, Error};
pub use ins_ord_hash_map::InsOrdHashMap;
pub use one_or_many::{HasLen, OneOrMany, RemoveResult};
pub use spatial::{MaybeSpatialKey, SpatialIndex, SpatialKey, SpatialPoint};
pub use table::{
    AutoIncrementable, Bounded, FkCheck, IntoQuery, MatchAll, QueryBound, QueryOpts, QueryOrder,
    TableMeta, in_bounds,
};

// Re-export derives so users write `use tabulosity::Table` etc.
pub use tabulosity_derive::{Bounded, CrcFeed, Database, Table};
