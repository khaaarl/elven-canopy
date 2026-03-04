//! Shared attribute parsing utilities for tabulosity derive macros.
//!
//! Extracts `#[primary_key]` and `#[indexed]` annotations from struct fields.
//! Used by `table.rs` during `#[derive(Table)]` expansion.

use syn::{Field, Ident, Type};

/// A parsed struct field with its tabulosity annotations.
pub struct ParsedField {
    pub ident: Ident,
    pub ty: Type,
    pub is_primary_key: bool,
    pub is_indexed: bool,
}

/// Parse all fields from a named struct, extracting tabulosity attributes.
pub fn parse_fields(fields: &syn::FieldsNamed) -> Vec<ParsedField> {
    fields.named.iter().map(parse_field).collect()
}

fn parse_field(field: &Field) -> ParsedField {
    let ident = field.ident.clone().expect("named field");
    let ty = field.ty.clone();
    let is_primary_key = field.attrs.iter().any(|a| a.path().is_ident("primary_key"));
    let is_indexed = field.attrs.iter().any(|a| a.path().is_ident("indexed"));
    ParsedField {
        ident,
        ty,
        is_primary_key,
        is_indexed,
    }
}
