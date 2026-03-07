//! Implementation of `#[derive(Table)]`.
//!
//! Generates a companion `{Name}Table` struct with:
//! - `rows: BTreeMap<PK, Row>` primary storage
//! - Simple indexes from `#[indexed]` fields: `BTreeSet<(FieldType, PK)>`
//! - Unique indexes from `#[indexed(unique)]` or `#[index(..., unique)]`
//! - Compound indexes from `#[index(...)]`: `BTreeSet<(F1, F2, ..., PK)>`
//! - Filtered indexes via optional `filter` on `#[index(...)]`
//! - Tracked bounds per unique type across all indexes: `_bounds_{type}`
//! - Public read methods (get, get_ref, contains, len, is_empty, keys, etc.)
//! - `pk_ref()` on the row type (used by `Database` cascade codegen)
//! - `#[doc(hidden)] pub` mutation methods (_no_fk suffix)
//! - `modify_unchecked` — closure-based in-place mutation bypassing indexes,
//!   with debug-build assertions that PK and indexed fields are unchanged
//! - `modify_unchecked_range` / `modify_unchecked_all` — batch variants using
//!   `BTreeMap::range_mut`, applying `FnMut` per row with the same debug checks
//! - Per-index query methods using `IntoQuery` (by_*, iter_by_*, count_by_*)
//!   with `QueryOpts` for ordering (asc/desc) and offset (skip N)
//! - `modify_each_by_*` — query-driven batch mutation with debug-build safety
//! - `rebuild_indexes()` for deserialization
//! - `Serialize` / `Deserialize` impls (behind `#[cfg(feature = "serde")]`)
//!
//! Uses `parse.rs` for attribute extraction. All generated code uses fully
//! qualified paths to avoid name conflicts in user code.
//!
//! The query API uses tracked runtime bounds (not static `Bounded` trait) for
//! composite BTreeSet range construction, enabling `String` and other types
//! without known MIN/MAX. The `IntoQuery` trait provides a unified API for
//! exact, range, and match-all queries on each field.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Ident, Type};

use crate::parse::{self, IndexDecl, ParsedField};

/// A resolved index — either from `#[indexed]` sugar or `#[index(...)]`.
struct ResolvedIndex {
    name: String,
    /// (field_ident, field_type) in order.
    fields: Vec<(Ident, Type)>,
    filter: Option<String>,
    is_unique: bool,
}

