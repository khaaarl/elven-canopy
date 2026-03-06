//! Tabulosity — a lightweight, typed, in-memory relational database library.
//!
//! Provides typed tables with primary keys, automatic secondary indexes
//! (simple, compound, filtered, and unique), auto-increment primary keys,
//! cross-table foreign key integrity, and `modify_unchecked` closure-based
//! in-place mutation (bypasses index maintenance, with debug-build safety).
//! All internal data structures use `BTreeMap`/`BTreeSet` for deterministic
//! iteration order.
//!
//! # Determinism constraint
//!
//! Tabulosity is intentionally fully deterministic: all internal data structures
//! use `BTreeMap`/`BTreeSet`, and all generated (derived) code emits only
//! ordered collections. This is a hard requirement for its primary consumer,
//! `elven_canopy_sim`, which must produce identical results given the same seed
//! for lockstep multiplayer and replay verification. **`HashMap` and `HashSet`
//! must never be introduced** into library code, generated code, or test
//! helpers — even where iteration order appears irrelevant — because hash
//! ordering varies across platforms, Rust versions, and process invocations.
//!
//! # Crate layout
//!
//! - `error.rs` — `Error` enum and `DeserializeError` for write failures.
//! - `table.rs` — `Bounded` trait, `FkCheck` trait, `IntoQuery`/`QueryBound`/
//!   `MatchAll` query types, `in_bounds` helper, and range bound helpers used
//!   by generated code.
//!
//! The companion proc macro crate `tabulosity_derive` provides
//! `#[derive(Bounded)]`, `#[derive(Table)]`, and `#[derive(Database)]`.
//! This crate re-exports those derives so users only need `use tabulosity::*`.

mod error;
mod table;

pub use error::{DeserializeError, Error};
pub use table::{
    AutoIncrementable, Bounded, FkCheck, IntoQuery, MatchAll, QueryBound, TableMeta, in_bounds,
};

// Re-export derives so users write `use tabulosity::Table` etc.
pub use tabulosity_derive::{Bounded, Database, Table};
