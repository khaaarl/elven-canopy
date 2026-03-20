//! Proc macro crate for tabulosity.
//!
//! Provides `#[derive(Bounded)]`, `#[derive(Table)]`, and `#[derive(Database)]`
//! macros. Users should depend on the `tabulosity` crate, which re-exports these
//! derives alongside the runtime types they reference.
//!
//! Implementation is split across modules:
//! - `bounded.rs` — `#[derive(Bounded)]` for newtype wrappers.
//! - `table.rs` — `#[derive(Table)]` companion struct generation.
//! - `database.rs` — `#[derive(Database)]` FK validation and write methods.
//! - `parse.rs` — shared attribute parsing (primary_key, indexed).

use proc_macro::TokenStream;
use syn::DeriveInput;

mod bounded;
mod database;
mod parse;
mod table;

/// Derives the `Bounded` and `AutoIncrementable` traits for single-field
/// tuple structs (newtypes).
///
/// Generates `Bounded::MIN` and `Bounded::MAX` by delegating to the inner
/// type's `Bounded` impl, and `AutoIncrementable::first()` / `successor()`
/// by delegating to the inner type's `AutoIncrementable` impl.
#[proc_macro_derive(Bounded)]
pub fn derive_bounded(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    bounded::derive(&input).into()
}

/// Derives a companion `{Name}Table` struct with typed CRUD operations and
/// optional secondary indexes.
///
/// Recognizes field attributes: `#[primary_key]` or `#[primary_key(auto_increment)]`
/// (exactly one for single-column PKs), `#[auto_increment]` (at most one non-PK field),
/// `#[indexed]` or `#[indexed(unique)]` (zero or more). Struct-level
/// `#[primary_key("field1", "field2")]` declares a compound (multi-column) primary key
/// with a tuple key type. Struct-level `#[index(...)]` supports `unique` keyword for
/// compound unique indexes.
#[proc_macro_derive(Table, attributes(primary_key, auto_increment, indexed, index, table))]
pub fn derive_table(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    table::derive(&input).into()
}

/// Derives safe write methods with foreign key validation on a database
/// schema struct.
///
/// Every field must have a `#[table(singular = "...", fks(...), auto, nonpk_auto)]`
/// attribute. The `auto` flag generates an `insert_{singular}_auto` method for
/// PK auto-increment tables. The `nonpk_auto` flag marks tables with a non-PK
/// `#[auto_increment]` field, enabling correct serde deserialization format.
/// Within `fks(...)`, the `pk` keyword marks an FK field as also being the
/// child table's primary key, enabling 1:1 parent-child relationships.
/// An optional struct-level `#[schema_version(N)]` attribute enables schema
/// versioning: the version number is included in serialized output and
/// checked on deserialization.
#[proc_macro_derive(Database, attributes(table, schema_version))]
pub fn derive_database(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    database::derive(&input).into()
}
