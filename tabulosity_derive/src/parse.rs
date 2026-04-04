//! Shared attribute parsing utilities for tabulosity derive macros.
//!
//! Extracts `#[primary_key]`, `#[primary_key(auto_increment)]`, `#[auto_increment]`,
//! `#[indexed]` / `#[indexed(hash)]` / `#[indexed(unique)]` / `#[indexed(spatial)]`
//! / `#[indexed(hash, unique)]` annotations from struct fields,
//! `#[primary_key("field1", "field2")]` from struct-level attributes (compound PKs),
//! `#[index(name = "...", fields("a", "b" spatial), kind = "hash"|"btree"|"spatial",
//! filter = "...", unique)]` from struct-level attributes (with optional per-field kind
//! keywords), and `#[table(primary_storage = "hash")]` from the struct being derived.
//! Used by `table.rs` during `#[derive(Table)]` expansion.

use syn::parse::{Parse, ParseStream};
use syn::{DeriveInput, Field, Ident, LitStr, Token, Type};

/// The backing storage kind for an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKind {
    /// BTreeSet-backed index (default). O(log n) lookup, supports range queries.
    BTree,
    /// InsOrdHashMap-backed index. O(1) lookup, deterministic insertion-order
    /// iteration. Does not support range queries.
    Hash,
    /// R-tree-backed spatial index. Supports intersection queries on axis-aligned
    /// bounding boxes. Field type must implement `SpatialKey` (or `Option<T>`
    /// where `T: SpatialKey`). Cannot be combined with `unique`.
    Spatial,
}

/// The backing storage kind for a table's primary key storage (`rows` field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryStorageKind {
    /// `BTreeMap<PK, Row>` — sorted by PK (default).
    BTree,
    /// `InsOrdHashMap<PK, Row>` — O(1) lookup, insertion-order iteration.
    Hash,
}

/// A parsed struct field with its tabulosity annotations.
pub struct ParsedField {
    pub ident: Ident,
    pub ty: Type,
    pub is_primary_key: bool,
    pub is_auto_increment: bool,
    pub is_indexed: bool,
    pub is_unique: bool,
    /// The index kind for field-level `#[indexed]` / `#[indexed(hash)]`.
    /// Only meaningful when `is_indexed` is true.
    pub index_kind: IndexKind,
}

/// A single field within an `#[index(fields(...))]` declaration.
///
/// Each field has a name (string literal) and an optional per-field kind
/// override. When `kind_override` is `None`, the field inherits the
/// index-level `kind` parameter (default: `BTree`).
pub struct IndexFieldDecl {
    pub name: String,
    /// Per-field kind override (`btree`, `hash`, or `spatial` keyword after
    /// the field name string). `None` = inherit from index-level `kind`.
    pub kind_override: Option<IndexKind>,
}

/// A parsed `#[index(...)]` struct-level attribute.
pub struct IndexDecl {
    pub name: String,
    pub fields: Vec<IndexFieldDecl>,
    pub filter: Option<String>,
    pub unique: bool,
    /// The index kind. Defaults to `BTree` if `kind` is omitted.
    pub kind: IndexKind,
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
    // Check for standalone `#[auto_increment]` (non-PK auto-increment).
    for attr in &field.attrs {
        if attr.path().is_ident("auto_increment") {
            if is_auto_increment {
                return Err(syn::Error::new_spanned(
                    attr,
                    "field already has auto_increment via #[primary_key(auto_increment)]; remove the redundant #[auto_increment]",
                ));
            }
            if is_primary_key {
                return Err(syn::Error::new_spanned(
                    attr,
                    "#[auto_increment] cannot be used on a #[primary_key] field; use #[primary_key(auto_increment)] instead",
                ));
            }
            // Reject arguments: `#[auto_increment]` takes no args.
            if let syn::Meta::List(meta_list) = &attr.meta {
                return Err(syn::Error::new_spanned(
                    meta_list,
                    "#[auto_increment] does not accept arguments",
                ));
            }
            is_auto_increment = true;
        }
    }
    let mut is_indexed = false;
    let mut is_unique = false;
    let mut index_kind = IndexKind::BTree;
    for attr in &field.attrs {
        if attr.path().is_ident("indexed") {
            is_indexed = true;
            // Parse comma-separated identifiers: hash, unique, btree.
            if let syn::Meta::List(meta_list) = &attr.meta {
                let parsed: IndexedArgs = syn::parse2(meta_list.tokens.clone())?;
                is_unique = parsed.unique;
                index_kind = parsed.kind;
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
        index_kind,
    })
}

/// Parsed arguments from `#[indexed(hash, unique)]`.
struct IndexedArgs {
    unique: bool,
    kind: IndexKind,
}

impl Parse for IndexedArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut unique = false;
        let mut has_hash = false;
        let mut has_btree = false;
        let mut has_spatial = false;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "unique" => {
                    if unique {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `unique` in #[indexed(...)]",
                        ));
                    }
                    unique = true;
                }
                "hash" => {
                    if has_hash {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `hash` in #[indexed(...)]",
                        ));
                    }
                    has_hash = true;
                }
                "btree" => {
                    if has_btree {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `btree` in #[indexed(...)]",
                        ));
                    }
                    has_btree = true;
                }
                "spatial" => {
                    if has_spatial {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `spatial` in #[indexed(...)]",
                        ));
                    }
                    has_spatial = true;
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown #[indexed(...)] argument: `{other}`; expected `hash`, `btree`, `spatial`, or `unique`"
                        ),
                    ));
                }
            }
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        let kind_count = has_hash as u8 + has_btree as u8 + has_spatial as u8;
        if kind_count > 1 {
            return Err(input.error(
                "cannot combine `hash`, `btree`, and `spatial` in #[indexed(...)]; pick one kind",
            ));
        }

        if has_spatial && unique {
            return Err(input.error(
                "spatial indexes cannot be unique; spatial queries use intersection, not equality",
            ));
        }

        let kind = if has_hash {
            IndexKind::Hash
        } else if has_spatial {
            IndexKind::Spatial
        } else {
            IndexKind::BTree
        };

        Ok(IndexedArgs { unique, kind })
    }
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
                kind: decl.kind,
            });
        }
    }
    Ok(decls)
}

