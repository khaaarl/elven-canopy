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

/// Derives the `Bounded` trait for single-field tuple structs (newtypes).
///
/// Generates `Bounded::MIN` and `Bounded::MAX` by delegating to the inner
/// type's `Bounded` impl.
#[proc_macro_derive(Bounded)]
pub fn derive_bounded(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    bounded::derive(&input).into()
}

/// Derives a companion `{Name}Table` struct with typed CRUD operations and
/// optional secondary indexes.
///
/// Recognizes two field attributes: `#[primary_key]` (exactly one required)
/// and `#[indexed]` (zero or more).
#[proc_macro_derive(Table, attributes(primary_key, indexed))]
pub fn derive_table(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    table::derive(&input).into()
}

/// Derives safe write methods with foreign key validation on a database
/// schema struct.
///
/// Every field must have a `#[table(singular = "...", fks(...))]` attribute.
#[proc_macro_derive(Database, attributes(table))]
pub fn derive_database(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    database::derive(&input).into()
}
