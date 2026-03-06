//! Tabulosity — a lightweight, typed, in-memory relational database library.
//!
//! Provides typed tables with primary keys, automatic secondary indexes
//! (simple, compound, and filtered), and cross-table foreign key integrity.
//! All internal data structures use `BTreeMap`/`BTreeSet` for deterministic
//! iteration order.
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