/// Internal parsed form for `#[index(name = "...", fields("a", "b" spatial), kind = "hash", filter = "...", unique)]`.
struct IndexDeclParsed {
    name: String,
    fields: Vec<IndexFieldDecl>,
    filter: Option<String>,
    unique: bool,
    kind: IndexKind,
}

impl Parse for IndexDeclParsed {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut fields = None;
        let mut filter = None;
        let mut unique = false;
        let mut kind = IndexKind::BTree;

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
                    let mut field_decls = Vec::new();
                    while !content.is_empty() {
                        let lit: LitStr = content.parse()?;
                        // Optionally consume a per-field kind keyword after the string.
                        let kind_override = if content.peek(Ident) {
                            let kind_ident: Ident = content.parse()?;
                            match kind_ident.to_string().as_str() {
                                "btree" => Some(IndexKind::BTree),
                                "hash" => Some(IndexKind::Hash),
                                "spatial" => Some(IndexKind::Spatial),
                                other => {
                                    return Err(syn::Error::new(
                                        kind_ident.span(),
                                        format!(
                                            "unknown per-field kind: `{other}`; expected `btree`, `hash`, or `spatial`"
                                        ),
                                    ));
                                }
                            }
                        } else {
                            None
                        };
                        field_decls.push(IndexFieldDecl {
                            name: lit.value(),
                            kind_override,
                        });
                        if content.peek(Token![,]) {
                            let _: Token![,] = content.parse()?;
                        }
                    }
                    fields = Some(field_decls);
                }
                "filter" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    filter = Some(lit.value());
                }
                "kind" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    match lit.value().as_str() {
                        "hash" => kind = IndexKind::Hash,
                        "btree" => kind = IndexKind::BTree,
                        "spatial" => kind = IndexKind::Spatial,
                        other => {
                            return Err(syn::Error::new_spanned(
                                &lit,
                                format!(
                                    "unknown index kind: `{other}`; expected `\"hash\"`, `\"btree\"`, or `\"spatial\"`"
                                ),
                            ));
                        }
                    }
                }
                "unique" => {
                    unique = true;
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown index attribute key: `{other}`; expected `name`, `fields`, `kind`, `filter`, or `unique`"
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

        if kind == IndexKind::Spatial && unique {
            return Err(input.error(
                "spatial indexes cannot be unique; spatial queries use intersection, not equality",
            ));
        }

        Ok(IndexDeclParsed {
            name,
            fields,
            filter,
            unique,
            kind,
        })
    }
}

/// Parse an optional struct-level `#[table(primary_storage = "hash")]` attribute.
/// Returns `BTree` (the default) if no such attribute exists.
pub fn parse_table_attr(input: &DeriveInput) -> syn::Result<PrimaryStorageKind> {
    let mut result = PrimaryStorageKind::BTree;
    let mut found = false;

    for attr in &input.attrs {
        if attr.path().is_ident("table") {
            if found {
                return Err(syn::Error::new_spanned(
                    attr,
                    "duplicate #[table(...)] attribute; only one is allowed",
                ));
            }
            found = true;
            let parsed: TableAttrParsed = attr.parse_args()?;
            result = parsed.primary_storage;
        }
    }

    Ok(result)
}

/// Internal parsed form for `#[table(primary_storage = "hash")]`.
struct TableAttrParsed {
    primary_storage: PrimaryStorageKind,
}

impl Parse for TableAttrParsed {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut primary_storage = PrimaryStorageKind::BTree;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "primary_storage" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    match lit.value().as_str() {
                        "hash" => primary_storage = PrimaryStorageKind::Hash,
                        "btree" => primary_storage = PrimaryStorageKind::BTree,
                        other => {
                            return Err(syn::Error::new_spanned(
                                &lit,
                                format!(
                                    "unknown primary_storage value: `{other}`; expected `\"hash\"` or `\"btree\"`"
                                ),
                            ));
                        }
                    }
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!("unknown #[table(...)] key: `{other}`; expected `primary_storage`"),
                    ));
                }
            }
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        Ok(TableAttrParsed { primary_storage })
    }
}