pub fn derive(input: &DeriveInput) -> TokenStream {
    let row_name = &input.ident;
    let vis = &input.vis;
    let table_name = format_ident!("{}Table", row_name);
    let table_name_str = format!("{}s", to_snake_case(&row_name.to_string()));

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => match parse::parse_fields(named) {
                Ok(f) => f,
                Err(e) => return e.to_compile_error(),
            },
            _ => {
                return syn::Error::new_spanned(
                    row_name,
                    "Table can only be derived for structs with named fields",
                )
                .to_compile_error();
            }
        },
        _ => {
            return syn::Error::new_spanned(row_name, "Table can only be derived for structs")
                .to_compile_error();
        }
    };

    // Find primary key field.
    let pk_fields: Vec<&ParsedField> = fields.iter().filter(|f| f.is_primary_key).collect();
    if pk_fields.len() != 1 {
        return syn::Error::new_spanned(
            row_name,
            "Table requires exactly one field with #[primary_key]",
        )
        .to_compile_error();
    }
    let pk_field = pk_fields[0];
    let pk_ident = &pk_field.ident;
    let pk_ty = &pk_field.ty;
    let is_auto_increment = pk_field.is_auto_increment;

    // Parse struct-level #[index(...)] attributes.
    let index_decls = match parse::parse_index_attrs(input) {
        Ok(d) => d,
        Err(e) => return e.to_compile_error(),
    };

    // Resolve all indexes.
    let resolved_indexes = match resolve_indexes(&fields, pk_field, &index_decls, input) {
        Ok(r) => r,
        Err(e) => return e.to_compile_error(),
    };

    // Collect unique tracked types (deduped by suffix).
    let unique_tracked = collect_unique_tracked_types(&resolved_indexes, pk_ty);

    // --- Companion struct fields ---
    let idx_field_decls = gen_idx_field_decls(&resolved_indexes, pk_ty);
    let bounds_field_decls = gen_bounds_field_decls(&unique_tracked);
    let idx_field_inits = gen_idx_field_inits(&resolved_indexes);
    let bounds_field_inits = gen_bounds_field_inits(&unique_tracked);

    // --- Bounds widening (insert/upsert-insert) ---
    let bounds_widen_row = gen_bounds_widen(&resolved_indexes, pk_ty, pk_ident, &fields);

    // --- Index maintenance ---
    let idx_insert = gen_all_idx_insert(&resolved_indexes, pk_ident);
    let idx_update = gen_all_idx_update(&resolved_indexes, pk_ident);
    let idx_upsert_update = gen_all_idx_update(&resolved_indexes, pk_ident);
    let idx_upsert_insert = gen_all_idx_insert(&resolved_indexes, pk_ident);
    let idx_remove = gen_all_idx_remove(&resolved_indexes, pk_ident);

    // --- Rebuild indexes ---
    let bounds_reset = gen_bounds_reset(&unique_tracked);
    let rebuild_body = gen_rebuild_body(&resolved_indexes, pk_ident);
    let rebuild_bounds_widen = gen_bounds_widen(&resolved_indexes, pk_ty, pk_ident, &fields);

    // --- Query methods ---
    let query_methods = gen_all_query_methods(&resolved_indexes, pk_ty, row_name, &fields);

    let table_name_str_ref = &table_name_str;

    // --- modify_each_by_* methods ---
    let modify_each_methods =
        gen_all_modify_each_methods(&resolved_indexes, pk_ident, pk_ty, row_name);

    // --- modify_unchecked (single + range + all) ---
    let modify_unchecked_method = gen_modify_unchecked(
        pk_ident,
        pk_ty,
        row_name,
        table_name_str_ref,
        &resolved_indexes,
    );
    let modify_unchecked_range_methods =
        gen_modify_unchecked_range(pk_ident, pk_ty, row_name, &resolved_indexes);

    // --- Unique index checks ---
    let unique_check_insert =
        gen_unique_checks_insert(&resolved_indexes, pk_ty, table_name_str_ref);
    let unique_check_update =
        gen_unique_checks_update(&resolved_indexes, pk_ty, table_name_str_ref);

    // Auto-increment: optional next_id field, init, bump logic, and methods.
    let next_id_field_decl = if is_auto_increment {
        quote! { next_id: #pk_ty, }
    } else {
        quote! {}
    };
    let next_id_field_init = if is_auto_increment {
        quote! { next_id: <#pk_ty as ::tabulosity::AutoIncrementable>::first(), }
    } else {
        quote! {}
    };
    let next_id_bump_on_insert = if is_auto_increment {
        quote! {
            if pk >= self.next_id {
                self.next_id = <#pk_ty as ::tabulosity::AutoIncrementable>::successor(&pk);
            }
        }
    } else {
        quote! {}
    };
    let next_id_bump_on_upsert = if is_auto_increment {
        quote! {
            if pk >= self.next_id {
                self.next_id = <#pk_ty as ::tabulosity::AutoIncrementable>::successor(&pk);
            }
        }
    } else {
        quote! {}
    };
    let auto_increment_methods = if is_auto_increment {
        quote! {
            /// Returns the next auto-increment ID that will be assigned.
            pub fn next_id(&self) -> #pk_ty {
                self.next_id.clone()
            }

            /// Inserts a row with an auto-assigned primary key.
            /// The closure receives the assigned PK and must return a row with that PK.
            #[doc(hidden)]
            pub fn insert_auto_no_fk(
                &mut self,
                f: impl ::std::ops::FnOnce(#pk_ty) -> #row_name,
            ) -> ::std::result::Result<#pk_ty, ::tabulosity::Error> {
                let pk = self.next_id.clone();
                let row = f(pk.clone());
                debug_assert_eq!(row.#pk_ident, pk);
                self.insert_no_fk(row)?;
                ::std::result::Result::Ok(pk)
            }
        }
    } else {
        quote! {}
    };

    let serde_impls = gen_serde_impls(
        &table_name,
        row_name,
        pk_ty,
        pk_ident,
        table_name_str_ref,
        is_auto_increment,
    );

    quote! {
        /// Companion table struct generated by `#[derive(Table)]`.
        #vis struct #table_name {
            rows: ::std::collections::BTreeMap<#pk_ty, #row_name>,
            #(#idx_field_decls,)*
            #(#bounds_field_decls,)*
            #next_id_field_decl
        }

        impl #table_name {
            /// Creates a new empty table.
            pub fn new() -> Self {
                Self {
                    rows: ::std::collections::BTreeMap::new(),
                    #(#idx_field_inits,)*
                    #(#bounds_field_inits,)*
                    #next_id_field_init
                }
            }

            // --- Read methods ---

            /// Returns a clone of the row with the given primary key, or `None`.
            pub fn get(&self, pk: &#pk_ty) -> ::std::option::Option<#row_name> {
                self.rows.get(pk).cloned()
            }

            /// Returns a reference to the row with the given primary key, or `None`.
            pub fn get_ref(&self, pk: &#pk_ty) -> ::std::option::Option<&#row_name> {
                self.rows.get(pk)
            }

            /// Returns `true` if the table contains a row with the given primary key.
            pub fn contains(&self, pk: &#pk_ty) -> bool {
                self.rows.contains_key(pk)
            }

            /// Returns the number of rows in the table.
            pub fn len(&self) -> usize {
                self.rows.len()
            }

            /// Returns `true` if the table has no rows.
            pub fn is_empty(&self) -> bool {
                self.rows.is_empty()
            }

            /// Returns all primary keys in order (cloned).
            pub fn keys(&self) -> ::std::vec::Vec<#pk_ty> {
                self.rows.keys().cloned().collect()
            }

            /// Iterates over all primary keys in order.
            pub fn iter_keys(&self) -> impl ::std::iter::Iterator<Item = &#pk_ty> {
                self.rows.keys()
            }

            /// Returns all rows in primary key order (cloned).
            pub fn all(&self) -> ::std::vec::Vec<#row_name> {
                self.rows.values().cloned().collect()
            }

            /// Iterates over references to all rows in primary key order.
            pub fn iter_all(&self) -> impl ::std::iter::Iterator<Item = &#row_name> {
                self.rows.values()
            }

            // --- Index query methods ---

            #(#query_methods)*

            #(#modify_each_methods)*

            // --- Mutation methods (doc-hidden, used by Database derive) ---

            /// Inserts a row. Returns `Err(DuplicateKey)` if the PK already exists,
            /// or `Err(DuplicateIndex)` if a unique index constraint is violated.
            #[doc(hidden)]
            pub fn insert_no_fk(&mut self, row: #row_name) -> ::std::result::Result<(), ::tabulosity::Error> {
                let pk = row.#pk_ident.clone();
                if self.rows.contains_key(&pk) {
                    return ::std::result::Result::Err(::tabulosity::Error::DuplicateKey {
                        table: #table_name_str_ref,
                        key: ::std::format!("{:?}", pk),
                    });
                }
                #(#bounds_widen_row)*
                #(#unique_check_insert)*
                #next_id_bump_on_insert
                #(#idx_insert)*
                self.rows.insert(pk, row);
                ::std::result::Result::Ok(())
            }

            #auto_increment_methods

            /// Updates a row. Returns `Err(NotFound)` if the PK is missing,
            /// or `Err(DuplicateIndex)` if a unique index constraint is violated.
            #[doc(hidden)]
            pub fn update_no_fk(&mut self, row: #row_name) -> ::std::result::Result<(), ::tabulosity::Error> {
                let pk = row.#pk_ident.clone();
                let old_row = match self.rows.get(&pk) {
                    Some(r) => r.clone(),
                    None => {
                        return ::std::result::Result::Err(::tabulosity::Error::NotFound {
                            table: #table_name_str_ref,
                            key: ::std::format!("{:?}", pk),
                        });
                    }
                };
                #(#unique_check_update)*
                #(#bounds_widen_row)*
                #(#idx_update)*
                self.rows.insert(pk, row);
                ::std::result::Result::Ok(())
            }

            /// Upserts a row — inserts if missing, updates if existing.
            /// Returns `Err(DuplicateIndex)` if a unique index constraint
            /// is violated.
            #[doc(hidden)]
            pub fn upsert_no_fk(&mut self, row: #row_name) -> ::std::result::Result<(), ::tabulosity::Error> {
                let pk = row.#pk_ident.clone();
                if let Some(old_row) = self.rows.get(&pk).cloned() {
                    #(#unique_check_update)*
                    #next_id_bump_on_upsert
                    #(#bounds_widen_row)*
                    #(#idx_upsert_update)*
                    self.rows.insert(pk, row);
                } else {
                    #(#bounds_widen_row)*
                    #(#unique_check_insert)*
                    #next_id_bump_on_upsert
                    #(#idx_upsert_insert)*
                    self.rows.insert(pk, row);
                }
                ::std::result::Result::Ok(())
            }

            /// Removes a row. Returns the removed row or `Err(NotFound)`.
            #[doc(hidden)]
            pub fn remove_no_fk(&mut self, pk: &#pk_ty) -> ::std::result::Result<#row_name, ::tabulosity::Error> {
                let row = match self.rows.remove(pk) {
                    Some(r) => r,
                    None => {
                        return ::std::result::Result::Err(::tabulosity::Error::NotFound {
                            table: #table_name_str_ref,
                            key: ::std::format!("{:?}", pk),
                        });
                    }
                };
                let pk = row.#pk_ident.clone();
                #(#idx_remove)*
                ::std::result::Result::Ok(row)
            }

            #modify_unchecked_method

            #modify_unchecked_range_methods

            /// Rebuilds all secondary indexes and tracked bounds from row data.
            #[doc(hidden)]
            pub fn rebuild_indexes(&mut self) {
                #(#bounds_reset)*
                #(#rebuild_body)*
                for (_pk, row) in &self.rows {
                    #(#rebuild_bounds_widen)*
                }
            }
        }


        impl #row_name {
            /// Returns a reference to the primary key field.
            #[doc(hidden)]
            pub fn pk_ref(&self) -> &#pk_ty {
                &self.#pk_ident
            }
        }

        impl ::tabulosity::TableMeta for #table_name {
            type Key = #pk_ty;
            type Row = #row_name;
        }

        impl ::std::default::Default for #table_name {
            fn default() -> Self {
                Self::new()
            }
        }

        #serde_impls
    }
}

// =============================================================================
// Index resolution
// =============================================================================

