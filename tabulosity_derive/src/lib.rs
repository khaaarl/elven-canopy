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
/// (exactly one required), `#[indexed]` or `#[indexed(unique)]` (zero or more).
/// Struct-level `#[index(...)]` supports `unique` keyword for compound unique indexes.
#[proc_macro_derive(Table, attributes(primary_key, indexed, index))]
pub fn derive_table(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    table::derive(&input).into()
}

/// Derives safe write methods with foreign key validation on a database
/// schema struct.
///
/// Every field must have a `#[table(singular = "...", fks(...), auto)]`
/// attribute. The `auto` flag generates an `insert_{singular}_auto` method.
#[proc_macro_derive(Database, attributes(table))]
pub fn derive_database(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    database::derive(&input).into()
}
