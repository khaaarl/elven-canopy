//! Shared attribute parsing utilities for tabulosity derive macros.
//!
//! Extracts `#[primary_key]` and `#[indexed]` annotations from struct fields,
//! and `#[index(name = "...", fields(...), filter = "...")]` from struct-level
//! attributes. Used by `table.rs` during `#[derive(Table)]` expansion.

use syn::parse::{Parse, ParseStream};
use syn::{DeriveInput, Field, Ident, LitStr, Token, Type};

/// A parsed struct field with its tabulosity annotations.
pub struct ParsedField {
    pub ident: Ident,
    pub ty: Type,
    pub is_primary_key: bool,
    pub is_indexed: bool,
}

/// A parsed `#[index(...)]` struct-level attribute.
pub struct IndexDecl {
    pub name: String,
    pub fields: Vec<String>,
    pub filter: Option<String>,
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

/// Parse all `#[index(...)]` attributes from the struct-level attributes.
pub fn parse_index_attrs(input: &DeriveInput) -> syn::Result<Vec<IndexDecl>> {
    let mut decls = Vec::new();
    for attr in &input.attrs {
        if attr.path().is_ident("index") {
            let decl: IndexDeclParsed = attr.parse_args()?;
            decls.push(IndexDecl {
                name: decl.name,
                fields: decl.fields,
                filter: decl.filter,
            });
        }
    }
    Ok(decls)
}

/// Internal parsed form for `#[index(name = "...", fields("a", "b"), filter = "...")]`.
struct IndexDeclParsed {
    name: String,
    fields: Vec<String>,
    filter: Option<String>,
}

impl Parse for IndexDeclParsed {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut fields = None;
        let mut filter = None;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "name" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    name = Some(lit.value());
                }
                "fields" => {
                    let content;
                    syn::parenthesized!(content in input);
                    let mut field_names = Vec::new();
                    while !content.is_empty() {
                        let lit: LitStr = content.parse()?;
                        field_names.push(lit.value());
                        if content.peek(Token![,]) {
                            let _: Token![,] = content.parse()?;
                        }
                    }
                    fields = Some(field_names);
                }
                "filter" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    filter = Some(lit.value());
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown index attribute key: `{other}`; expected `name`, `fields`, or `filter`"
                        ),
                    ));
                }
            }
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        let name = name.ok_or_else(|| input.error("missing `name = \"...\"` in #[index(...)]"))?;
        let fields = fields.ok_or_else(|| input.error("missing `fields(...)` in #[index(...)]"))?;

        if fields.is_empty() {
            return Err(input.error("fields(...) must contain at least one field name"));
        }

        Ok(IndexDeclParsed {
            name,
            fields,
            filter,
        })
    }
}