fn resolve_indexes(
    fields: &[ParsedField],
    pk_field: &ParsedField,
    index_decls: &[IndexDecl],
    input: &DeriveInput,
) -> syn::Result<Vec<ResolvedIndex>> {
    let mut resolved = Vec::new();
    let mut used_names = std::collections::BTreeSet::new();

    // Simple indexes from #[indexed].
    for f in fields {
        if f.is_indexed {
            let name = f.ident.to_string();
            if !used_names.insert(name.clone()) {
                return Err(syn::Error::new_spanned(
                    &f.ident,
                    format!("duplicate index name: `{name}`"),
                ));
            }
            resolved.push(ResolvedIndex {
                name,
                fields: vec![(f.ident.clone(), f.ty.clone())],
                filter: None,
                is_unique: f.is_unique,
            });
        }
    }

    // Compound/filtered indexes from #[index(...)].
    for decl in index_decls {
        validate_index_decl(input, decl, fields, pk_field, &used_names)?;
        used_names.insert(decl.name.clone());

        let idx_fields: Vec<(Ident, Type)> = decl
            .fields
            .iter()
            .map(|fname| {
                let f = fields.iter().find(|f| f.ident == fname.as_str()).unwrap();
                (f.ident.clone(), f.ty.clone())
            })
            .collect();

        resolved.push(ResolvedIndex {
            name: decl.name.clone(),
            fields: idx_fields,
            filter: decl.filter.clone(),
            is_unique: decl.unique,
        });
    }

    Ok(resolved)
}

fn validate_index_decl(
    input: &DeriveInput,
    decl: &IndexDecl,
    fields: &[ParsedField],
    pk_field: &ParsedField,
    used_names: &std::collections::BTreeSet<String>,
) -> syn::Result<()> {
    if used_names.contains(&decl.name) {
        return Err(syn::Error::new_spanned(
            input,
            format!(
                "duplicate index name: `{}`; names must be unique across #[indexed] and #[index(...)]",
                decl.name
            ),
        ));
    }
    for fname in &decl.fields {
        let field = fields.iter().find(|f| f.ident == fname.as_str());
        match field {
            None => {
                return Err(syn::Error::new_spanned(
                    input,
                    format!(
                        "index `{}`: field `{}` does not exist on the struct",
                        decl.name, fname
                    ),
                ));
            }
            Some(f) if f.ident == pk_field.ident => {
                return Err(syn::Error::new_spanned(
                    input,
                    format!(
                        "index `{}`: field '{}' is the primary key and is automatically included in every index; remove it from fields(...)",
                        decl.name, fname
                    ),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

// =============================================================================
// Tracked bounds
// =============================================================================

struct TrackedType {
    bounds_suffix: String,
    ty: Type,
}

fn type_suffix(ty: &Type) -> String {
    match ty {
        Type::Path(tp) => {
            let seg = tp.path.segments.last().unwrap();
            let base = to_snake_case(&seg.ident.to_string());
            match &seg.arguments {
                syn::PathArguments::None => base,
                syn::PathArguments::AngleBracketed(args) => {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        let inner_suffix = type_suffix(inner);
                        format!("{}_{}", base, inner_suffix)
                    } else {
                        base
                    }
                }
                _ => base,
            }
        }
        _ => "unknown".to_string(),
    }
}

fn collect_unique_tracked_types(indexes: &[ResolvedIndex], pk_ty: &Type) -> Vec<TrackedType> {
    let mut result = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for idx in indexes {
        for (_, ty) in &idx.fields {
            let suffix = type_suffix(ty);
            if seen.insert(suffix.clone()) {
                result.push(TrackedType {
                    bounds_suffix: suffix,
                    ty: ty.clone(),
                });
            }
        }
    }

    let pk_suffix = type_suffix(pk_ty);
    if seen.insert(pk_suffix.clone()) {
        result.push(TrackedType {
            bounds_suffix: pk_suffix,
            ty: pk_ty.clone(),
        });
    }

    result
}

// =============================================================================
// Struct field codegen
// =============================================================================

fn gen_idx_field_decls(indexes: &[ResolvedIndex], pk_ty: &Type) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| {
            let idx_name = format_ident!("idx_{}", idx.name);
            let field_tys: Vec<&Type> = idx.fields.iter().map(|(_, ty)| ty).collect();
            quote! {
                #idx_name: ::std::collections::BTreeSet<(#(#field_tys,)* #pk_ty)>
            }
        })
        .collect()
}

fn gen_bounds_field_decls(tracked: &[TrackedType]) -> Vec<TokenStream> {
    tracked
        .iter()
        .map(|t| {
            let name = format_ident!("_bounds_{}", t.bounds_suffix);
            let ty = &t.ty;
            quote! { #name: ::std::option::Option<(#ty, #ty)> }
        })
        .collect()
}

fn gen_idx_field_inits(indexes: &[ResolvedIndex]) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| {
            let idx_name = format_ident!("idx_{}", idx.name);
            quote! { #idx_name: ::std::collections::BTreeSet::new() }
        })
        .collect()
}

fn gen_bounds_field_inits(tracked: &[TrackedType]) -> Vec<TokenStream> {
    tracked
        .iter()
        .map(|t| {
            let name = format_ident!("_bounds_{}", t.bounds_suffix);
            quote! { #name: ::std::option::Option::None }
        })
        .collect()
}

// =============================================================================
// Bounds widening codegen
// =============================================================================

fn gen_bounds_widen(
    indexes: &[ResolvedIndex],
    pk_ty: &Type,
    pk_ident: &Ident,
    all_fields: &[ParsedField],
) -> Vec<TokenStream> {
    let mut wideners = Vec::new();
    // Deduplicate by (field_name, type_suffix) so that every field contributing
    // to tracked bounds is widened, even when multiple fields share a type.
    let mut seen_fields = std::collections::BTreeSet::new();

    for idx in indexes {
        for (field_ident, _) in &idx.fields {
            let f = all_fields.iter().find(|f| &f.ident == field_ident).unwrap();
            let suffix = type_suffix(&f.ty);
            let key = field_ident.to_string();
            if seen_fields.insert(key) {
                let bounds_name = format_ident!("_bounds_{}", suffix);
                wideners.push(quote! {
                    match &mut self.#bounds_name {
                        ::std::option::Option::Some((lo, hi)) => {
                            if row.#field_ident < *lo { *lo = row.#field_ident.clone(); }
                            if row.#field_ident > *hi { *hi = row.#field_ident.clone(); }
                        }
                        ::std::option::Option::None => {
                            self.#bounds_name = ::std::option::Option::Some((
                                row.#field_ident.clone(),
                                row.#field_ident.clone(),
                            ));
                        }
                    }
                });
            }
        }
    }

    let pk_suffix = type_suffix(pk_ty);
    let pk_key = pk_ident.to_string();
    if seen_fields.insert(pk_key) {
        let bounds_name = format_ident!("_bounds_{}", pk_suffix);
        wideners.push(quote! {
            match &mut self.#bounds_name {
                ::std::option::Option::Some((lo, hi)) => {
                    if row.#pk_ident < *lo { *lo = row.#pk_ident.clone(); }
                    if row.#pk_ident > *hi { *hi = row.#pk_ident.clone(); }
                }
                ::std::option::Option::None => {
                    self.#bounds_name = ::std::option::Option::Some((
                        row.#pk_ident.clone(),
                        row.#pk_ident.clone(),
                    ));
                }
            }
        });
    }

    wideners
}

