//! Shared attribute parsing utilities for tabulosity derive macros.
//!
//! Extracts `#[primary_key]`, `#[indexed]`, and `#[indexed(unique)]` annotations
//! from struct fields, `#[primary_key("field1", "field2")]` from struct-level
//! attributes (compound PKs), and `#[index(name = "...", fields(...), filter = "...",
//! unique)]` from struct-level attributes. Used by `table.rs` during `#[derive(Table)]`
//! expansion.

use syn::parse::{Parse, ParseStream};
use syn::{DeriveInput, Field, Ident, LitStr, Token, Type};

/// A parsed struct field with its tabulosity annotations.
pub struct ParsedField {
    pub ident: Ident,
    pub ty: Type,
    pub is_primary_key: bool,
    pub is_auto_increment: bool,
    pub is_indexed: bool,
    pub is_unique: bool,
}

/// A parsed `#[index(...)]` struct-level attribute.
pub struct IndexDecl {
    pub name: String,
    pub fields: Vec<String>,
    pub filter: Option<String>,
    pub unique: bool,
}

/// Parse all fields from a named struct, extracting tabulosity attributes.
pub fn parse_fields(fields: &syn::FieldsNamed) -> syn::Result<Vec<ParsedField>> {
    fields.named.iter().map(parse_field).collect()
}

fn parse_field(field: &Field) -> syn::Result<ParsedField> {
    let ident = field.ident.clone().expect("named field");
    let ty = field.ty.clone();
    let mut is_primary_key = false;
    let mut is_auto_increment = false;
    for attr in &field.attrs {
        if attr.path().is_ident("primary_key") {
            is_primary_key = true;
            // Check for `#[primary_key(auto_increment)]`.
            if let syn::Meta::List(meta_list) = &attr.meta {
                let parsed: syn::Result<Ident> = syn::parse2(meta_list.tokens.clone());
                match parsed {
                    Ok(id) if id == "auto_increment" => {
                        is_auto_increment = true;
                    }
                    Ok(id) => {
                        return Err(syn::Error::new(
                            id.span(),
                            format!(
                                "unknown #[primary_key(...)] argument: `{id}`; expected `auto_increment`"
                            ),
                        ));
                    }
                    Err(e) => {
                        return Err(syn::Error::new(
                            e.span(),
                            "invalid #[primary_key(...)] syntax; expected `auto_increment`",
                        ));
                    }
                }
            }
        }
    }
    let mut is_indexed = false;
    let mut is_unique = false;
    for attr in &field.attrs {
        if attr.path().is_ident("indexed") {
            is_indexed = true;
            // Check for `#[indexed(unique)]`.
            if let syn::Meta::List(meta_list) = &attr.meta {
                let parsed: syn::Result<Ident> = syn::parse2(meta_list.tokens.clone());
                match parsed {
                    Ok(id) if id == "unique" => {
                        is_unique = true;
                    }
                    Ok(id) => {
                        return Err(syn::Error::new(
                            id.span(),
                            format!("unknown #[indexed(...)] argument: `{id}`; expected `unique`"),
                        ));
                    }
                    Err(e) => {
                        return Err(syn::Error::new(
                            e.span(),
                            "invalid #[indexed(...)] syntax; expected `unique`",
                        ));
                    }
                }
            }
        }
    }
    Ok(ParsedField {
        ident,
        ty,
        is_primary_key,
        is_auto_increment,
        is_indexed,
        is_unique,
    })
}

/// Parse an optional struct-level `#[primary_key("field1", "field2")]` attribute
/// for compound primary keys. Returns `None` if no such attribute exists.
pub fn parse_compound_pk_attr(input: &DeriveInput) -> syn::Result<Option<Vec<String>>> {
    for attr in &input.attrs {
        if attr.path().is_ident("primary_key") {
            let field_names: CompoundPkParsed = attr.parse_args()?;
            if field_names.fields.len() < 2 {
                return Err(syn::Error::new_spanned(
                    attr,
                    "struct-level #[primary_key(...)] requires at least 2 field names; use field-level #[primary_key] for single-column PKs",
                ));
            }
            // Check for duplicate field names.
            let mut seen = std::collections::BTreeSet::new();
            for name in &field_names.fields {
                if !seen.insert(name.as_str()) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        format!(
                            "duplicate field `{}` in #[primary_key(...)]; each field can appear at most once",
                            name
                        ),
                    ));
                }
            }
            return Ok(Some(field_names.fields));
        }
    }
    Ok(None)
}

/// Internal parsed form for `#[primary_key("field1", "field2")]`.
struct CompoundPkParsed {
    fields: Vec<String>,
}

impl Parse for CompoundPkParsed {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut fields = Vec::new();
        while !input.is_empty() {
            let lit: LitStr = input.parse()?;
            fields.push(lit.value());
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }
        Ok(CompoundPkParsed { fields })
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
                unique: decl.unique,
            });
        }
    }
    Ok(decls)
}

/// Internal parsed form for `#[index(name = "...", fields("a", "b"), filter = "...", unique)]`.
struct IndexDeclParsed {
    name: String,
    fields: Vec<String>,
    filter: Option<String>,
    unique: bool,
}

impl Parse for IndexDeclParsed {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut fields = None;
        let mut filter = None;
        let mut unique = false;

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
                "unique" => {
                    unique = true;
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown index attribute key: `{other}`; expected `name`, `fields`, `filter`, or `unique`"
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
            unique,
        })
    }
}