fn gen_bounds_reset(tracked: &[TrackedType]) -> Vec<TokenStream> {
    tracked
        .iter()
        .map(|t| {
            let name = format_ident!("_bounds_{}", t.bounds_suffix);
            quote! { self.#name = ::std::option::Option::None; }
        })
        .collect()
}

// =============================================================================
// Unique index check codegen
// =============================================================================

/// Generate uniqueness checks for insert. Runs after bounds widening, before
/// index insertion. For each unique index, checks whether the BTreeSet already
/// contains an entry with the same field values.
fn gen_unique_checks_insert(
    indexes: &[ResolvedIndex],
    pk_ty: &Type,
    table_name_str: &str,
) -> Vec<TokenStream> {
    indexes
        .iter()
        .filter(|idx| idx.is_unique)
        .map(|idx| gen_unique_check_insert(idx, pk_ty, table_name_str))
        .collect()
}

fn gen_unique_check_insert(idx: &ResolvedIndex, pk_ty: &Type, table_name_str: &str) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let idx_name_str = &idx.name;
    let pk_suffix = type_suffix(pk_ty);
    let pk_bounds_name = format_ident!("_bounds_{}", pk_suffix);

    let field_clones: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi.clone() })
        .collect();

    let key_fmt: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi })
        .collect();

    let range_check = quote! {
        if let ::std::option::Option::Some((__pk_min, __pk_max)) = &self.#pk_bounds_name {
            let __start = (#(#field_clones,)* __pk_min.clone());
            let __end = (#(#field_clones,)* __pk_max.clone());
            if self.#idx_name.range(__start..=__end).next().is_some() {
                return ::std::result::Result::Err(::tabulosity::Error::DuplicateIndex {
                    table: #table_name_str,
                    index: #idx_name_str,
                    key: ::std::format!("{:?}", (#(&#key_fmt),*)),
                });
            }
        }
    };

    match &idx.filter {
        Some(filter_path) => {
            let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
            quote! {
                if #filter_fn(&row) {
                    #range_check
                }
            }
        }
        None => range_check,
    }
}

/// Generate uniqueness checks for update. Runs before any mutation.
/// Only checks when field values actually changed.
fn gen_unique_checks_update(
    indexes: &[ResolvedIndex],
    pk_ty: &Type,
    table_name_str: &str,
) -> Vec<TokenStream> {
    indexes
        .iter()
        .filter(|idx| idx.is_unique)
        .map(|idx| gen_unique_check_update(idx, pk_ty, table_name_str))
        .collect()
}

fn gen_unique_check_update(idx: &ResolvedIndex, pk_ty: &Type, table_name_str: &str) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let idx_name_str = &idx.name;
    let pk_suffix = type_suffix(pk_ty);
    let pk_bounds_name = format_ident!("_bounds_{}", pk_suffix);

    let field_clones: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi.clone() })
        .collect();

    let field_changed_check: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { old_row.#fi != row.#fi })
        .collect();

    let key_fmt: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi })
        .collect();

    // For update: old entry with (old_field_val, pk) is still in the set.
    // If field value changed, the old entry won't match the new value search.
    // So any match found is a genuine conflict.
    let range_check = quote! {
        if #(#field_changed_check)||* {
            if let ::std::option::Option::Some((__pk_min, __pk_max)) = &self.#pk_bounds_name {
                let __start = (#(#field_clones,)* __pk_min.clone());
                let __end = (#(#field_clones,)* __pk_max.clone());
                if self.#idx_name.range(__start..=__end).next().is_some() {
                    return ::std::result::Result::Err(::tabulosity::Error::DuplicateIndex {
                        table: #table_name_str,
                        index: #idx_name_str,
                        key: ::std::format!("{:?}", (#(&#key_fmt),*)),
                    });
                }
            }
        }
    };

    match &idx.filter {
        Some(filter_path) => {
            let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
            // Only check if the new row passes the filter.
            // If it doesn't pass, the row won't be in the index, so no conflict.
            // Must also check when transitioning from filtered-out to filtered-in,
            // even if the indexed field values didn't change.
            quote! {
                if #filter_fn(&row) {
                    let __needs_check = !#filter_fn(&old_row) || (#(#field_changed_check)||*);
                    if __needs_check {
                        if let ::std::option::Option::Some((__pk_min, __pk_max)) = &self.#pk_bounds_name {
                            let __start = (#(#field_clones,)* __pk_min.clone());
                            let __end = (#(#field_clones,)* __pk_max.clone());
                            if self.#idx_name.range(__start..=__end).next().is_some() {
                                return ::std::result::Result::Err(::tabulosity::Error::DuplicateIndex {
                                    table: #table_name_str,
                                    index: #idx_name_str,
                                    key: ::std::format!("{:?}", (#(&#key_fmt),*)),
                                });
                            }
                        }
                    }
                }
            }
        }
        None => range_check,
    }
}

// =============================================================================
// Index maintenance codegen
// =============================================================================

fn gen_all_idx_insert(indexes: &[ResolvedIndex], pk_ident: &Ident) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| gen_idx_insert(idx, pk_ident))
        .collect()
}

fn gen_all_idx_update(indexes: &[ResolvedIndex], pk_ident: &Ident) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| gen_idx_update(idx, pk_ident))
        .collect()
}

fn gen_all_idx_remove(indexes: &[ResolvedIndex], pk_ident: &Ident) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| gen_idx_remove(idx, pk_ident))
        .collect()
}

fn gen_idx_insert(idx: &ResolvedIndex, pk_ident: &Ident) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let field_clones: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi.clone() })
        .collect();

    let insert_stmt = quote! {
        self.#idx_name.insert((#(#field_clones,)* row.#pk_ident.clone()));
    };

    match &idx.filter {
        Some(filter_path) => {
            let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
            quote! {
                if #filter_fn(&row) {
                    #insert_stmt
                }
            }
        }
        None => insert_stmt,
    }
}

fn gen_idx_update(idx: &ResolvedIndex, _pk_ident: &Ident) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let old_field_clones: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { old_row.#fi.clone() })
        .collect();
    let new_field_clones: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi.clone() })
        .collect();
    let field_changed_check: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { old_row.#fi != row.#fi })
        .collect();

    match &idx.filter {
        Some(filter_path) => {
            let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
            quote! {
                {
                    let old_passes = #filter_fn(&old_row);
                    let new_passes = #filter_fn(&row);
                    match (old_passes, new_passes) {
                        (true, true) => {
                            if #(#field_changed_check)||* {
                                self.#idx_name.remove(&(#(#old_field_clones,)* pk.clone()));
                                self.#idx_name.insert((#(#new_field_clones,)* pk.clone()));
                            }
                        }
                        (true, false) => {
                            self.#idx_name.remove(&(#(#old_field_clones,)* pk.clone()));
                        }
                        (false, true) => {
                            self.#idx_name.insert((#(#new_field_clones,)* pk.clone()));
                        }
                        (false, false) => {}
                    }
                }
            }
        }
        None => {
            quote! {
                if #(#field_changed_check)||* {
                    self.#idx_name.remove(&(#(#old_field_clones,)* pk.clone()));
                    self.#idx_name.insert((#(#new_field_clones,)* pk.clone()));
                }
            }
        }
    }
}

fn gen_idx_remove(idx: &ResolvedIndex, _pk_ident: &Ident) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let field_clones: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi.clone() })
        .collect();

    quote! {
        self.#idx_name.remove(&(#(#field_clones,)* pk.clone()));
    }
}

/// Generates the `modify_unchecked` method for the table companion struct.
///
/// In debug builds, snapshots the PK and all indexed fields before the closure
/// and asserts they are unchanged after. In release builds, the method is just
/// `BTreeMap::get_mut` + closure.
fn gen_modify_unchecked(
    pk_ident: &Ident,
    pk_ty: &Type,
    row_name: &Ident,
    table_name_str: &str,
    indexes: &[ResolvedIndex],
) -> TokenStream {
    // Collect all indexed field idents (deduplicated by name).
    let mut seen = std::collections::BTreeSet::new();
    let mut indexed_fields: Vec<&Ident> = Vec::new();
    for idx in indexes {
        for (fi, _) in &idx.fields {
            if seen.insert(fi.to_string()) {
                indexed_fields.push(fi);
            }
        }
    }

    // Debug-build snapshot: clone PK + each indexed field.
    let snap_pk = format_ident!("__snap_{}", pk_ident);
    let snap_stmts: Vec<TokenStream> = std::iter::once(quote! {
        #[cfg(debug_assertions)]
        let #snap_pk = row.#pk_ident.clone();
    })
    .chain(indexed_fields.iter().map(|fi| {
        let snap_name = format_ident!("__snap_{}", fi);
        quote! {
            #[cfg(debug_assertions)]
            let #snap_name = row.#fi.clone();
        }
    }))
    .collect();

    // Debug-build assertions: verify PK + each indexed field unchanged.
    let pk_str = pk_ident.to_string();
    let assert_stmts: Vec<TokenStream> = std::iter::once(quote! {
        assert!(
            row.#pk_ident == #snap_pk,
            "modify_unchecked: primary key field `{}` was changed (from {:?} to {:?}); use update() instead",
            #pk_str,
            #snap_pk,
            row.#pk_ident,
        );
    })
    .chain(indexed_fields.iter().map(|fi| {
        let snap_name = format_ident!("__snap_{}", fi);
        let field_str = fi.to_string();
        quote! {
            assert!(
                row.#fi == #snap_name,
                "modify_unchecked: indexed field `{}` was changed (from {:?} to {:?}); use update() instead",
                #field_str,
                #snap_name,
                row.#fi,
            );
        }
    }))
    .collect();

    quote! {
        /// Mutates a row in place via closure, bypassing index maintenance
        /// and FK validation. In debug builds, asserts that the primary key
        /// and all indexed fields are unchanged after the closure returns.
        ///
        /// Use this for hot-path mutations of non-indexed payload fields
        /// (e.g., decrementing `food` or `rest`). If you need to change
        /// indexed fields, use `update_no_fk` instead.
        #[doc(hidden)]
        pub fn modify_unchecked(
            &mut self,
            pk: &#pk_ty,
            f: impl ::std::ops::FnOnce(&mut #row_name),
        ) -> ::std::result::Result<(), ::tabulosity::Error> {
            let row = match self.rows.get_mut(pk) {
                ::std::option::Option::Some(r) => r,
                ::std::option::Option::None => {
                    return ::std::result::Result::Err(::tabulosity::Error::NotFound {
                        table: #table_name_str,
                        key: ::std::format!("{:?}", pk),
                    });
                }
            };

            #(#snap_stmts)*

            f(row);

            #[cfg(debug_assertions)]
            {
                #(#assert_stmts)*
            }

            ::std::result::Result::Ok(())
        }
    }
}

/// Generates `modify_unchecked_range` and `modify_unchecked_all` methods.
///
/// `modify_unchecked_range` uses `BTreeMap::range_mut` to iterate a PK range,
/// applying an `FnMut` closure to each row. `modify_unchecked_all` is sugar
/// for the full range (`..`). Both return the count of rows modified.
///
/// Debug assertions are identical to `modify_unchecked`: per-row snapshots of
/// PK + indexed fields, with post-closure assertions that they are unchanged.
fn gen_modify_unchecked_range(
    pk_ident: &Ident,
    pk_ty: &Type,
    row_name: &Ident,
    indexes: &[ResolvedIndex],
) -> TokenStream {
    // Collect all indexed field idents (deduplicated by name).
    let mut seen = std::collections::BTreeSet::new();
    let mut indexed_fields: Vec<&Ident> = Vec::new();
    for idx in indexes {
        for (fi, _) in &idx.fields {
            if seen.insert(fi.to_string()) {
                indexed_fields.push(fi);
            }
        }
    }

    // Debug-build per-row snapshot + assertion stmts (inside the loop body).
    let snap_stmts: Vec<TokenStream> = indexed_fields
        .iter()
        .map(|fi| {
            let snap_name = format_ident!("__snap_{}", fi);
            quote! {
                let #snap_name = row.#fi.clone();
            }
        })
        .collect();

    let pk_str = pk_ident.to_string();
    let assert_stmts: Vec<TokenStream> = indexed_fields
        .iter()
        .map(|fi| {
            let snap_name = format_ident!("__snap_{}", fi);
            let field_str = fi.to_string();
            quote! {
                assert!(
                    row.#fi == #snap_name,
                    "modify_unchecked_range: indexed field `{}` was changed (from {:?} to {:?}); use update() instead",
                    #field_str,
                    #snap_name,
                    row.#fi,
                );
            }
        })
        .collect();

    quote! {
        /// Mutates all rows in a PK range via closure, bypassing index
        /// maintenance and FK validation. Returns the number of rows modified.
        ///
        /// The closure is `FnMut(&PK, &mut Row)`, called once per row in PK
        /// order. An empty range returns 0.
        ///
        /// In debug builds, asserts per-row that PK and indexed fields are
        /// unchanged after each closure invocation.
        #[doc(hidden)]
        pub fn modify_unchecked_range<__R: ::std::ops::RangeBounds<#pk_ty>>(
            &mut self,
            range: __R,
            mut f: impl ::std::ops::FnMut(&#pk_ty, &mut #row_name),
        ) -> usize {
            let mut __count = 0usize;
            for (__pk, row) in self.rows.range_mut(range) {
                #[cfg(debug_assertions)]
                let __snap_pk = row.#pk_ident.clone();
                #(
                    #[cfg(debug_assertions)]
                    #snap_stmts
                )*

                f(__pk, row);

                #[cfg(debug_assertions)]
                {
                    assert!(
                        row.#pk_ident == __snap_pk,
                        "modify_unchecked_range: primary key field `{}` was changed (from {:?} to {:?}); use update() instead",
                        #pk_str,
                        __snap_pk,
                        row.#pk_ident,
                    );
                    #(#assert_stmts)*
                }

                __count += 1;
            }
            __count
        }

        /// Mutates all rows in the table via closure, bypassing index
        /// maintenance and FK validation. Returns the number of rows modified.
        ///
        /// Equivalent to `modify_unchecked_range(.., f)`.
        #[doc(hidden)]
        pub fn modify_unchecked_all(
            &mut self,
            mut f: impl ::std::ops::FnMut(&#pk_ty, &mut #row_name),
        ) -> usize {
            self.modify_unchecked_range(.., f)
        }
    }
}

fn gen_rebuild_body(indexes: &[ResolvedIndex], _pk_ident: &Ident) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| {
            let idx_name = format_ident!("idx_{}", idx.name);
            let field_clones: Vec<TokenStream> = idx
                .fields
                .iter()
                .map(|(fi, _)| quote! { row.#fi.clone() })
                .collect();

            let insert_stmt = quote! {
                self.#idx_name.insert((#(#field_clones,)* pk.clone()));
            };

            let body = match &idx.filter {
                Some(filter_path) => {
                    let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
                    quote! {
                        for (pk, row) in &self.rows {
                            if #filter_fn(row) {
                                #insert_stmt
                            }
                        }
                    }
                }
                None => {
                    quote! {
                        for (pk, row) in &self.rows {
                            #insert_stmt
                        }
                    }
                }
            };

            quote! {
                self.#idx_name.clear();
                #body
            }
        })
        .collect()
}

// =============================================================================
// Query method codegen
// =============================================================================

fn gen_all_query_methods(
    indexes: &[ResolvedIndex],
    pk_ty: &Type,
    row_name: &Ident,
    all_fields: &[ParsedField],
) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| gen_query_methods(idx, pk_ty, row_name, all_fields))
        .collect()
}

fn gen_query_methods(
    idx: &ResolvedIndex,
    pk_ty: &Type,
    row_name: &Ident,
    all_fields: &[ParsedField],
) -> TokenStream {
    let n = idx.fields.len();
    let by_fn = format_ident!("by_{}", idx.name);
    let iter_by_fn = format_ident!("iter_by_{}", idx.name);
    let count_by_fn = format_ident!("count_by_{}", idx.name);
    let helper_fn = format_ident!("_query_{}", idx.name);
    let idx_name = format_ident!("idx_{}", idx.name);

    let pk_suffix = type_suffix(pk_ty);
    let pk_bounds_name = format_ident!("_bounds_{}", pk_suffix);

    // Parameter declarations.
    let param_names: Vec<Ident> = (0..n).map(|i| format_ident!("__q{}", i)).collect();
    let field_tys: Vec<&Type> = idx.fields.iter().map(|(_, ty)| ty).collect();

    let params_into_query: Vec<TokenStream> = param_names
        .iter()
        .zip(field_tys.iter())
        .map(|(pn, ty)| quote! { #pn: impl ::tabulosity::IntoQuery<#ty> })
        .collect();

    let params_querybound: Vec<TokenStream> = (0..n)
        .map(|i| {
            let qbn = format_ident!("__qb{}", i);
            let ty = field_tys[i];
            quote! { #qbn: ::tabulosity::QueryBound<#ty> }
        })
        .collect();

    // Convert params to QueryBound for the wrapper methods.
    let qb_names: Vec<Ident> = (0..n).map(|i| format_ident!("__qb{}", i)).collect();
    let into_query_calls: Vec<TokenStream> = param_names
        .iter()
        .zip(qb_names.iter())
        .map(|(pn, qbn)| quote! { let #qbn = ::tabulosity::IntoQuery::into_query(#pn); })
        .collect();

    // Field bounds names.
    let field_bounds_names: Vec<Ident> = idx
        .fields
        .iter()
        .map(|(fi, _)| {
            let f = all_fields.iter().find(|f| &f.ident == fi).unwrap();
            format_ident!("_bounds_{}", type_suffix(&f.ty))
        })
        .collect();

    // Generate the match cascade for the helper method.
    let match_cascade = gen_match_cascade(
        n,
        &qb_names,
        &field_tys,
        &field_bounds_names,
        &pk_bounds_name,
        pk_ty,
        &idx_name,
        row_name,
    );

    // Forward calls from public methods to helper.
    let qb_forwards: Vec<TokenStream> = qb_names.iter().map(|qbn| quote! { #qbn }).collect();

    quote! {
        #[doc(hidden)]
        fn #helper_fn(&self, #(#params_querybound,)* __opts: ::tabulosity::QueryOpts) -> ::std::boxed::Box<dyn ::std::iter::Iterator<Item = &#row_name> + '_> {
            #match_cascade
        }

        /// Returns cloned rows matching the query.
        pub fn #by_fn(&self, #(#params_into_query,)* opts: ::tabulosity::QueryOpts) -> ::std::vec::Vec<#row_name> {
            #(#into_query_calls)*
            self.#helper_fn(#(#qb_forwards,)* opts).cloned().collect()
        }

        /// Returns a boxed iterator over references to matching rows.
        pub fn #iter_by_fn(&self, #(#params_into_query,)* opts: ::tabulosity::QueryOpts) -> ::std::boxed::Box<dyn ::std::iter::Iterator<Item = &#row_name> + '_> {
            #(#into_query_calls)*
            self.#helper_fn(#(#qb_forwards,)* opts)
        }

        /// Returns the count of matching rows.
        pub fn #count_by_fn(&self, #(#params_into_query,)* opts: ::tabulosity::QueryOpts) -> usize {
            #(#into_query_calls)*
            self.#helper_fn(#(#qb_forwards,)* opts).count()
        }
    }
}

/// Generate the match cascade for the query helper method.
///
/// Strategy: N+1 match arms, ordered from most exact (all N fields Exact) to
/// least (boundary at position 0 = catch-all).
///
/// For arm with `num_exact` leading Exact fields:
/// - Scan range: exact values for leading fields, tracked min/max for rest + PK.
/// - Post-filter: fields at positions num_exact..N based on their QueryBound.
#[allow(clippy::too_many_arguments)]
fn gen_match_cascade(
    n: usize,
    qb_names: &[Ident],
    field_tys: &[&Type],
    field_bounds_names: &[Ident],
    pk_bounds_name: &Ident,
    pk_ty: &Type,
    idx_name: &Ident,
    row_name: &Ident,
) -> TokenStream {
    let pk_idx = syn::Index::from(n);

    // Generate match arms from most specific to least specific.
    let mut arms = Vec::new();

    for num_exact in (0..=n).rev() {
        let pattern = gen_match_pattern(n, num_exact, qb_names);
        let body = gen_arm_body(
            n,
            num_exact,
            qb_names,
            field_tys,
            field_bounds_names,
            pk_bounds_name,
            pk_ty,
            idx_name,
            row_name,
            &pk_idx,
        );
        arms.push(quote! { #pattern => { #body } });
    }

    let match_tuple: Vec<TokenStream> = qb_names.iter().map(|qbn| quote! { &#qbn }).collect();

    quote! {
        match (#(#match_tuple),*) {
            #(#arms)*
        }
    }
}

/// Generate the match pattern for an arm with `num_exact` leading Exact fields.
fn gen_match_pattern(n: usize, num_exact: usize, _qb_names: &[Ident]) -> TokenStream {
    let pats: Vec<TokenStream> = (0..n)
        .map(|i| {
            if i < num_exact {
                let var = format_ident!("__v{}", i);
                quote! { ::tabulosity::QueryBound::Exact(#var) }
            } else {
                quote! { _ }
            }
        })
        .collect();

    if n == 1 {
        // Single element — not a tuple, just the pattern directly.
        let pat = &pats[0];
        quote! { #pat }
    } else {
        quote! { (#(#pats),*) }
    }
}

/// Generate the body for a match arm with `num_exact` leading Exact fields.
#[allow(clippy::too_many_arguments)]
fn gen_arm_body(
    n: usize,
    num_exact: usize,
    qb_names: &[Ident],
    _field_tys: &[&Type],
    field_bounds_names: &[Ident],
    pk_bounds_name: &Ident,
    _pk_ty: &Type,
    idx_name: &Ident,
    _row_name: &Ident,
    pk_idx: &syn::Index,
) -> TokenStream {
    let empty_ret = quote! {
        return ::std::boxed::Box::new(::std::iter::empty());
    };

    // We always need PK bounds.
    let pk_bounds_check = quote! {
        let ::std::option::Option::Some((__pk_min, __pk_max)) = &self.#pk_bounds_name else {
            #empty_ret
        };
    };

    // For fields num_exact..n, we need their tracked bounds.
    let mut bounds_checks = Vec::new();
    let mut needed_suffixes = std::collections::BTreeSet::new();
    for bn in &field_bounds_names[num_exact..n] {
        let suffix = bn.to_string();
        if needed_suffixes.insert(suffix) {
            let min_var = format_ident!("__{}_min", bn);
            let max_var = format_ident!("__{}_max", bn);
            bounds_checks.push(quote! {
                let ::std::option::Option::Some((#min_var, #max_var)) = &self.#bn else {
                    #empty_ret
                };
            });
        }
    }

    // Construct start tuple elements.
    let start_elems: Vec<TokenStream> = (0..n)
        .map(|i| {
            if i < num_exact {
                let var = format_ident!("__v{}", i);
                quote! { #var.clone() }
            } else {
                let min_var = format_ident!("__{}_min", field_bounds_names[i]);
                quote! { #min_var.clone() }
            }
        })
        .chain(std::iter::once(quote! { __pk_min.clone() }))
        .collect();

    // Construct end tuple elements.
    let end_elems: Vec<TokenStream> = (0..n)
        .map(|i| {
            if i < num_exact {
                let var = format_ident!("__v{}", i);
                quote! { #var.clone() }
            } else {
                let max_var = format_ident!("__{}_max", field_bounds_names[i]);
                quote! { #max_var.clone() }
            }
        })
        .chain(std::iter::once(quote! { __pk_max.clone() }))
        .collect();

    // Generate post-filter for fields num_exact..n.
    let needs_post_filter = num_exact < n;

    if needs_post_filter {
        // Clone the QueryBound values for fields that need post-filtering into
        // local variables so they can be moved into the filter closure.
        let qb_clones: Vec<TokenStream> = (num_exact..n)
            .map(|i| {
                let clone_var = format_ident!("__qbc{}", i);
                let qbn = &qb_names[i];
                quote! { let #clone_var = #qbn.clone(); }
            })
            .collect();

        // Build filter expression.
        let filter_checks: Vec<TokenStream> = (num_exact..n)
            .map(|i| {
                let clone_var = format_ident!("__qbc{}", i);
                let tuple_idx = syn::Index::from(i);
                quote! {
                    (match &#clone_var {
                        ::tabulosity::QueryBound::Exact(__fv) => __entry.#tuple_idx == *__fv,
                        ::tabulosity::QueryBound::Range { start: __fs, end: __fe } => {
                            ::tabulosity::in_bounds(&__entry.#tuple_idx, __fs, __fe)
                        }
                        ::tabulosity::QueryBound::MatchAll => true,
                    })
                }
            })
            .collect();

        let combined_filter = if filter_checks.len() == 1 {
            filter_checks[0].clone()
        } else {
            quote! { #(#filter_checks)&&* }
        };

        // Generate a second set of clone variables for the Desc arm, since
        // the `move` closures consume them.
        let qb_clones_desc: Vec<TokenStream> = (num_exact..n)
            .map(|i| {
                let clone_var = format_ident!("__qbc{}", i);
                let qbn = &qb_names[i];
                quote! { let #clone_var = #qbn.clone(); }
            })
            .collect();

        quote! {
            #pk_bounds_check
            #(#bounds_checks)*
            let __start = (#(#start_elems),*);
            let __end = (#(#end_elems),*);
            match __opts.order {
                ::tabulosity::QueryOrder::Asc => {
                    #(#qb_clones)*
                    ::std::boxed::Box::new(
                        self.#idx_name.range(__start..=__end)
                            .filter(move |__entry| {
                                #combined_filter
                            })
                            .map(|__entry| &self.rows[&__entry.#pk_idx])
                            .skip(__opts.offset)
                    )
                }
                ::tabulosity::QueryOrder::Desc => {
                    #(#qb_clones_desc)*
                    ::std::boxed::Box::new(
                        self.#idx_name.range(__start..=__end)
                            .rev()
                            .filter(move |__entry| {
                                #combined_filter
                            })
                            .map(|__entry| &self.rows[&__entry.#pk_idx])
                            .skip(__opts.offset)
                    )
                }
            }
        }
    } else {
        // All exact — no post-filter needed.
        quote! {
            #pk_bounds_check
            let __start = (#(#start_elems),*);
            let __end = (#(#end_elems),*);
            match __opts.order {
                ::tabulosity::QueryOrder::Asc => ::std::boxed::Box::new(
                    self.#idx_name.range(__start..=__end)
                        .map(|__entry| &self.rows[&__entry.#pk_idx])
                        .skip(__opts.offset)
                ),
                ::tabulosity::QueryOrder::Desc => ::std::boxed::Box::new(
                    self.#idx_name.range(__start..=__end)
                        .rev()
                        .map(|__entry| &self.rows[&__entry.#pk_idx])
                        .skip(__opts.offset)
                ),
            }
        }
    }
}

// =============================================================================
// modify_each_by_* codegen
// =============================================================================

/// Generates one `modify_each_by_{name}` method per index.
///
/// Each method queries matching rows via the existing `_query_{name}` helper,
/// collects their PKs, then iterates and mutates via `rows.get_mut`. In debug
/// builds, snapshots of the PK and all indexed fields are taken before the
/// closure and asserted unchanged after.
fn gen_all_modify_each_methods(
    indexes: &[ResolvedIndex],
    pk_ident: &Ident,
    pk_ty: &Type,
    row_name: &Ident,
) -> Vec<TokenStream> {
    // Collect all indexed field idents (deduplicated) across ALL indexes.
    let mut seen = std::collections::BTreeSet::new();
    let mut indexed_fields: Vec<&Ident> = Vec::new();
    for idx in indexes {
        for (fi, _) in &idx.fields {
            if seen.insert(fi.to_string()) {
                indexed_fields.push(fi);
            }
        }
    }

    indexes
        .iter()
        .map(|idx| gen_modify_each_method(idx, pk_ident, pk_ty, row_name, &indexed_fields))
        .collect()
}

fn gen_modify_each_method(
    idx: &ResolvedIndex,
    pk_ident: &Ident,
    pk_ty: &Type,
    row_name: &Ident,
    indexed_fields: &[&Ident],
) -> TokenStream {
    let n = idx.fields.len();
    let modify_each_fn = format_ident!("modify_each_by_{}", idx.name);
    let helper_fn = format_ident!("_query_{}", idx.name);

    // Parameter declarations (IntoQuery).
    let param_names: Vec<Ident> = (0..n).map(|i| format_ident!("__q{}", i)).collect();
    let field_tys: Vec<&Type> = idx.fields.iter().map(|(_, ty)| ty).collect();

    let params_into_query: Vec<TokenStream> = param_names
        .iter()
        .zip(field_tys.iter())
        .map(|(pn, ty)| quote! { #pn: impl ::tabulosity::IntoQuery<#ty> })
        .collect();

    let qb_names: Vec<Ident> = (0..n).map(|i| format_ident!("__qb{}", i)).collect();
    let into_query_calls: Vec<TokenStream> = param_names
        .iter()
        .zip(qb_names.iter())
        .map(|(pn, qbn)| quote! { let #qbn = ::tabulosity::IntoQuery::into_query(#pn); })
        .collect();

    let qb_forwards: Vec<TokenStream> = qb_names.iter().map(|qbn| quote! { #qbn }).collect();

    // Debug-build snapshot + assertion statements.
    let method_name_str = modify_each_fn.to_string();
    let snap_pk = format_ident!("__snap_{}", pk_ident);
    let snap_stmts: Vec<TokenStream> = std::iter::once(quote! {
        #[cfg(debug_assertions)]
        let #snap_pk = __row.#pk_ident.clone();
    })
    .chain(indexed_fields.iter().map(|fi| {
        let snap_name = format_ident!("__snap_{}", fi);
        quote! {
            #[cfg(debug_assertions)]
            let #snap_name = __row.#fi.clone();
        }
    }))
    .collect();

    let pk_str = pk_ident.to_string();
    let assert_stmts: Vec<TokenStream> = std::iter::once({
        let msg = format!("{method_name_str}: primary key field `{pk_str}` was changed");
        quote! {
            assert!(
                __row.#pk_ident == #snap_pk,
                #msg,
            );
        }
    })
    .chain(indexed_fields.iter().map(|fi| {
        let snap_name = format_ident!("__snap_{}", fi);
        let field_str = fi.to_string();
        let msg = format!("{method_name_str}: indexed field `{field_str}` was changed");
        quote! {
            assert!(
                __row.#fi == #snap_name,
                #msg,
            );
        }
    }))
    .collect();

    quote! {
        /// Mutates all matching rows in place via closure, bypassing index
        /// maintenance. In debug builds, asserts that the primary key and all
        /// indexed fields are unchanged after each closure call. Returns the
        /// number of rows modified.
        pub fn #modify_each_fn(
            &mut self,
            #(#params_into_query,)*
            opts: ::tabulosity::QueryOpts,
            mut f: impl ::std::ops::FnMut(&#pk_ty, &mut #row_name),
        ) -> usize {
            #(#into_query_calls)*
            let __pks: ::std::vec::Vec<#pk_ty> = self.#helper_fn(#(#qb_forwards,)* opts)
                .map(|__r| __r.#pk_ident.clone())
                .collect();
            let mut __count = 0usize;
            for __pk in __pks {
                let __row = self.rows.get_mut(&__pk).unwrap();
                #(#snap_stmts)*
                f(&__pk, __row);
                #[cfg(debug_assertions)]
                {
                    #(#assert_stmts)*
                }
                __count += 1;
            }
            __count
        }
    }
}

// =============================================================================
// Serde codegen
// =============================================================================

fn gen_serde_impls(
    table_name: &Ident,
    row_name: &Ident,
    pk_ty: &Type,
    pk_ident: &Ident,
    table_name_str: &str,
    is_auto_increment: bool,
) -> TokenStream {
    if is_auto_increment {
        gen_serde_impls_auto(table_name, row_name, pk_ty, pk_ident, table_name_str)
    } else {
        gen_serde_impls_plain(table_name, row_name, pk_ty, pk_ident, table_name_str)
    }
}

/// Non-auto tables serialize as a bare JSON array of rows.
fn gen_serde_impls_plain(
    table_name: &Ident,
    row_name: &Ident,
    _pk_ty: &Type,
    pk_ident: &Ident,
    table_name_str: &str,
) -> TokenStream {
    quote! {
        #[cfg(feature = "serde")]
        impl ::serde::Serialize for #table_name
        where
            #row_name: ::serde::Serialize,
        {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
                use ::serde::ser::SerializeSeq;
                let mut seq = serializer.serialize_seq(::std::option::Option::Some(self.rows.len()))?;
                for row in self.rows.values() {
                    seq.serialize_element(row)?;
                }
                seq.end()
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> ::serde::Deserialize<'de> for #table_name
        where
            #row_name: ::serde::Deserialize<'de>,
        {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {
                let rows: ::std::vec::Vec<#row_name> = ::serde::Deserialize::deserialize(deserializer)?;
                let mut table = Self::new();
                for row in rows {
                    let pk = row.#pk_ident.clone();
                    if table.rows.contains_key(&pk) {
                        return ::std::result::Result::Err(::serde::de::Error::custom(
                            ::std::format!("duplicate key in {}: {:?}", #table_name_str, pk),
                        ));
                    }
                    table.rows.insert(pk, row);
                }
                table.rebuild_indexes();
                ::std::result::Result::Ok(table)
            }
        }
    }
}

/// Auto-increment tables serialize as `{"next_id": N, "rows": [...]}`.
fn gen_serde_impls_auto(
    table_name: &Ident,
    row_name: &Ident,
    pk_ty: &Type,
    pk_ident: &Ident,
    table_name_str: &str,
) -> TokenStream {
    let table_name_str_owned = table_name_str.to_string();
    let visitor_name = format_ident!("__{}SerdeVisitor", table_name);
    quote! {
        #[cfg(feature = "serde")]
        impl ::serde::Serialize for #table_name
        where
            #row_name: ::serde::Serialize,
            #pk_ty: ::serde::Serialize,
        {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
                use ::serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(#table_name_str, 2)?;
                state.serialize_field("next_id", &self.next_id)?;
                let rows_vec: ::std::vec::Vec<&#row_name> = self.rows.values().collect();
                state.serialize_field("rows", &rows_vec)?;
                state.end()
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> ::serde::Deserialize<'de> for #table_name
        where
            #row_name: ::serde::Deserialize<'de>,
            #pk_ty: ::serde::Deserialize<'de> + ::tabulosity::AutoIncrementable,
        {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {
                struct #visitor_name;

                impl<'de> ::serde::de::Visitor<'de> for #visitor_name
                where
                    #row_name: ::serde::Deserialize<'de>,
                    #pk_ty: ::serde::Deserialize<'de> + ::tabulosity::AutoIncrementable,
                {
                    type Value = #table_name;

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str(#table_name_str_owned)
                    }

                    fn visit_map<A: ::serde::de::MapAccess<'de>>(
                        self,
                        mut map: A,
                    ) -> ::std::result::Result<Self::Value, A::Error> {
                        let mut next_id: ::std::option::Option<#pk_ty> = ::std::option::Option::None;
                        let mut rows_vec: ::std::option::Option<::std::vec::Vec<#row_name>> = ::std::option::Option::None;

                        while let ::std::option::Option::Some(key) = map.next_key::<::std::string::String>()? {
                            match key.as_str() {
                                "next_id" => {
                                    if next_id.is_some() {
                                        return ::std::result::Result::Err(::serde::de::Error::duplicate_field("next_id"));
                                    }
                                    next_id = ::std::option::Option::Some(map.next_value()?);
                                }
                                "rows" => {
                                    if rows_vec.is_some() {
                                        return ::std::result::Result::Err(::serde::de::Error::duplicate_field("rows"));
                                    }
                                    rows_vec = ::std::option::Option::Some(map.next_value()?);
                                }
                                _ => {
                                    let _ = map.next_value::<::serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        let deserialized_next_id = next_id.ok_or_else(|| ::serde::de::Error::missing_field("next_id"))?;
                        let rows = rows_vec.ok_or_else(|| ::serde::de::Error::missing_field("rows"))?;

                        let mut table = #table_name::new();
                        for row in rows {
                            let pk = row.#pk_ident.clone();
                            if table.rows.contains_key(&pk) {
                                return ::std::result::Result::Err(::serde::de::Error::custom(
                                    ::std::format!("duplicate key in {}: {:?}", #table_name_str_owned, pk),
                                ));
                            }
                            table.rows.insert(pk, row);
                        }
                        table.rebuild_indexes();

                        // Defensively set next_id to max(deserialized, max_pk_successor).
                        let mut effective_next_id = deserialized_next_id;
                        if let ::std::option::Option::Some((&ref max_pk, _)) = table.rows.last_key_value() {
                            let max_successor = <#pk_ty as ::tabulosity::AutoIncrementable>::successor(max_pk);
                            if max_successor > effective_next_id {
                                effective_next_id = max_successor;
                            }
                        }
                        table.next_id = effective_next_id;

                        ::std::result::Result::Ok(table)
                    }
                }

                const FIELDS: &[&str] = &["next_id", "rows"];
                deserializer.deserialize_struct(#table_name_str_owned, FIELDS, #visitor_name)
            }
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Convert a PascalCase name to snake_case.
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_ascii_lowercase());
    }
    result
}
