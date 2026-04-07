//! Implementation of `#[derive(Table)]`.
//!
//! Generates a companion `{Name}Table` struct with:
//! - `rows: BTreeMap<PK, Row>` primary storage (or `InsOrdHashMap<PK, Row>`
//!   with `#[table(primary_storage = "hash")]`)
//! - BTree indexes from `#[indexed]`: `BTreeSet<(FieldType, PK)>`
//! - Hash indexes from `#[indexed(hash)]`: `InsOrdHashMap<FieldType, OneOrMany<PK, Inner>>`
//! - Unique indexes from `#[indexed(unique)]` or `#[indexed(hash, unique)]`
//! - Compound indexes from `#[index(...)]` with optional `kind = "hash"`
//! - Spatial indexes from `#[indexed(spatial)]` or `#[index(kind = "spatial")]`:
//!   R-tree-backed intersection queries via `SpatialIndex`. `Option<T>` spatial
//!   fields use a parallel none-set for rows where the field is `None`.
//! - Compound spatial indexes from per-field kind annotations (e.g.,
//!   `fields("zone_id", "pos" spatial)`): prefix fields partition rows into
//!   separate R-trees via `BTreeMap<PrefixKey, SpatialIndex>` (or `InsOrdHashMap`
//!   for hash prefixes). Query methods take prefix params + envelope.
//! - Filtered indexes via optional `filter` on `#[index(...)]`
//! - Tracked bounds per unique type across all indexes: `_bounds_{type}`
//! - Public read methods (get, get_ref, contains, len, is_empty, keys, etc.)
//! - `pk_val()` on the row type (returns owned key; used by `Database` cascade
//!   codegen). Single-column PKs also get `pk_ref()` for backward compat.
//! - `#[doc(hidden)] pub` mutation methods (_no_fk suffix)
//! - `modify_unchecked` — closure-based in-place mutation bypassing indexes,
//!   with debug-build assertions that PK and indexed fields are unchanged
//! - `modify_unchecked_range` / `modify_unchecked_all` — batch variants using
//!   `BTreeMap::range_mut`, applying `FnMut` per row with the same debug checks
//! - Per-index query methods using `IntoQuery` (by_*, iter_by_*, count_by_*)
//!   with `QueryOpts` for ordering (asc/desc) and offset (skip N)
//! - `modify_each_by_*` — query-driven batch mutation with debug-build safety
//! - `post_deser_rebuild_indexes()` for deserialization (BTree only)
//! - `manual_rebuild_all_indexes()` for full rebuild (BTree + hash)
//! - `Serialize` / `Deserialize` impls (behind `#[cfg(feature = "serde")]`)
//! - Non-PK auto-increment via `#[auto_increment]` on a non-primary-key field:
//!   generates a `next_<field>` counter, `insert_auto_no_fk` method, and serde
//!   support with fallback counter initialization from `max(field) + 1`.
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

use crate::parse::{self, IndexDecl, IndexKind, ParsedField, PrimaryStorageKind};

/// Abstraction over single-column and compound primary keys.
///
/// Single PK: `fields` has exactly 1 element, `is_auto_increment` may be true.
/// Compound PK: `fields` has 2+ elements, `is_auto_increment` is always false.
struct PkInfo {
    /// (field_ident, field_type) for each PK column, in declaration order.
    fields: Vec<(Ident, Type)>,
    is_auto_increment: bool,
}

impl PkInfo {
    fn is_compound(&self) -> bool {
        self.fields.len() > 1
    }

    /// The key type used in BTreeMap and method signatures.
    /// Single: `SomeType`. Compound: `(Type1, Type2)`.
    fn key_type(&self) -> TokenStream {
        if self.is_compound() {
            let tys: Vec<&Type> = self.fields.iter().map(|(_, ty)| ty).collect();
            quote! { (#(#tys),*) }
        } else {
            let ty = &self.fields[0].1;
            quote! { #ty }
        }
    }

    /// Expression to extract the key from a `row` variable.
    /// Single: `row.id.clone()`. Compound: `(row.a.clone(), row.b.clone())`.
    fn extract_key_from_row(&self) -> TokenStream {
        if self.is_compound() {
            let clones: Vec<TokenStream> = self
                .fields
                .iter()
                .map(|(id, _)| quote! { row.#id.clone() })
                .collect();
            quote! { (#(#clones),*) }
        } else {
            let id = &self.fields[0].0;
            quote! { row.#id.clone() }
        }
    }

    /// Expression to extract the key from a `pk` variable.
    /// Single: `pk.clone()`. Compound: `pk.clone()` (already a tuple).
    fn extract_key_from_var(&self) -> TokenStream {
        quote! { pk.clone() }
    }

    /// The flattened PK field types for appending to index tuples.
    fn index_pk_types(&self) -> Vec<&Type> {
        self.fields.iter().map(|(_, ty)| ty).collect()
    }

    /// Expressions to append PK fields from `row` into an index tuple.
    fn index_pk_clones_from_row(&self) -> Vec<TokenStream> {
        self.fields
            .iter()
            .map(|(id, _)| quote! { row.#id.clone() })
            .collect()
    }

    /// Expressions to append PK fields from a `pk` variable into an index tuple
    /// (used in update/remove where pk is already extracted).
    fn index_pk_clones_from_var(&self) -> Vec<TokenStream> {
        if self.is_compound() {
            (0..self.fields.len())
                .map(|i| {
                    let idx = syn::Index::from(i);
                    quote! { pk.#idx.clone() }
                })
                .collect()
        } else {
            vec![quote! { pk.clone() }]
        }
    }

    /// Expression to look up a row in `self.rows` from an index entry tuple,
    /// where PK fields start at position `start_idx`.
    fn row_lookup_from_entry(&self, start_idx: usize) -> TokenStream {
        if self.is_compound() {
            let indices: Vec<syn::Index> = (0..self.fields.len())
                .map(|i| syn::Index::from(start_idx + i))
                .collect();
            quote! { &self.rows[&(#(__entry.#indices),*)].row }
        } else {
            let idx = syn::Index::from(start_idx);
            quote! { &self.rows[&__entry.#idx].row }
        }
    }

    /// Generate `pk_val()` method on the row struct.
    /// Single: `self.field.clone()`. Compound: `(self.a.clone(), self.b.clone())`.
    fn gen_pk_val_method(&self) -> TokenStream {
        let key_ty = self.key_type();
        if self.is_compound() {
            let clones: Vec<TokenStream> = self
                .fields
                .iter()
                .map(|(id, _)| quote! { self.#id.clone() })
                .collect();
            quote! {
                /// Returns the compound primary key as an owned tuple.
                #[doc(hidden)]
                pub fn pk_val(&self) -> #key_ty {
                    (#(#clones),*)
                }
            }
        } else {
            let id = &self.fields[0].0;
            quote! {
                /// Returns the primary key value (cloned).
                #[doc(hidden)]
                pub fn pk_val(&self) -> #key_ty {
                    self.#id.clone()
                }
            }
        }
    }

    /// Generate debug snapshot statements for modify_unchecked variants.
    /// Snapshots all PK fields and returns (snap_stmts, assert_stmts).
    /// `method_name` appears in the panic message (e.g., "modify_unchecked",
    /// "modify_unchecked_range", or the `modify_each_by_*` method name).
    fn gen_modify_unchecked_pk_checks(
        &self,
        row_var: &Ident,
        method_name: &str,
    ) -> (Vec<TokenStream>, Vec<TokenStream>) {
        let mut snap_stmts = Vec::new();
        let mut assert_stmts = Vec::new();
        for (fi, _) in &self.fields {
            let snap_name = format_ident!("__snap_{}", fi);
            snap_stmts.push(quote! {
                #[cfg(debug_assertions)]
                let #snap_name = #row_var.#fi.clone();
            });
            let field_str = fi.to_string();
            let msg = format!(
                "{method_name}: primary key field `{field_str}` was changed (from {{:?}} to {{:?}}); use update() instead"
            );
            assert_stmts.push(quote! {
                assert!(
                    #row_var.#fi == #snap_name,
                    #msg,
                    #snap_name,
                    #row_var.#fi,
                );
            });
        }
        (snap_stmts, assert_stmts)
    }
}

/// Compound spatial index metadata — present only when an index has
/// non-spatial prefix fields followed by a spatial tail field.
struct CompoundSpatialInfo {
    /// Indices into `ResolvedIndex.fields` for the non-spatial prefix fields.
    prefix_field_indices: Vec<usize>,
    /// Index into `ResolvedIndex.fields` for the spatial field (always last).
    spatial_field_index: usize,
    /// Resolved kind for the prefix map (BTree or Hash).
    prefix_kind: IndexKind,
}

/// A resolved index — either from `#[indexed]` sugar or `#[index(...)]`.
struct ResolvedIndex {
    name: String,
    /// (field_ident, field_type) in order.
    fields: Vec<(Ident, Type)>,
    filter: Option<String>,
    is_unique: bool,
    /// The index backing storage kind: BTree, Hash, or Spatial.
    kind: IndexKind,
    /// Compound spatial support. `None` for non-compound-spatial indexes
    /// (including single-field spatial indexes).
    compound_spatial: Option<CompoundSpatialInfo>,
}

/// Generate a hash key expression from field clones.
/// Single field: `row.field.clone()` directly. Multiple: `(row.a.clone(), row.b.clone())`.
fn gen_hash_key_expr(field_clones: &[TokenStream]) -> TokenStream {
    if field_clones.len() == 1 {
        field_clones[0].clone()
    } else {
        quote! { (#(#field_clones),*) }
    }
}

/// Generate the OneOrMany insert method name based on primary storage.
fn gen_one_or_many_insert(primary_storage: PrimaryStorageKind) -> Ident {
    match primary_storage {
        PrimaryStorageKind::BTree => format_ident!("insert_btree"),
        PrimaryStorageKind::Hash => format_ident!("insert_hash"),
    }
}

/// Generate the OneOrMany remove method name based on primary storage.
fn gen_one_or_many_remove(primary_storage: PrimaryStorageKind) -> Ident {
    match primary_storage {
        PrimaryStorageKind::BTree => format_ident!("remove_btree"),
        PrimaryStorageKind::Hash => format_ident!("remove_hash"),
    }
}

/// Generate the OneOrMany iter method name based on primary storage.
fn gen_one_or_many_iter(primary_storage: PrimaryStorageKind) -> Ident {
    match primary_storage {
        PrimaryStorageKind::BTree => format_ident!("iter_btree"),
        PrimaryStorageKind::Hash => format_ident!("iter_hash"),
    }
}

/// Generate the prefix key type for a compound spatial index.
/// Single prefix field: bare type. Multiple: tuple.
fn gen_compound_prefix_key_ty(cs: &CompoundSpatialInfo, fields: &[(Ident, Type)]) -> TokenStream {
    let prefix_tys: Vec<&Type> = cs
        .prefix_field_indices
        .iter()
        .map(|&i| &fields[i].1)
        .collect();
    if prefix_tys.len() == 1 {
        let ty = prefix_tys[0];
        quote! { #ty }
    } else {
        quote! { (#(#prefix_tys),*) }
    }
}

/// Generate the prefix key expression from a row variable for a compound spatial index.
/// Single prefix field: `row.field.clone()`. Multiple: `(row.a.clone(), row.b.clone())`.
fn gen_compound_prefix_key_expr(
    cs: &CompoundSpatialInfo,
    fields: &[(Ident, Type)],
    row_var: &str,
) -> TokenStream {
    let row_ident = format_ident!("{}", row_var);
    let clones: Vec<TokenStream> = cs
        .prefix_field_indices
        .iter()
        .map(|&i| {
            let fi = &fields[i].0;
            quote! { #row_ident.#fi.clone() }
        })
        .collect();
    if clones.len() == 1 {
        clones[0].clone()
    } else {
        quote! { (#(#clones),*) }
    }
}

/// Generate the compound spatial insert body: extract prefix key, get-or-create
/// partition, then insert into R-tree or none-set.
///
/// Uses `entry().or_insert_with()` for BTreeMap prefixes, and a
/// `get_mut / insert` pattern for InsOrdHashMap prefixes (which lack `entry()`).
fn gen_compound_spatial_insert(
    cs: &CompoundSpatialInfo,
    idx: &ResolvedIndex,
    primary_storage: PrimaryStorageKind,
    row_var: &str,
) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let none_name = format_ident!("idx_{}_none", idx.name);
    let none_insert = gen_none_set_insert(primary_storage);
    let spatial_fi = &idx.fields[cs.spatial_field_index].0;
    let prefix_key_expr = gen_compound_prefix_key_expr(cs, &idx.fields, row_var);
    let row_ident = format_ident!("{}", row_var);

    match cs.prefix_kind {
        IndexKind::BTree => {
            let none_set_new = gen_none_set_new(primary_storage);
            quote! {
                let __prefix_key = #prefix_key_expr;
                match ::tabulosity::MaybeSpatialKey::as_spatial(&#row_ident.#spatial_fi) {
                    ::std::option::Option::Some(__key) => {
                        self.#idx_name
                            .entry(__prefix_key)
                            .or_insert_with(|| ::tabulosity::SpatialIndex::new())
                            .insert(__key, pk.clone());
                    }
                    ::std::option::Option::None => {
                        self.#none_name
                            .entry(__prefix_key)
                            .or_insert_with(|| #none_set_new)
                            .#none_insert;
                    }
                }
            }
        }
        IndexKind::Hash => {
            // InsOrdHashMap has no entry() API — use get_mut/insert pattern.
            let none_set_new = gen_none_set_new(primary_storage);
            quote! {
                let __prefix_key = #prefix_key_expr;
                match ::tabulosity::MaybeSpatialKey::as_spatial(&#row_ident.#spatial_fi) {
                    ::std::option::Option::Some(__key) => {
                        match self.#idx_name.get_mut(&__prefix_key) {
                            ::std::option::Option::Some(__p) => {
                                __p.insert(__key, pk.clone());
                            }
                            ::std::option::Option::None => {
                                let mut __p = ::tabulosity::SpatialIndex::new();
                                __p.insert(__key, pk.clone());
                                self.#idx_name.insert(__prefix_key, __p);
                            }
                        }
                    }
                    ::std::option::Option::None => {
                        match self.#none_name.get_mut(&__prefix_key) {
                            ::std::option::Option::Some(__ns) => {
                                __ns.#none_insert;
                            }
                            ::std::option::Option::None => {
                                let mut __ns = #none_set_new;
                                __ns.#none_insert;
                                self.#none_name.insert(__prefix_key, __ns);
                            }
                        }
                    }
                }
            }
        }
        IndexKind::Spatial => unreachable!("prefix kind cannot be Spatial"),
    }
}

/// Generate a constructor expression for a spatial index's none-set.
fn gen_none_set_new(primary_storage: PrimaryStorageKind) -> TokenStream {
    match primary_storage {
        PrimaryStorageKind::BTree => quote! { ::std::collections::BTreeSet::new() },
        PrimaryStorageKind::Hash => quote! { ::tabulosity::InsOrdHashMap::new() },
    }
}

/// Generate the insert expression for a spatial index's none-set.
/// The `pk_expr` variable must be in scope as `pk` (from extract_key_from_row).
fn gen_none_set_insert(primary_storage: PrimaryStorageKind) -> TokenStream {
    match primary_storage {
        PrimaryStorageKind::BTree => quote! { insert(pk.clone()) },
        PrimaryStorageKind::Hash => quote! { insert(pk.clone(), ()) },
    }
}

/// Generate the remove expression for a spatial index's none-set.
fn gen_none_set_remove(primary_storage: PrimaryStorageKind) -> TokenStream {
    match primary_storage {
        PrimaryStorageKind::BTree => quote! { remove(&pk) },
        PrimaryStorageKind::Hash => quote! { remove(&pk) },
    }
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

    // Parse compound PK attribute (struct-level).
    let compound_pk_fields = match parse::parse_compound_pk_attr(input) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };

    // Find field-level primary key(s).
    let field_level_pks: Vec<&ParsedField> = fields.iter().filter(|f| f.is_primary_key).collect();

    // Find non-PK auto-increment fields.
    let nonpk_auto_fields: Vec<&ParsedField> = fields
        .iter()
        .filter(|f| f.is_auto_increment && !f.is_primary_key)
        .collect();
    if nonpk_auto_fields.len() > 1 {
        return syn::Error::new_spanned(row_name, "at most one #[auto_increment] field per table")
            .to_compile_error();
    }
    let nonpk_auto_field: Option<(Ident, Type)> = nonpk_auto_fields
        .first()
        .map(|f| (f.ident.clone(), f.ty.clone()));

    // Build PkInfo — either from struct-level compound PK or field-level single PK.
    let pk_info = if let Some(ref compound_names) = compound_pk_fields {
        // Struct-level compound PK.
        if !field_level_pks.is_empty() {
            return syn::Error::new_spanned(
                row_name,
                "cannot combine struct-level #[primary_key(...)] with field-level #[primary_key]; use one or the other",
            )
            .to_compile_error();
        }
        let mut pk_fields_resolved = Vec::new();
        for name in compound_names {
            match fields.iter().find(|f| f.ident == name.as_str()) {
                Some(f) => {
                    // `#[primary_key(auto_increment)]` on a compound PK field is still
                    // forbidden — but a standalone `#[auto_increment]` (non-PK) is fine
                    // and was already parsed separately above.
                    if f.is_primary_key && f.is_auto_increment {
                        return syn::Error::new_spanned(
                            row_name,
                            "compound primary keys cannot use #[primary_key(auto_increment)]",
                        )
                        .to_compile_error();
                    }
                    pk_fields_resolved.push((f.ident.clone(), f.ty.clone()));
                }
                None => {
                    return syn::Error::new_spanned(
                        row_name,
                        format!(
                            "compound primary key field `{}` does not exist on the struct",
                            name
                        ),
                    )
                    .to_compile_error();
                }
            }
        }
        PkInfo {
            fields: pk_fields_resolved,
            is_auto_increment: false,
        }
    } else {
        // Field-level single PK.
        if field_level_pks.len() != 1 {
            return syn::Error::new_spanned(
                row_name,
                "Table requires exactly one field with #[primary_key] or a struct-level #[primary_key(\"field1\", \"field2\")]",
            )
            .to_compile_error();
        }
        let pk_field = field_level_pks[0];
        PkInfo {
            fields: vec![(pk_field.ident.clone(), pk_field.ty.clone())],
            is_auto_increment: pk_field.is_auto_increment,
        }
    };

    // Validate: cannot have both PK auto-increment and non-PK auto-increment.
    if pk_info.is_auto_increment && nonpk_auto_field.is_some() {
        return syn::Error::new_spanned(
            row_name,
            "cannot have both #[primary_key(auto_increment)] and a separate #[auto_increment] field on the same table",
        )
        .to_compile_error();
    }

    let key_ty = pk_info.key_type();
    let extract_key = pk_info.extract_key_from_row();
    let is_auto_increment = pk_info.is_auto_increment;

    // Parse struct-level #[table(...)] attribute for primary storage kind and checksummed flag.
    let table_attr = match parse::parse_table_attr(input) {
        Ok(ta) => ta,
        Err(e) => return e.to_compile_error(),
    };
    let primary_storage = table_attr.primary_storage;
    let checksummed = table_attr.checksummed;
    let row_entry_name = format_ident!("{}RowEntry", row_name);

    // Parse struct-level #[index(...)] attributes.
    let index_decls = match parse::parse_index_attrs(input) {
        Ok(d) => d,
        Err(e) => return e.to_compile_error(),
    };

    // Resolve all indexes.
    let resolved_indexes = match resolve_indexes(&fields, &pk_info, &index_decls, input) {
        Ok(r) => r,
        Err(e) => return e.to_compile_error(),
    };

    // Collect unique tracked types (deduped by suffix).
    let unique_tracked = collect_unique_tracked_types(&resolved_indexes, &pk_info);

    // --- Companion struct fields ---
    let idx_field_decls = gen_idx_field_decls(&resolved_indexes, &pk_info, primary_storage);
    let bounds_field_decls = gen_bounds_field_decls(&unique_tracked);
    let idx_field_inits = gen_idx_field_inits(&resolved_indexes, primary_storage);
    let bounds_field_inits = gen_bounds_field_inits(&unique_tracked);

    // --- Bounds widening (insert/upsert-insert) ---
    let bounds_widen_row = gen_bounds_widen(&resolved_indexes, &pk_info, &fields);

    // --- Index maintenance ---
    let idx_insert = gen_all_idx_insert(&resolved_indexes, &pk_info, primary_storage);
    let idx_update = gen_all_idx_update(&resolved_indexes, &pk_info, primary_storage);
    let idx_upsert_update = gen_all_idx_update(&resolved_indexes, &pk_info, primary_storage);
    let idx_upsert_insert = gen_all_idx_insert(&resolved_indexes, &pk_info, primary_storage);
    let idx_remove = gen_all_idx_remove(&resolved_indexes, &pk_info, primary_storage);

    // --- Rebuild indexes ---
    let bounds_reset = gen_bounds_reset(&unique_tracked);
    let rebuild_body = gen_rebuild_body(&resolved_indexes, &pk_info, primary_storage, false);
    let rebuild_all_body = gen_rebuild_body(&resolved_indexes, &pk_info, primary_storage, true);
    let rebuild_bounds_widen = gen_bounds_widen(&resolved_indexes, &pk_info, &fields);

    // --- Query methods ---
    let query_methods = gen_all_query_methods(
        &resolved_indexes,
        &pk_info,
        row_name,
        &fields,
        primary_storage,
    );

    let table_name_str_ref = &table_name_str;

    // --- modify_each_by_* methods ---
    let modify_each_methods =
        gen_all_modify_each_methods(&resolved_indexes, &pk_info, row_name, checksummed);

    // --- modify_unchecked (single + range + all) ---
    let modify_unchecked_method = gen_modify_unchecked(
        &pk_info,
        row_name,
        table_name_str_ref,
        &resolved_indexes,
        checksummed,
    );
    let modify_unchecked_range_methods = gen_modify_unchecked_range(
        &pk_info,
        row_name,
        &resolved_indexes,
        primary_storage,
        checksummed,
    );

    // --- Unique index checks ---
    let unique_check_insert =
        gen_unique_checks_insert(&resolved_indexes, &pk_info, table_name_str_ref);
    let unique_check_update =
        gen_unique_checks_update(&resolved_indexes, &pk_info, table_name_str_ref);

    // Auto-increment: optional next_id field, init, bump logic, and methods.
    // Only available for single-column PKs.
    let next_id_field_decl = if is_auto_increment {
        quote! { next_id: #key_ty, }
    } else {
        quote! {}
    };
    let next_id_field_init = if is_auto_increment {
        quote! { next_id: <#key_ty as ::tabulosity::AutoIncrementable>::first(), }
    } else {
        quote! {}
    };
    let next_id_bump_on_insert = if is_auto_increment {
        quote! {
            if pk >= self.next_id {
                self.next_id = <#key_ty as ::tabulosity::AutoIncrementable>::successor(&pk);
            }
        }
    } else {
        quote! {}
    };
    let next_id_bump_on_upsert = if is_auto_increment {
        quote! {
            if pk >= self.next_id {
                self.next_id = <#key_ty as ::tabulosity::AutoIncrementable>::successor(&pk);
            }
        }
    } else {
        quote! {}
    };
    let pk_ident_single = if !pk_info.is_compound() {
        Some(&pk_info.fields[0].0)
    } else {
        None
    };
    let auto_increment_methods = if is_auto_increment {
        let pk_ident = pk_ident_single.unwrap();
        quote! {
            /// Returns the next auto-increment ID that will be assigned.
            pub fn next_id(&self) -> #key_ty {
                self.next_id.clone()
            }

            /// Inserts a row with an auto-assigned primary key.
            /// The closure receives the assigned PK and must return a row with that PK.
            #[doc(hidden)]
            pub fn insert_auto_no_fk(
                &mut self,
                f: impl ::std::ops::FnOnce(#key_ty) -> #row_name,
            ) -> ::std::result::Result<#key_ty, ::tabulosity::Error> {
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

    // --- Non-PK auto-increment: counter field, init, bump logic, and methods ---
    let nonpk_auto_field_decl = if let Some((ref fi, ref ty)) = nonpk_auto_field {
        let counter_name = format_ident!("next_{}", fi);
        quote! { #counter_name: #ty, }
    } else {
        quote! {}
    };
    let nonpk_auto_field_init = if let Some((ref fi, ref ty)) = nonpk_auto_field {
        let counter_name = format_ident!("next_{}", fi);
        quote! { #counter_name: <#ty as ::tabulosity::AutoIncrementable>::first(), }
    } else {
        quote! {}
    };
    let nonpk_auto_bump_on_insert = if let Some((ref fi, ref ty)) = nonpk_auto_field {
        let counter_name = format_ident!("next_{}", fi);
        quote! {
            if row.#fi >= self.#counter_name {
                self.#counter_name = <#ty as ::tabulosity::AutoIncrementable>::successor(&row.#fi);
            }
        }
    } else {
        quote! {}
    };
    let nonpk_auto_bump_on_upsert = nonpk_auto_bump_on_insert.clone();
    let nonpk_auto_methods = if let Some((ref fi, ref ty)) = nonpk_auto_field {
        let counter_name = format_ident!("next_{}", fi);
        let next_fn = format_ident!("next_{}", fi);
        quote! {
            /// Returns the next auto-increment value that will be assigned
            /// to the `#fi` field.
            pub fn #next_fn(&self) -> #ty {
                self.#counter_name.clone()
            }

            /// Inserts a row with an auto-assigned value for the auto-increment field.
            /// The closure receives the assigned value and must return a row with that value.
            #[doc(hidden)]
            pub fn insert_auto_no_fk(
                &mut self,
                f: impl ::std::ops::FnOnce(#ty) -> #row_name,
            ) -> ::std::result::Result<#key_ty, ::tabulosity::Error> {
                let __auto_val = self.#counter_name.clone();
                let row = f(__auto_val.clone());
                debug_assert_eq!(row.#fi, __auto_val);
                let pk = #extract_key;
                self.insert_no_fk(row)?;
                ::std::result::Result::Ok(pk)
            }
        }
    } else {
        quote! {}
    };

    let hash_indexes: Vec<&ResolvedIndex> = resolved_indexes
        .iter()
        .filter(|idx| idx.kind == IndexKind::Hash)
        .collect();
    let serde_impls = gen_serde_impls(
        &table_name,
        row_name,
        &pk_info,
        table_name_str_ref,
        is_auto_increment,
        &nonpk_auto_field,
        &hash_indexes,
        primary_storage,
        &row_entry_name,
    );

    // pk_ref method (single PK only, for backward compat).
    let pk_ref_method = if !pk_info.is_compound() {
        let pk_ident = &pk_info.fields[0].0;
        quote! {
            /// Returns a reference to the primary key field.
            #[doc(hidden)]
            pub fn pk_ref(&self) -> &#key_ty {
                &self.#pk_ident
            }
        }
    } else {
        quote! {}
    };
    let pk_val_method = pk_info.gen_pk_val_method();

    // Remove re-extraction: for remove_no_fk, we need to re-extract the key
    // from the row for index removal.
    let re_extract_key_for_remove = {
        if pk_info.is_compound() {
            let clones: Vec<TokenStream> = pk_info
                .fields
                .iter()
                .map(|(id, _)| quote! { row.#id.clone() })
                .collect();
            quote! { let pk = (#(#clones),*); }
        } else {
            let id = &pk_info.fields[0].0;
            quote! { let pk = row.#id.clone(); }
        }
    };

    // --- RowEntry wrapper ---
    let row_entry_struct = if checksummed {
        quote! {
            /// Per-row wrapper generated by `#[derive(Table)]`.
            /// Checksummed: holds the row and its transient CRC.
            #[doc(hidden)]
            #vis struct #row_entry_name {
                pub row: #row_name,
                pub crc: ::std::option::Option<u32>,
            }
            impl #row_entry_name {
                #[inline]
                fn new(row: #row_name) -> Self {
                    Self { row, crc: ::std::option::Option::None }
                }
            }
            impl ::std::clone::Clone for #row_entry_name {
                fn clone(&self) -> Self {
                    Self { row: self.row.clone(), crc: self.crc }
                }
            }
        }
    } else {
        quote! {
            /// Per-row wrapper generated by `#[derive(Table)]`.
            /// Non-checksummed: zero-overhead single-field wrapper.
            #[doc(hidden)]
            #vis struct #row_entry_name {
                pub row: #row_name,
            }
            impl #row_entry_name {
                #[inline]
                fn new(row: #row_name) -> Self {
                    Self { row }
                }
            }
            impl ::std::clone::Clone for #row_entry_name {
                fn clone(&self) -> Self {
                    Self { row: self.row.clone() }
                }
            }
        }
    };

    // CRC bookkeeping fields (checksummed tables only).
    //
    // `_crc_dirty` does not grow unboundedly: mutation paths only push when
    // `entry.crc` transitions from `Some` to `None` (subsequent mutations
    // to an already-dirty row are no-ops). A PK normally appears at most
    // once, but insert-remove-reinsert of the same PK between checksum
    // calls can produce duplicates — the drain loop handles this via a
    // skip guard. The vec is drained completely on every `checksum()` call.
    // Its high-water mark equals the number of row mutations (not distinct
    // rows) between consecutive `checksum()` calls, bounded by table size
    // plus insert-remove-reinsert sequences.
    let crc_field_decls = if checksummed {
        quote! {
            /// Running XOR of all row CRCs. `None` until the first `checksum()` call.
            _crc_xor: ::std::option::Option<u32>,
            /// PKs of rows needing CRC recomputation. Mutation paths only push
            /// when `entry.crc` transitions from `Some` to `None`; subsequent
            /// mutations to an already-dirty row are no-ops. A PK normally
            /// appears at most once, but insert-remove-reinsert can produce
            /// duplicates (handled by the drain loop's skip guard). Drained
            /// completely on every `checksum()` call.
            _crc_dirty: ::std::vec::Vec<#key_ty>,
        }
    } else {
        quote! {}
    };
    let crc_field_inits = if checksummed {
        quote! {
            _crc_xor: ::std::option::Option::None,
            _crc_dirty: ::std::vec::Vec::new(),
        }
    } else {
        quote! {}
    };

    let rows_field_ty = match primary_storage {
        PrimaryStorageKind::BTree => {
            quote! { ::std::collections::BTreeMap<#key_ty, #row_entry_name> }
        }
        PrimaryStorageKind::Hash => {
            quote! { ::tabulosity::InsOrdHashMap<#key_ty, #row_entry_name> }
        }
    };
    let rows_field_init = match primary_storage {
        PrimaryStorageKind::BTree => quote! { ::std::collections::BTreeMap::new() },
        PrimaryStorageKind::Hash => quote! { ::tabulosity::InsOrdHashMap::new() },
    };

    // --- CRC bookkeeping token fragments ---
    // Insert: if CRC system is active, push pk to dirty.
    let crc_on_insert = if checksummed {
        quote! {
            if self._crc_xor.is_some() {
                self._crc_dirty.push(pk.clone());
            }
        }
    } else {
        quote! {}
    };
    // Update/upsert (existing row): XOR out old CRC if present, mark dirty.
    let crc_on_update_pre = if checksummed {
        quote! {
            if let ::std::option::Option::Some(__old_crc) = self.rows.get(&pk).and_then(|__e| __e.crc) {
                if let ::std::option::Option::Some(ref mut __xor) = self._crc_xor {
                    *__xor ^= __old_crc;
                }
                self._crc_dirty.push(pk.clone());
            } else if self._crc_xor.is_some() && self.rows.get(&pk).map_or(false, |__e| __e.crc.is_none()) {
                // Already dirty — no-op (row is already in dirty vec or will be
                // handled on next checksum). But we need to ensure it's in dirty
                // vec if CRC system is active. For update, the entry always existed,
                // so if crc is None and system is active, it's already dirty.
            }
        }
    } else {
        quote! {}
    };
    // Remove: XOR out old CRC if present.
    let crc_on_remove = if checksummed {
        quote! {
            if let ::std::option::Option::Some(__old_crc) = __entry.crc {
                if let ::std::option::Option::Some(ref mut __xor) = self._crc_xor {
                    *__xor ^= __old_crc;
                }
            }
            // If crc was None, the PK is in the dirty vec and will be skipped on drain.
        }
    } else {
        quote! {}
    };
    // checksum() method (checksummed only).
    let checksum_method = if checksummed {
        quote! {
            /// Computes and returns the table-level CRC32 checksum.
            ///
            /// On first call, computes CRC for every row (O(n)). Subsequent
            /// calls are O(dirty_rows) — only recomputes rows that changed
            /// since the last checksum.
            pub fn checksum(&mut self) -> ::std::option::Option<u32> {
                if self._crc_xor.is_none() {
                    // First checksum ever: compute CRC for every row.
                    let mut xor = 0u32;
                    for (_pk, entry) in self.rows.iter_mut() {
                        let crc = ::tabulosity::crc32_of(&entry.row);
                        entry.crc = ::std::option::Option::Some(crc);
                        xor ^= crc;
                    }
                    self._crc_xor = ::std::option::Option::Some(xor);
                    self._crc_dirty.clear();
                } else {
                    // Drain dirty vec: recompute CRC for rows that changed
                    // since the last checksum. Each PK normally appears at most
                    // once (mutation paths only push on the Some->None transition),
                    // but insert-remove-reinsert of the same PK can produce
                    // duplicates, so we skip entries that were already recomputed.
                    let dirty = ::std::mem::take(&mut self._crc_dirty);
                    for pk in dirty {
                        if let ::std::option::Option::Some(entry) = self.rows.get_mut(&pk) {
                            // Skip if already recomputed (duplicate PK in dirty vec
                            // from insert-remove-reinsert sequence).
                            if entry.crc.is_some() {
                                continue;
                            }
                            let crc = ::tabulosity::crc32_of(&entry.row);
                            entry.crc = ::std::option::Option::Some(crc);
                            if let ::std::option::Option::Some(ref mut __xor) = self._crc_xor {
                                *__xor ^= crc;
                            }
                        }
                        // If row was removed, its old CRC was already XORed out
                        // during removal. Skip.
                    }
                }
                self._crc_xor
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #row_entry_struct

        /// Companion table struct generated by `#[derive(Table)]`.
        #vis struct #table_name {
            rows: #rows_field_ty,
            #(#idx_field_decls,)*
            #(#bounds_field_decls,)*
            #next_id_field_decl
            #nonpk_auto_field_decl
            #crc_field_decls
        }

        impl #table_name {
            /// Creates a new empty table.
            pub fn new() -> Self {
                Self {
                    rows: #rows_field_init,
                    #(#idx_field_inits,)*
                    #(#bounds_field_inits,)*
                    #next_id_field_init
                    #nonpk_auto_field_init
                    #crc_field_inits
                }
            }

            // --- Read methods ---

            /// Returns a clone of the row with the given primary key, or `None`.
            pub fn get(&self, pk: &#key_ty) -> ::std::option::Option<#row_name> {
                self.rows.get(pk).map(|__e| __e.row.clone())
            }

            /// Returns a reference to the row with the given primary key, or `None`.
            pub fn get_ref(&self, pk: &#key_ty) -> ::std::option::Option<&#row_name> {
                self.rows.get(pk).map(|__e| &__e.row)
            }

            /// Returns `true` if the table contains a row with the given primary key.
            pub fn contains(&self, pk: &#key_ty) -> bool {
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
            pub fn keys(&self) -> ::std::vec::Vec<#key_ty> {
                self.rows.keys().cloned().collect()
            }

            /// Iterates over all primary keys in order.
            pub fn iter_keys(&self) -> impl ::std::iter::Iterator<Item = &#key_ty> {
                self.rows.keys()
            }

            /// Returns all rows in primary key order (cloned).
            pub fn all(&self) -> ::std::vec::Vec<#row_name> {
                self.rows.values().map(|__e| __e.row.clone()).collect()
            }

            /// Iterates over references to all rows in primary key order.
            pub fn iter_all(&self) -> impl ::std::iter::Iterator<Item = &#row_name> {
                self.rows.values().map(|__e| &__e.row)
            }

            // --- Index query methods ---

            #(#query_methods)*

            #(#modify_each_methods)*

            // --- Mutation methods (doc-hidden, used by Database derive) ---

            /// Inserts a row. Returns `Err(DuplicateKey)` if the PK already exists,
            /// or `Err(DuplicateIndex)` if a unique index constraint is violated.
            #[doc(hidden)]
            pub fn insert_no_fk(&mut self, row: #row_name) -> ::std::result::Result<(), ::tabulosity::Error> {
                let pk = #extract_key;
                if self.rows.contains_key(&pk) {
                    return ::std::result::Result::Err(::tabulosity::Error::DuplicateKey {
                        table: #table_name_str_ref,
                        key: ::std::format!("{:?}", pk),
                    });
                }
                #(#bounds_widen_row)*
                #(#unique_check_insert)*
                #next_id_bump_on_insert
                #nonpk_auto_bump_on_insert
                #(#idx_insert)*
                #crc_on_insert
                self.rows.insert(pk, #row_entry_name::new(row));
                ::std::result::Result::Ok(())
            }

            #auto_increment_methods

            #nonpk_auto_methods

            /// Updates a row. Returns `Err(NotFound)` if the PK is missing,
            /// or `Err(DuplicateIndex)` if a unique index constraint is violated.
            #[doc(hidden)]
            pub fn update_no_fk(&mut self, row: #row_name) -> ::std::result::Result<(), ::tabulosity::Error> {
                let pk = #extract_key;
                let old_row = match self.rows.get(&pk) {
                    Some(__e) => __e.row.clone(),
                    None => {
                        return ::std::result::Result::Err(::tabulosity::Error::NotFound {
                            table: #table_name_str_ref,
                            key: ::std::format!("{:?}", pk),
                        });
                    }
                };
                #(#unique_check_update)*
                // CRC bookkeeping runs after all fallible checks so that
                // a failed unique constraint doesn't corrupt the XOR state.
                #crc_on_update_pre
                #(#bounds_widen_row)*
                #(#idx_update)*
                self.rows.insert(pk, #row_entry_name::new(row));
                ::std::result::Result::Ok(())
            }

            /// Upserts a row — inserts if missing, updates if existing.
            /// Returns `Err(DuplicateIndex)` if a unique index constraint
            /// is violated.
            #[doc(hidden)]
            pub fn upsert_no_fk(&mut self, row: #row_name) -> ::std::result::Result<(), ::tabulosity::Error> {
                let pk = #extract_key;
                if let Some(old_row) = self.rows.get(&pk).map(|__e| __e.row.clone()) {
                    #(#unique_check_update)*
                    // CRC bookkeeping runs after all fallible checks so that
                    // a failed unique constraint doesn't corrupt the XOR state.
                    #crc_on_update_pre
                    #next_id_bump_on_upsert
                    #nonpk_auto_bump_on_upsert
                    #(#bounds_widen_row)*
                    #(#idx_upsert_update)*
                    self.rows.insert(pk, #row_entry_name::new(row));
                } else {
                    #(#bounds_widen_row)*
                    #(#unique_check_insert)*
                    #next_id_bump_on_upsert
                    #nonpk_auto_bump_on_upsert
                    #(#idx_upsert_insert)*
                    #crc_on_insert
                    self.rows.insert(pk, #row_entry_name::new(row));
                }
                ::std::result::Result::Ok(())
            }

            /// Removes a row. Returns the removed row or `Err(NotFound)`.
            #[doc(hidden)]
            pub fn remove_no_fk(&mut self, pk: &#key_ty) -> ::std::result::Result<#row_name, ::tabulosity::Error> {
                let __entry = match self.rows.remove(pk) {
                    Some(r) => r,
                    None => {
                        return ::std::result::Result::Err(::tabulosity::Error::NotFound {
                            table: #table_name_str_ref,
                            key: ::std::format!("{:?}", pk),
                        });
                    }
                };
                #crc_on_remove
                let row = __entry.row;
                #re_extract_key_for_remove
                #(#idx_remove)*
                ::std::result::Result::Ok(row)
            }

            #modify_unchecked_method

            #modify_unchecked_range_methods

            #checksum_method

            /// Rebuilds BTree and spatial secondary indexes and tracked bounds
            /// from row data. Hash indexes are NOT rebuilt — they are
            /// deserialized directly to preserve insertion order.
            #[doc(hidden)]
            pub fn post_deser_rebuild_indexes(&mut self) {
                #(#bounds_reset)*
                #(#rebuild_body)*
                for (_pk, __entry) in &self.rows {
                    let row = &__entry.row;
                    #(#rebuild_bounds_widen)*
                }
            }

            /// Rebuilds ALL secondary indexes (BTree and hash) and tracked
            /// bounds from row data. Hash index insertion order after this
            /// call reflects `rows` iteration order.
            pub fn manual_rebuild_all_indexes(&mut self) {
                #(#bounds_reset)*
                #(#rebuild_all_body)*
                for (_pk, __entry) in &self.rows {
                    let row = &__entry.row;
                    #(#rebuild_bounds_widen)*
                }
            }
        }


        impl #row_name {
            #pk_ref_method
            #pk_val_method
        }

        impl ::tabulosity::TableMeta for #table_name {
            type Key = #key_ty;
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
    pk_info: &PkInfo,
    index_decls: &[IndexDecl],
    input: &DeriveInput,
) -> syn::Result<Vec<ResolvedIndex>> {
    let mut resolved = Vec::new();
    let mut used_names = std::collections::BTreeSet::new();

    let pk_field_names: std::collections::BTreeSet<String> = pk_info
        .fields
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();

    // Simple indexes from #[indexed].
    for f in fields {
        if f.is_indexed {
            // Spatial index on a PK field is forbidden.
            if f.index_kind == IndexKind::Spatial && pk_field_names.contains(&f.ident.to_string()) {
                return Err(syn::Error::new_spanned(
                    &f.ident,
                    "spatial index cannot be placed on a primary key field",
                ));
            }
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
                kind: f.index_kind,
                compound_spatial: None,
            });
        }
    }

    // Compound/filtered indexes from #[index(...)].
    for decl in index_decls {
        validate_index_decl(input, decl, fields, pk_info, &used_names)?;
        used_names.insert(decl.name.clone());

        let idx_fields: Vec<(Ident, Type)> = decl
            .fields
            .iter()
            .map(|fd| {
                let f = fields.iter().find(|f| f.ident == fd.name.as_str()).unwrap();
                (f.ident.clone(), f.ty.clone())
            })
            .collect();

        // Resolve per-field kinds and detect compound spatial indexes.
        let resolved_kinds: Vec<IndexKind> = decl
            .fields
            .iter()
            .map(|fd| fd.kind_override.unwrap_or(decl.kind))
            .collect();

        // Find spatial fields.
        let spatial_indices: Vec<usize> = resolved_kinds
            .iter()
            .enumerate()
            .filter(|(_, k)| **k == IndexKind::Spatial)
            .map(|(i, _)| i)
            .collect();

        if spatial_indices.len() > 1 {
            // Check if all fields inherited spatial from the index-level kind.
            let all_inherited = decl.kind == IndexKind::Spatial
                && decl.fields.iter().all(|fd| fd.kind_override.is_none());
            if all_inherited {
                return Err(syn::Error::new_spanned(
                    input,
                    format!(
                        "index `{}`: index-level kind = \"spatial\" cannot be used with multiple fields; \
                         use per-field spatial annotation on the spatial field instead \
                         (e.g., fields(\"a\", \"b\" spatial))",
                        decl.name
                    ),
                ));
            }
            return Err(syn::Error::new_spanned(
                input,
                format!("index `{}`: at most one field may be `spatial`", decl.name),
            ));
        }

        let (resolved_kind, compound_spatial) = if let Some(&spatial_idx) = spatial_indices.first()
        {
            // Validate spatial field is last.
            if spatial_idx != decl.fields.len() - 1 {
                return Err(syn::Error::new_spanned(
                    input,
                    format!(
                        "index `{}`: the spatial field must be the last field in fields(...)",
                        decl.name
                    ),
                ));
            }

            if decl.fields.len() == 1 {
                // Single-field spatial — no compound info needed.
                (IndexKind::Spatial, None)
            } else {
                // Compound spatial: validate all prefix fields have the same kind.
                let prefix_kinds: Vec<IndexKind> = resolved_kinds[..spatial_idx].to_vec();
                let first_prefix_kind = prefix_kinds[0];
                if first_prefix_kind == IndexKind::Spatial {
                    return Err(syn::Error::new_spanned(
                        input,
                        format!(
                            "index `{}`: prefix fields cannot be spatial; only the last field can be spatial",
                            decl.name
                        ),
                    ));
                }
                for (i, pk) in prefix_kinds.iter().enumerate().skip(1) {
                    if *pk != first_prefix_kind {
                        return Err(syn::Error::new_spanned(
                            input,
                            format!(
                                "index `{}`: all prefix fields must have the same kind, but field `{}` \
                                 is {:?} while field `{}` is {:?}; set the index-level `kind` or annotate \
                                 each prefix field explicitly",
                                decl.name,
                                decl.fields[0].name,
                                first_prefix_kind,
                                decl.fields[i].name,
                                pk,
                            ),
                        ));
                    }
                }

                let prefix_field_indices = (0..spatial_idx).collect();
                (
                    IndexKind::Spatial,
                    Some(CompoundSpatialInfo {
                        prefix_field_indices,
                        spatial_field_index: spatial_idx,
                        prefix_kind: first_prefix_kind,
                    }),
                )
            }
        } else {
            // No spatial field — use the index-level kind as-is.
            (decl.kind, None)
        };

        // Spatial indexes (including compound spatial detected via per-field
        // annotation) cannot be unique. The parse-time check only catches
        // `kind = "spatial"` + `unique`; per-field annotation resolves to
        // Spatial at resolution time, so we need a post-resolution check.
        if resolved_kind == IndexKind::Spatial && decl.unique {
            return Err(syn::Error::new_spanned(
                input,
                format!(
                    "index `{}`: spatial indexes cannot be unique; spatial queries use intersection, not equality",
                    decl.name
                ),
            ));
        }

        resolved.push(ResolvedIndex {
            name: decl.name.clone(),
            fields: idx_fields,
            filter: decl.filter.clone(),
            is_unique: decl.unique,
            kind: resolved_kind,
            compound_spatial,
        });
    }

    Ok(resolved)
}

fn validate_index_decl(
    input: &DeriveInput,
    decl: &IndexDecl,
    fields: &[ParsedField],
    pk_info: &PkInfo,
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
    let pk_field_names: std::collections::BTreeSet<String> = pk_info
        .fields
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();
    for fd in &decl.fields {
        let field = fields.iter().find(|f| f.ident == fd.name.as_str());
        match field {
            None => {
                return Err(syn::Error::new_spanned(
                    input,
                    format!(
                        "index `{}`: field `{}` does not exist on the struct",
                        decl.name, fd.name
                    ),
                ));
            }
            Some(f) if pk_field_names.contains(&f.ident.to_string()) => {
                return Err(syn::Error::new_spanned(
                    input,
                    format!(
                        "index `{}`: field '{}' is part of the primary key and is automatically included in every index; remove it from fields(...)",
                        decl.name, fd.name
                    ),
                ));
            }
            _ => {}
        }
    }

    // Note: spatial-on-PK is already caught by the general PK check above
    // (line "is part of the primary key"). Field-level #[indexed(spatial)]
    // on a PK field is caught separately in resolve_indexes().

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

fn collect_unique_tracked_types(indexes: &[ResolvedIndex], pk_info: &PkInfo) -> Vec<TrackedType> {
    let mut result = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for idx in indexes {
        // Hash indexes don't need bounds tracking.
        if idx.kind == IndexKind::Hash {
            continue;
        }
        // Spatial indexes: single-field spatial doesn't need bounds tracking.
        // Compound spatial with btree prefix does need it for prefix fields.
        if idx.kind == IndexKind::Spatial {
            if let Some(cs) = &idx.compound_spatial
                && cs.prefix_kind == IndexKind::BTree
            {
                for &i in &cs.prefix_field_indices {
                    let ty = &idx.fields[i].1;
                    let suffix = type_suffix(ty);
                    if seen.insert(suffix.clone()) {
                        result.push(TrackedType {
                            bounds_suffix: suffix,
                            ty: ty.clone(),
                        });
                    }
                }
            }
            continue;
        }
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

    // Track bounds for each PK field type (compound PKs have multiple).
    for (_, pk_ty) in &pk_info.fields {
        let pk_suffix = type_suffix(pk_ty);
        if seen.insert(pk_suffix.clone()) {
            result.push(TrackedType {
                bounds_suffix: pk_suffix,
                ty: pk_ty.clone(),
            });
        }
    }

    result
}

// =============================================================================
// Struct field codegen
// =============================================================================

fn gen_idx_field_decls(
    indexes: &[ResolvedIndex],
    pk_info: &PkInfo,
    primary_storage: PrimaryStorageKind,
) -> Vec<TokenStream> {
    let pk_tys = pk_info.index_pk_types();
    let key_ty = pk_info.key_type();
    let mut result = Vec::new();
    for idx in indexes {
        let idx_name = format_ident!("idx_{}", idx.name);
        let field_tys: Vec<&Type> = idx.fields.iter().map(|(_, ty)| ty).collect();
        match idx.kind {
            IndexKind::BTree => {
                result.push(quote! {
                    #idx_name: ::std::collections::BTreeSet<(#(#field_tys,)* #(#pk_tys),*)>
                });
            }
            IndexKind::Hash => {
                // Hash key type: single field → field type, compound → tuple.
                let hash_key_ty = if field_tys.len() == 1 {
                    let ty = field_tys[0];
                    quote! { #ty }
                } else {
                    quote! { (#(#field_tys),*) }
                };
                if idx.is_unique {
                    result.push(quote! {
                        #idx_name: ::tabulosity::InsOrdHashMap<#hash_key_ty, #key_ty>
                    });
                } else {
                    // Non-unique: inner collection depends on primary storage.
                    let inner_ty = match primary_storage {
                        PrimaryStorageKind::BTree => {
                            quote! { ::std::collections::BTreeSet<#key_ty> }
                        }
                        PrimaryStorageKind::Hash => {
                            quote! { ::tabulosity::InsOrdHashMap<#key_ty, ()> }
                        }
                    };
                    result.push(quote! {
                        #idx_name: ::tabulosity::InsOrdHashMap<#hash_key_ty, ::tabulosity::OneOrMany<#key_ty, #inner_ty>>
                    });
                }
            }
            IndexKind::Spatial => {
                let none_name = format_ident!("idx_{}_none", idx.name);
                let none_set_ty = match primary_storage {
                    PrimaryStorageKind::BTree => {
                        quote! { ::std::collections::BTreeSet<#key_ty> }
                    }
                    PrimaryStorageKind::Hash => {
                        quote! { ::tabulosity::InsOrdHashMap<#key_ty, ()> }
                    }
                };

                if let Some(cs) = &idx.compound_spatial {
                    // Compound spatial: Map<PrefixKey, SpatialIndex<PK, Point>>
                    let spatial_field_ty = &idx.fields[cs.spatial_field_index].1;
                    let prefix_key_ty = gen_compound_prefix_key_ty(cs, &idx.fields);

                    let (outer_map_ty, outer_none_map_ty) = match cs.prefix_kind {
                        IndexKind::BTree => (
                            quote! {
                                ::std::collections::BTreeMap<#prefix_key_ty, ::tabulosity::SpatialIndex<
                                    #key_ty,
                                    <<#spatial_field_ty as ::tabulosity::MaybeSpatialKey>::Key as ::tabulosity::SpatialKey>::Point,
                                >>
                            },
                            quote! {
                                ::std::collections::BTreeMap<#prefix_key_ty, #none_set_ty>
                            },
                        ),
                        IndexKind::Hash => (
                            quote! {
                                ::tabulosity::InsOrdHashMap<#prefix_key_ty, ::tabulosity::SpatialIndex<
                                    #key_ty,
                                    <<#spatial_field_ty as ::tabulosity::MaybeSpatialKey>::Key as ::tabulosity::SpatialKey>::Point,
                                >>
                            },
                            quote! {
                                ::tabulosity::InsOrdHashMap<#prefix_key_ty, #none_set_ty>
                            },
                        ),
                        IndexKind::Spatial => unreachable!("prefix kind cannot be Spatial"),
                    };

                    result.push(quote! { #idx_name: #outer_map_ty });
                    result.push(quote! { #none_name: #outer_none_map_ty });
                } else {
                    // Single-field spatial index.
                    let field_ty = field_tys[0];
                    result.push(quote! {
                        #idx_name: ::tabulosity::SpatialIndex<
                            #key_ty,
                            <<#field_ty as ::tabulosity::MaybeSpatialKey>::Key as ::tabulosity::SpatialKey>::Point,
                        >
                    });
                    result.push(quote! {
                        #none_name: #none_set_ty
                    });
                }
            }
        }
    }
    result
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

fn gen_idx_field_inits(
    indexes: &[ResolvedIndex],
    primary_storage: PrimaryStorageKind,
) -> Vec<TokenStream> {
    let mut result = Vec::new();
    for idx in indexes {
        let idx_name = format_ident!("idx_{}", idx.name);
        match idx.kind {
            IndexKind::BTree => {
                result.push(quote! { #idx_name: ::std::collections::BTreeSet::new() });
            }
            IndexKind::Hash => {
                result.push(quote! { #idx_name: ::tabulosity::InsOrdHashMap::new() });
            }
            IndexKind::Spatial => {
                let none_name = format_ident!("idx_{}_none", idx.name);
                if let Some(cs) = &idx.compound_spatial {
                    // Compound spatial: initialize outer maps to empty.
                    match cs.prefix_kind {
                        IndexKind::BTree => {
                            result.push(quote! { #idx_name: ::std::collections::BTreeMap::new() });
                            result.push(quote! { #none_name: ::std::collections::BTreeMap::new() });
                        }
                        IndexKind::Hash => {
                            result.push(quote! { #idx_name: ::tabulosity::InsOrdHashMap::new() });
                            result.push(quote! { #none_name: ::tabulosity::InsOrdHashMap::new() });
                        }
                        IndexKind::Spatial => unreachable!("prefix kind cannot be Spatial"),
                    }
                } else {
                    result.push(quote! { #idx_name: ::tabulosity::SpatialIndex::new() });
                    match primary_storage {
                        PrimaryStorageKind::BTree => {
                            result.push(quote! { #none_name: ::std::collections::BTreeSet::new() });
                        }
                        PrimaryStorageKind::Hash => {
                            result.push(quote! { #none_name: ::tabulosity::InsOrdHashMap::new() });
                        }
                    }
                }
            }
        }
    }
    result
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
    pk_info: &PkInfo,
    all_fields: &[ParsedField],
) -> Vec<TokenStream> {
    let mut wideners = Vec::new();
    // Deduplicate by field name so that each field contributing to tracked
    // bounds is widened exactly once. Multiple fields sharing a type suffix
    // widen the same `_bounds_*` tracker, which is correct (conservative).
    let mut seen_fields = std::collections::BTreeSet::new();

    for idx in indexes {
        // Hash indexes don't need bounds tracking.
        if idx.kind == IndexKind::Hash {
            continue;
        }
        // Spatial indexes: only compound spatial with btree prefix needs
        // bounds widening, and only for prefix fields.
        if idx.kind == IndexKind::Spatial {
            if let Some(cs) = &idx.compound_spatial
                && cs.prefix_kind == IndexKind::BTree
            {
                for &i in &cs.prefix_field_indices {
                    let field_ident = &idx.fields[i].0;
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
            continue;
        }
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

    // Widen bounds for each PK field.
    for (pk_ident, pk_ty) in &pk_info.fields {
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
    pk_info: &PkInfo,
    table_name_str: &str,
) -> Vec<TokenStream> {
    indexes
        .iter()
        .filter(|idx| idx.is_unique)
        .map(|idx| gen_unique_check_insert(idx, pk_info, table_name_str))
        .collect()
}

fn gen_unique_check_insert(
    idx: &ResolvedIndex,
    pk_info: &PkInfo,
    table_name_str: &str,
) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let idx_name_str = &idx.name;

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

    let uniqueness_check = match idx.kind {
        IndexKind::BTree => {
            // Generate bounds checks and min/max vars for each PK field.
            let (pk_bounds_checks, pk_min_clones, pk_max_clones) =
                gen_pk_bounds_for_unique(pk_info);

            quote! {
                #(#pk_bounds_checks)*
                {
                    let __start = (#(#field_clones,)* #(#pk_min_clones),*);
                    let __end = (#(#field_clones,)* #(#pk_max_clones),*);
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
        IndexKind::Hash => {
            let hash_key = gen_hash_key_expr(&field_clones);
            quote! {
                if self.#idx_name.contains_key(&#hash_key) {
                    return ::std::result::Result::Err(::tabulosity::Error::DuplicateIndex {
                        table: #table_name_str,
                        index: #idx_name_str,
                        key: ::std::format!("{:?}", (#(&#key_fmt),*)),
                    });
                }
            }
        }
        // Spatial indexes cannot be unique — rejected at parse time.
        IndexKind::Spatial => unreachable!("spatial indexes cannot be unique"),
    };

    match &idx.filter {
        Some(filter_path) => {
            let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
            quote! {
                if #filter_fn(&row) {
                    #uniqueness_check
                }
            }
        }
        None => uniqueness_check,
    }
}

/// Generate the PK bounds variable declarations, start clones, and end clones
/// for unique index range checks. Returns (bounds_checks, min_clones, max_clones).
fn gen_pk_bounds_for_unique(
    pk_info: &PkInfo,
) -> (Vec<TokenStream>, Vec<TokenStream>, Vec<TokenStream>) {
    let mut bounds_checks = Vec::new();
    let mut min_clones = Vec::new();
    let mut max_clones = Vec::new();
    let mut seen_suffixes = std::collections::BTreeSet::new();

    for (i, (_, pk_ty)) in pk_info.fields.iter().enumerate() {
        let suffix = type_suffix(pk_ty);
        let bounds_name = format_ident!("_bounds_{}", suffix);
        let min_var = format_ident!("__pk_min_{}", i);
        let max_var = format_ident!("__pk_max_{}", i);

        if seen_suffixes.insert(suffix) {
            // First time seeing this type — emit the bounds check.
            // If bounds are None (table empty), we can skip the unique check
            // because there can't be any conflicts.
            bounds_checks.push(quote! {
                let (#min_var, #max_var) = match &self.#bounds_name {
                    ::std::option::Option::Some((lo, hi)) => (lo.clone(), hi.clone()),
                    ::std::option::Option::None => { return ::std::result::Result::Ok(()); }
                };
            });
        } else {
            // Same type as a previous PK field — reuse its bounds.
            let first_idx = pk_info
                .fields
                .iter()
                .position(|(_, ty)| type_suffix(ty) == type_suffix(pk_ty))
                .unwrap();
            let first_min = format_ident!("__pk_min_{}", first_idx);
            let first_max = format_ident!("__pk_max_{}", first_idx);
            bounds_checks.push(quote! {
                let #min_var = #first_min.clone();
                let #max_var = #first_max.clone();
            });
        }

        min_clones.push(quote! { #min_var.clone() });
        max_clones.push(quote! { #max_var.clone() });
    }

    (bounds_checks, min_clones, max_clones)
}

/// Generate uniqueness checks for update. Runs before any mutation.
/// Only checks when field values actually changed.
fn gen_unique_checks_update(
    indexes: &[ResolvedIndex],
    pk_info: &PkInfo,
    table_name_str: &str,
) -> Vec<TokenStream> {
    indexes
        .iter()
        .filter(|idx| idx.is_unique)
        .map(|idx| gen_unique_check_update(idx, pk_info, table_name_str))
        .collect()
}

fn gen_unique_check_update(
    idx: &ResolvedIndex,
    pk_info: &PkInfo,
    table_name_str: &str,
) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let idx_name_str = &idx.name;

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

    let dup_error = quote! {
        return ::std::result::Result::Err(::tabulosity::Error::DuplicateIndex {
            table: #table_name_str,
            index: #idx_name_str,
            key: ::std::format!("{:?}", (#(&#key_fmt),*)),
        });
    };

    match idx.kind {
        IndexKind::BTree => {
            let (pk_bounds_checks, pk_min_clones, pk_max_clones) =
                gen_pk_bounds_for_unique(pk_info);

            let range_check = quote! {
                if #(#field_changed_check)||* {
                    #(#pk_bounds_checks)*
                    {
                        let __start = (#(#field_clones,)* #(#pk_min_clones),*);
                        let __end = (#(#field_clones,)* #(#pk_max_clones),*);
                        if self.#idx_name.range(__start..=__end).next().is_some() {
                            #dup_error
                        }
                    }
                }
            };

            match &idx.filter {
                Some(filter_path) => {
                    let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
                    quote! {
                        if #filter_fn(&row) {
                            let __needs_check = !#filter_fn(&old_row) || (#(#field_changed_check)||*);
                            if __needs_check {
                                #(#pk_bounds_checks)*
                                {
                                    let __start = (#(#field_clones,)* #(#pk_min_clones),*);
                                    let __end = (#(#field_clones,)* #(#pk_max_clones),*);
                                    if self.#idx_name.range(__start..=__end).next().is_some() {
                                        #dup_error
                                    }
                                }
                            }
                        }
                    }
                }
                None => range_check,
            }
        }
        IndexKind::Hash => {
            let hash_key = gen_hash_key_expr(&field_clones);

            let hash_check = quote! {
                if #(#field_changed_check)||* {
                    if self.#idx_name.contains_key(&#hash_key) {
                        #dup_error
                    }
                }
            };

            match &idx.filter {
                Some(filter_path) => {
                    let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
                    quote! {
                        if #filter_fn(&row) {
                            let __needs_check = !#filter_fn(&old_row) || (#(#field_changed_check)||*);
                            if __needs_check {
                                if self.#idx_name.contains_key(&#hash_key) {
                                    #dup_error
                                }
                            }
                        }
                    }
                }
                None => hash_check,
            }
        }
        // Spatial indexes cannot be unique — rejected at parse time.
        IndexKind::Spatial => unreachable!("spatial indexes cannot be unique"),
    }
}

// =============================================================================
// Index maintenance codegen
// =============================================================================

fn gen_all_idx_insert(
    indexes: &[ResolvedIndex],
    pk_info: &PkInfo,
    primary_storage: PrimaryStorageKind,
) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| gen_idx_insert(idx, pk_info, primary_storage))
        .collect()
}

fn gen_all_idx_update(
    indexes: &[ResolvedIndex],
    pk_info: &PkInfo,
    primary_storage: PrimaryStorageKind,
) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| gen_idx_update(idx, pk_info, primary_storage))
        .collect()
}

fn gen_all_idx_remove(
    indexes: &[ResolvedIndex],
    pk_info: &PkInfo,
    primary_storage: PrimaryStorageKind,
) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| gen_idx_remove(idx, pk_info, primary_storage))
        .collect()
}

fn gen_idx_insert(
    idx: &ResolvedIndex,
    pk_info: &PkInfo,
    primary_storage: PrimaryStorageKind,
) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let field_clones: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi.clone() })
        .collect();

    let insert_stmt = match idx.kind {
        IndexKind::BTree => {
            let pk_clones = pk_info.index_pk_clones_from_row();
            quote! {
                self.#idx_name.insert((#(#field_clones,)* #(#pk_clones),*));
            }
        }
        IndexKind::Hash => {
            let hash_key = gen_hash_key_expr(&field_clones);
            let pk_expr = pk_info.extract_key_from_row();
            if idx.is_unique {
                quote! {
                    self.#idx_name.insert(#hash_key, #pk_expr);
                }
            } else {
                let om_insert = gen_one_or_many_insert(primary_storage);
                quote! {
                    match self.#idx_name.get_mut(&#hash_key) {
                        ::std::option::Option::Some(__om) => {
                            __om.#om_insert(#pk_expr);
                        }
                        ::std::option::Option::None => {
                            self.#idx_name.insert(#hash_key, ::tabulosity::OneOrMany::One(#pk_expr));
                        }
                    }
                }
            }
        }
        IndexKind::Spatial => {
            if let Some(cs) = &idx.compound_spatial {
                gen_compound_spatial_insert(cs, idx, primary_storage, "row")
            } else {
                let none_name = format_ident!("idx_{}_none", idx.name);
                let none_insert = gen_none_set_insert(primary_storage);
                let fi = &idx.fields[0].0;
                // `pk` variable is in scope from the calling insert_no_fk method.
                quote! {
                    match ::tabulosity::MaybeSpatialKey::as_spatial(&row.#fi) {
                        ::std::option::Option::Some(__key) => {
                            self.#idx_name.insert(__key, pk.clone());
                        }
                        ::std::option::Option::None => {
                            self.#none_name.#none_insert;
                        }
                    }
                }
            }
        }
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

fn gen_idx_update(
    idx: &ResolvedIndex,
    pk_info: &PkInfo,
    primary_storage: PrimaryStorageKind,
) -> TokenStream {
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

    match idx.kind {
        IndexKind::BTree => {
            let pk_var_clones = pk_info.index_pk_clones_from_var();

            let remove_stmt = quote! {
                self.#idx_name.remove(&(#(#old_field_clones,)* #(#pk_var_clones),*));
            };
            let insert_stmt = quote! {
                self.#idx_name.insert((#(#new_field_clones,)* #(#pk_var_clones),*));
            };

            gen_idx_update_with_stmts(idx, &field_changed_check, &remove_stmt, &insert_stmt)
        }
        IndexKind::Hash => {
            let old_hash_key = gen_hash_key_expr(&old_field_clones);
            let new_hash_key = gen_hash_key_expr(&new_field_clones);
            let pk_expr = pk_info.extract_key_from_var();

            if idx.is_unique {
                let remove_stmt = quote! {
                    self.#idx_name.remove(&#old_hash_key);
                };
                let insert_stmt = quote! {
                    self.#idx_name.insert(#new_hash_key, #pk_expr);
                };
                gen_idx_update_with_stmts(idx, &field_changed_check, &remove_stmt, &insert_stmt)
            } else {
                let om_insert = gen_one_or_many_insert(primary_storage);
                let om_remove = gen_one_or_many_remove(primary_storage);
                let remove_stmt = quote! {
                    if let ::std::option::Option::Some(__om) = self.#idx_name.get_mut(&#old_hash_key) {
                        let __result = __om.#om_remove(&#pk_expr);
                        if __result == ::tabulosity::RemoveResult::Empty {
                            self.#idx_name.remove(&#old_hash_key);
                        }
                    }
                };
                let insert_stmt = quote! {
                    match self.#idx_name.get_mut(&#new_hash_key) {
                        ::std::option::Option::Some(__om) => {
                            __om.#om_insert(#pk_expr);
                        }
                        ::std::option::Option::None => {
                            self.#idx_name.insert(#new_hash_key, ::tabulosity::OneOrMany::One(#pk_expr));
                        }
                    }
                };
                gen_idx_update_with_stmts(idx, &field_changed_check, &remove_stmt, &insert_stmt)
            }
        }
        IndexKind::Spatial => {
            let none_name = format_ident!("idx_{}_none", idx.name);
            let none_remove = gen_none_set_remove(primary_storage);

            if let Some(cs) = &idx.compound_spatial {
                // Compound spatial: bypass gen_idx_update_with_stmts and implement
                // full four-branch filter logic with prefix/spatial change detection.
                let spatial_fi = &idx.fields[cs.spatial_field_index].0;
                let old_prefix_expr = gen_compound_prefix_key_expr(cs, &idx.fields, "old_row");

                // Prefix changed check.
                let prefix_changed_checks: Vec<TokenStream> = cs
                    .prefix_field_indices
                    .iter()
                    .map(|&i| {
                        let fi = &idx.fields[i].0;
                        quote! { old_row.#fi != row.#fi }
                    })
                    .collect();

                // Remove from old partition.
                let remove_from_old = quote! {
                    let __old_prefix = #old_prefix_expr;
                    match ::tabulosity::MaybeSpatialKey::as_spatial(&old_row.#spatial_fi) {
                        ::std::option::Option::Some(__key) => {
                            if let ::std::option::Option::Some(__p) = self.#idx_name.get_mut(&__old_prefix) {
                                __p.remove(__key, &pk);
                            }
                        }
                        ::std::option::Option::None => {
                            if let ::std::option::Option::Some(__ns) = self.#none_name.get_mut(&__old_prefix) {
                                __ns.#none_remove;
                            }
                        }
                    }
                };

                // Partition cleanup for old prefix (only needed when prefix changed).
                let partition_cleanup = quote! {
                    if __prefix_changed {
                        let __rtree_empty = self.#idx_name
                            .get(&__old_prefix).map_or(true, |p| p.is_empty());
                        let __none_empty = self.#none_name
                            .get(&__old_prefix).map_or(true, |s| s.is_empty());
                        if __rtree_empty && __none_empty {
                            self.#idx_name.remove(&__old_prefix);
                            self.#none_name.remove(&__old_prefix);
                        }
                    }
                };

                // Insert into new partition (reuses the compound spatial insert helper).
                let insert_into_new = gen_compound_spatial_insert(cs, idx, primary_storage, "row");

                // Remove-only with cleanup (for filter true→false).
                let remove_only_with_cleanup = quote! {
                    #remove_from_old
                    // Always check cleanup when removing without reinserting.
                    {
                        let __prefix_changed = true;
                        #partition_cleanup
                    }
                };

                let true_true_body = quote! {
                    let __prefix_changed = #(#prefix_changed_checks)||*;
                    let __spatial_changed = old_row.#spatial_fi != row.#spatial_fi;
                    if __prefix_changed || __spatial_changed {
                        #remove_from_old
                        #partition_cleanup
                        #insert_into_new
                    }
                };

                match &idx.filter {
                    Some(filter_path) => {
                        let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
                        quote! {
                            {
                                let old_passes = #filter_fn(&old_row);
                                let new_passes = #filter_fn(&row);
                                match (old_passes, new_passes) {
                                    (true, true) => {
                                        #true_true_body
                                    }
                                    (true, false) => {
                                        #remove_only_with_cleanup
                                    }
                                    (false, true) => {
                                        #insert_into_new
                                    }
                                    (false, false) => {}
                                }
                            }
                        }
                    }
                    None => {
                        quote! {
                            #true_true_body
                        }
                    }
                }
            } else {
                let none_insert = gen_none_set_insert(primary_storage);
                let fi = &idx.fields[0].0;

                // Remove from old location (R-tree or none-set).
                let remove_stmt = quote! {
                    match ::tabulosity::MaybeSpatialKey::as_spatial(&old_row.#fi) {
                        ::std::option::Option::Some(__key) => {
                            self.#idx_name.remove(__key, &pk);
                        }
                        ::std::option::Option::None => {
                            self.#none_name.#none_remove;
                        }
                    }
                };
                // Insert at new location (R-tree or none-set).
                let insert_stmt = quote! {
                    match ::tabulosity::MaybeSpatialKey::as_spatial(&row.#fi) {
                        ::std::option::Option::Some(__key) => {
                            self.#idx_name.insert(__key, pk.clone());
                        }
                        ::std::option::Option::None => {
                            self.#none_name.#none_insert;
                        }
                    }
                };
                gen_idx_update_with_stmts(idx, &field_changed_check, &remove_stmt, &insert_stmt)
            }
        }
    }
}

/// Helper: wrap remove+insert stmts with filter/changed logic for updates.
fn gen_idx_update_with_stmts(
    idx: &ResolvedIndex,
    field_changed_check: &[TokenStream],
    remove_stmt: &TokenStream,
    insert_stmt: &TokenStream,
) -> TokenStream {
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
                                #remove_stmt
                                #insert_stmt
                            }
                        }
                        (true, false) => {
                            #remove_stmt
                        }
                        (false, true) => {
                            #insert_stmt
                        }
                        (false, false) => {}
                    }
                }
            }
        }
        None => {
            quote! {
                if #(#field_changed_check)||* {
                    #remove_stmt
                    #insert_stmt
                }
            }
        }
    }
}

fn gen_idx_remove(
    idx: &ResolvedIndex,
    pk_info: &PkInfo,
    primary_storage: PrimaryStorageKind,
) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let field_clones: Vec<TokenStream> = idx
        .fields
        .iter()
        .map(|(fi, _)| quote! { row.#fi.clone() })
        .collect();

    match idx.kind {
        IndexKind::BTree => {
            let pk_var_clones = pk_info.index_pk_clones_from_var();
            quote! {
                self.#idx_name.remove(&(#(#field_clones,)* #(#pk_var_clones),*));
            }
        }
        IndexKind::Hash => {
            let hash_key = gen_hash_key_expr(&field_clones);
            if idx.is_unique {
                quote! {
                    self.#idx_name.remove(&#hash_key);
                }
            } else {
                let pk_expr = pk_info.extract_key_from_var();
                let om_remove = gen_one_or_many_remove(primary_storage);
                quote! {
                    if let ::std::option::Option::Some(__om) = self.#idx_name.get_mut(&#hash_key) {
                        let __result = __om.#om_remove(&#pk_expr);
                        if __result == ::tabulosity::RemoveResult::Empty {
                            self.#idx_name.remove(&#hash_key);
                        }
                    }
                }
            }
        }
        IndexKind::Spatial => {
            let none_name = format_ident!("idx_{}_none", idx.name);
            let none_remove = gen_none_set_remove(primary_storage);

            if let Some(cs) = &idx.compound_spatial {
                let spatial_fi = &idx.fields[cs.spatial_field_index].0;
                let prefix_key_expr = gen_compound_prefix_key_expr(cs, &idx.fields, "row");

                quote! {
                    let __prefix = #prefix_key_expr;
                    match ::tabulosity::MaybeSpatialKey::as_spatial(&row.#spatial_fi) {
                        ::std::option::Option::Some(__key) => {
                            if let ::std::option::Option::Some(__partition) = self.#idx_name.get_mut(&__prefix) {
                                __partition.remove(__key, &pk);
                            }
                        }
                        ::std::option::Option::None => {
                            if let ::std::option::Option::Some(__none_set) = self.#none_name.get_mut(&__prefix) {
                                __none_set.#none_remove;
                            }
                        }
                    }
                    // Partition cleanup: remove empty partitions.
                    let __rtree_empty = self.#idx_name
                        .get(&__prefix).map_or(true, |p| p.is_empty());
                    let __none_empty = self.#none_name
                        .get(&__prefix).map_or(true, |s| s.is_empty());
                    if __rtree_empty && __none_empty {
                        self.#idx_name.remove(&__prefix);
                        self.#none_name.remove(&__prefix);
                    }
                }
            } else {
                let fi = &idx.fields[0].0;
                quote! {
                    match ::tabulosity::MaybeSpatialKey::as_spatial(&row.#fi) {
                        ::std::option::Option::Some(__key) => {
                            self.#idx_name.remove(__key, &pk);
                        }
                        ::std::option::Option::None => {
                            self.#none_name.#none_remove;
                        }
                    }
                }
            }
        }
    }
}

/// Generates the `modify_unchecked` method for the table companion struct.
///
/// In debug builds, snapshots the PK and all indexed fields before the closure
/// and asserts they are unchanged after. In release builds, the method is just
/// `BTreeMap::get_mut` + closure.
fn gen_modify_unchecked(
    pk_info: &PkInfo,
    row_name: &Ident,
    table_name_str: &str,
    indexes: &[ResolvedIndex],
    checksummed: bool,
) -> TokenStream {
    let key_ty = pk_info.key_type();

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

    let row_ident = format_ident!("row");
    let (pk_snap_stmts, pk_assert_stmts) =
        pk_info.gen_modify_unchecked_pk_checks(&row_ident, "modify_unchecked");

    // Debug-build snapshot: clone PK fields + each indexed field.
    let snap_stmts: Vec<TokenStream> = pk_snap_stmts
        .into_iter()
        .chain(indexed_fields.iter().map(|fi| {
            let snap_name = format_ident!("__snap_{}", fi);
            quote! {
                #[cfg(debug_assertions)]
                let #snap_name = row.#fi.clone();
            }
        }))
        .collect();

    // Debug-build assertions: verify PK fields + each indexed field unchanged.
    let assert_stmts: Vec<TokenStream> = pk_assert_stmts
        .into_iter()
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

    // CRC dirtying after the closure returns (checksummed tables only).
    let crc_dirty_after = if checksummed {
        quote! {
            if let ::std::option::Option::Some(__old_crc) = __entry.crc {
                if let ::std::option::Option::Some(ref mut __xor) = self._crc_xor {
                    *__xor ^= __old_crc;
                }
                self._crc_dirty.push(pk.clone());
                __entry.crc = ::std::option::Option::None;
            } else if self._crc_xor.is_some() {
                // Already dirty — no-op.
            }
        }
    } else {
        quote! {}
    };

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
            pk: &#key_ty,
            f: impl ::std::ops::FnOnce(&mut #row_name),
        ) -> ::std::result::Result<(), ::tabulosity::Error> {
            let __entry = match self.rows.get_mut(pk) {
                ::std::option::Option::Some(r) => r,
                ::std::option::Option::None => {
                    return ::std::result::Result::Err(::tabulosity::Error::NotFound {
                        table: #table_name_str,
                        key: ::std::format!("{:?}", pk),
                    });
                }
            };

            let row = &mut __entry.row;
            #(#snap_stmts)*

            f(row);

            #[cfg(debug_assertions)]
            {
                #(#assert_stmts)*
            }

            #crc_dirty_after

            ::std::result::Result::Ok(())
        }
    }
}

/// Generates `modify_unchecked_range` and `modify_unchecked_all` methods.
fn gen_modify_unchecked_range(
    pk_info: &PkInfo,
    row_name: &Ident,
    indexes: &[ResolvedIndex],
    primary_storage: PrimaryStorageKind,
    checksummed: bool,
) -> TokenStream {
    let key_ty = pk_info.key_type();

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

    let row_ident = format_ident!("row");
    let method_label = if primary_storage == PrimaryStorageKind::Hash {
        "modify_unchecked_all"
    } else {
        "modify_unchecked_range"
    };
    let (pk_snap_stmts, pk_assert_stmts) =
        pk_info.gen_modify_unchecked_pk_checks(&row_ident, method_label);

    // Debug-build per-row snapshot + assertion stmts (inside the loop body).
    let idx_snap_stmts: Vec<TokenStream> = indexed_fields
        .iter()
        .map(|fi| {
            let snap_name = format_ident!("__snap_{}", fi);
            quote! {
                let #snap_name = row.#fi.clone();
            }
        })
        .collect();

    let idx_assert_stmts: Vec<TokenStream> = indexed_fields
        .iter()
        .map(|fi| {
            let snap_name = format_ident!("__snap_{}", fi);
            let field_str = fi.to_string();
            let msg = format!(
                "{}: indexed field `{}` was changed (from {{:?}} to {{:?}}); use update() instead",
                method_label, field_str
            );
            quote! {
                assert!(
                    row.#fi == #snap_name,
                    #msg,
                    #snap_name,
                    row.#fi,
                );
            }
        })
        .collect();

    // CRC dirtying after each row mutation (checksummed tables only).
    let crc_dirty_range = if checksummed {
        quote! {
            if let ::std::option::Option::Some(__old_crc) = __entry.crc {
                if let ::std::option::Option::Some(ref mut __xor) = self._crc_xor {
                    *__xor ^= __old_crc;
                }
                __dirty_pks.push(__pk.clone());
                __entry.crc = ::std::option::Option::None;
            } else if self._crc_xor.is_some() {
                // Already dirty — no-op.
            }
        }
    } else {
        quote! {}
    };

    let debug_loop_body = quote! {
        let row = &mut __entry.row;
        #(
            #[cfg(debug_assertions)]
            #pk_snap_stmts
        )*
        #(
            #[cfg(debug_assertions)]
            #idx_snap_stmts
        )*

        f(__pk, row);

        #[cfg(debug_assertions)]
        {
            #(#pk_assert_stmts)*
            #(#idx_assert_stmts)*
        }

        #crc_dirty_range

        __count += 1;
    };

    // Wrapper for collecting dirty PKs in checksummed tables.
    let dirty_pks_init = if checksummed {
        quote! { let mut __dirty_pks: ::std::vec::Vec<#key_ty> = ::std::vec::Vec::new(); }
    } else {
        quote! {}
    };
    let dirty_pks_flush = if checksummed {
        quote! { self._crc_dirty.append(&mut __dirty_pks); }
    } else {
        quote! {}
    };

    match primary_storage {
        PrimaryStorageKind::BTree => {
            quote! {
                /// Mutates all rows in a PK range via closure, bypassing index
                /// maintenance and FK validation. Returns the number of rows modified.
                #[doc(hidden)]
                pub fn modify_unchecked_range<__R: ::std::ops::RangeBounds<#key_ty>>(
                    &mut self,
                    range: __R,
                    mut f: impl ::std::ops::FnMut(&#key_ty, &mut #row_name),
                ) -> usize {
                    let mut __count = 0usize;
                    #dirty_pks_init
                    for (__pk, __entry) in self.rows.range_mut(range) {
                        #debug_loop_body
                    }
                    #dirty_pks_flush
                    __count
                }

                /// Mutates all rows in the table via closure, bypassing index
                /// maintenance and FK validation. Returns the number of rows modified.
                ///
                /// Equivalent to `modify_unchecked_range(.., f)`.
                #[doc(hidden)]
                pub fn modify_unchecked_all(
                    &mut self,
                    mut f: impl ::std::ops::FnMut(&#key_ty, &mut #row_name),
                ) -> usize {
                    self.modify_unchecked_range(.., f)
                }
            }
        }
        PrimaryStorageKind::Hash => {
            // Hash primary: modify_unchecked_range panics (InsOrdHashMap doesn't support range).
            // modify_unchecked_all iterates directly via iter_mut().
            quote! {
                /// **Not supported for hash primary storage.** Always panics.
                /// Use `modify_unchecked_all` instead.
                #[doc(hidden)]
                pub fn modify_unchecked_range<__R: ::std::ops::RangeBounds<#key_ty>>(
                    &mut self,
                    _range: __R,
                    _f: impl ::std::ops::FnMut(&#key_ty, &mut #row_name),
                ) -> usize {
                    panic!("modify_unchecked_range is not supported on tables with hash primary storage; use modify_unchecked_all instead")
                }

                /// Mutates all rows in the table via closure, bypassing index
                /// maintenance and FK validation. Returns the number of rows modified.
                #[doc(hidden)]
                pub fn modify_unchecked_all(
                    &mut self,
                    mut f: impl ::std::ops::FnMut(&#key_ty, &mut #row_name),
                ) -> usize {
                    let mut __count = 0usize;
                    #dirty_pks_init
                    for (__pk, __entry) in self.rows.iter_mut() {
                        #debug_loop_body
                    }
                    #dirty_pks_flush
                    __count
                }
            }
        }
    }
}

/// Generate the rebuild body for `post_deser_rebuild_indexes` (BTree + Spatial)
/// and `manual_rebuild_all_indexes` (all indexes).
fn gen_rebuild_body(
    indexes: &[ResolvedIndex],
    pk_info: &PkInfo,
    primary_storage: PrimaryStorageKind,
    include_hash: bool,
) -> Vec<TokenStream> {
    // In rebuild, `pk` is the BTreeMap key (already the correct key type).
    // For compound PKs, we need to destructure it into individual field values
    // for the index tuple.
    let pk_clone_exprs: Vec<TokenStream> = if pk_info.is_compound() {
        (0..pk_info.fields.len())
            .map(|i| {
                let idx = syn::Index::from(i);
                quote! { pk.#idx.clone() }
            })
            .collect()
    } else {
        vec![quote! { pk.clone() }]
    };

    indexes
        .iter()
        // BTree and Spatial indexes are always rebuilt (they're transient).
        // Hash indexes are only included in manual_rebuild_all_indexes.
        .filter(|idx| idx.kind != IndexKind::Hash || include_hash)
        .map(|idx| {
            let idx_name = format_ident!("idx_{}", idx.name);
            let field_clones: Vec<TokenStream> = idx
                .fields
                .iter()
                .map(|(fi, _)| quote! { row.#fi.clone() })
                .collect();

            let insert_stmt = match idx.kind {
                IndexKind::BTree => {
                    quote! {
                        self.#idx_name.insert((#(#field_clones,)* #(#pk_clone_exprs),*));
                    }
                }
                IndexKind::Hash => {
                    let hash_key = gen_hash_key_expr(&field_clones);
                    let pk_expr = quote! { pk.clone() };
                    if idx.is_unique {
                        quote! {
                            self.#idx_name.insert(#hash_key, #pk_expr);
                        }
                    } else {
                        let om_insert = gen_one_or_many_insert(primary_storage);
                        quote! {
                            match self.#idx_name.get_mut(&#hash_key) {
                                ::std::option::Option::Some(__om) => {
                                    __om.#om_insert(#pk_expr);
                                }
                                ::std::option::Option::None => {
                                    self.#idx_name.insert(#hash_key, ::tabulosity::OneOrMany::One(#pk_expr));
                                }
                            }
                        }
                    }
                }
                IndexKind::Spatial => {
                    let none_name = format_ident!("idx_{}_none", idx.name);
                    let none_insert = gen_none_set_insert(primary_storage);

                    if let Some(cs) = &idx.compound_spatial {
                        gen_compound_spatial_insert(cs, idx, primary_storage, "row")
                    } else {
                        let fi = &idx.fields[0].0;
                        quote! {
                            match ::tabulosity::MaybeSpatialKey::as_spatial(&row.#fi) {
                                ::std::option::Option::Some(__key) => {
                                    self.#idx_name.insert(__key, pk.clone());
                                }
                                ::std::option::Option::None => {
                                    self.#none_name.#none_insert;
                                }
                            }
                        }
                    }
                }
            };

            let body = match &idx.filter {
                Some(filter_path) => {
                    let filter_fn: syn::ExprPath = syn::parse_str(filter_path).unwrap();
                    quote! {
                        for (pk, __entry) in &self.rows {
                            let row = &__entry.row;
                            if #filter_fn(row) {
                                #insert_stmt
                            }
                        }
                    }
                }
                None => {
                    quote! {
                        for (pk, __entry) in &self.rows {
                            let row = &__entry.row;
                            #insert_stmt
                        }
                    }
                }
            };

            let clear_stmt = match idx.kind {
                IndexKind::BTree => quote! { self.#idx_name.clear(); },
                IndexKind::Hash => {
                    quote! {
                        self.#idx_name = ::tabulosity::InsOrdHashMap::with_capacity(self.rows.len());
                    }
                }
                IndexKind::Spatial => {
                    let none_name = format_ident!("idx_{}_none", idx.name);
                    quote! {
                        self.#idx_name.clear();
                        self.#none_name.clear();
                    }
                }
            };

            quote! {
                #clear_stmt
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
    pk_info: &PkInfo,
    row_name: &Ident,
    all_fields: &[ParsedField],
    primary_storage: PrimaryStorageKind,
) -> Vec<TokenStream> {
    indexes
        .iter()
        .map(|idx| gen_query_methods(idx, pk_info, row_name, all_fields, primary_storage))
        .collect()
}

fn gen_query_methods(
    idx: &ResolvedIndex,
    pk_info: &PkInfo,
    row_name: &Ident,
    all_fields: &[ParsedField],
    primary_storage: PrimaryStorageKind,
) -> TokenStream {
    // Spatial indexes get their own query method shape.
    if idx.kind == IndexKind::Spatial {
        return gen_spatial_query_methods(idx, pk_info, row_name);
    }

    let n = idx.fields.len();
    let by_fn = format_ident!("by_{}", idx.name);
    let iter_by_fn = format_ident!("iter_by_{}", idx.name);
    let count_by_fn = format_ident!("count_by_{}", idx.name);
    let helper_fn = format_ident!("_query_{}", idx.name);
    let idx_name = format_ident!("idx_{}", idx.name);

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

    let helper_body = match idx.kind {
        IndexKind::BTree => {
            // Field bounds names.
            let field_bounds_names: Vec<Ident> = idx
                .fields
                .iter()
                .map(|(fi, _)| {
                    let f = all_fields.iter().find(|f| &f.ident == fi).unwrap();
                    format_ident!("_bounds_{}", type_suffix(&f.ty))
                })
                .collect();

            gen_match_cascade(
                n,
                &qb_names,
                &field_tys,
                &field_bounds_names,
                pk_info,
                &idx_name,
                row_name,
            )
        }
        IndexKind::Hash => gen_hash_query_dispatch(
            n,
            &qb_names,
            &field_tys,
            pk_info,
            &idx_name,
            row_name,
            idx.is_unique,
            primary_storage,
            &idx.name,
        ),
        // Spatial indexes use gen_spatial_query_methods — early return above.
        IndexKind::Spatial => unreachable!("spatial handled by early return"),
    };

    // Forward calls from public methods to helper.
    let qb_forwards: Vec<TokenStream> = qb_names.iter().map(|qbn| quote! { #qbn }).collect();

    quote! {
        #[doc(hidden)]
        fn #helper_fn(&self, #(#params_querybound,)* __opts: ::tabulosity::QueryOpts) -> ::std::boxed::Box<dyn ::std::iter::Iterator<Item = &#row_name> + '_> {
            #helper_body
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

/// Generate query methods for spatial indexes.
///
/// Produces `intersecting_{name}(&self, envelope: &FieldType) -> Vec<Row>`,
/// `iter_intersecting_{name}`, and `count_intersecting_{name}`.
/// The envelope parameter type is the field's inner spatial key type (i.e.,
/// `T` for `T: SpatialKey` or `T` for `Option<T: SpatialKey>`).
fn gen_spatial_query_methods(
    idx: &ResolvedIndex,
    pk_info: &PkInfo,
    row_name: &Ident,
) -> TokenStream {
    let idx_name = format_ident!("idx_{}", idx.name);
    let key_ty = pk_info.key_type();

    if let Some(cs) = &idx.compound_spatial {
        // Compound spatial: query methods take prefix field params + envelope.
        let intersecting_fn = format_ident!("intersecting_by_{}", idx.name);
        let iter_intersecting_fn = format_ident!("iter_intersecting_by_{}", idx.name);
        let count_intersecting_fn = format_ident!("count_intersecting_by_{}", idx.name);

        let spatial_field_ty = &idx.fields[cs.spatial_field_index].1;
        let envelope_ty = quote! { <#spatial_field_ty as ::tabulosity::MaybeSpatialKey>::Key };

        // Prefix field params: each prefix field is a separate `&Type` parameter.
        let prefix_params: Vec<TokenStream> = cs
            .prefix_field_indices
            .iter()
            .map(|&i| {
                let fi = &idx.fields[i].0;
                let ty = &idx.fields[i].1;
                quote! { #fi: &#ty }
            })
            .collect();

        // Build partition lookup key expression.
        let prefix_lookup_key = if cs.prefix_field_indices.len() == 1 {
            let fi = &idx.fields[cs.prefix_field_indices[0]].0;
            quote! { #fi }
        } else {
            let clones: Vec<TokenStream> = cs
                .prefix_field_indices
                .iter()
                .map(|&i| {
                    let fi = &idx.fields[i].0;
                    quote! { #fi.clone() }
                })
                .collect();
            quote! { &(#(#clones),*) }
        };

        quote! {
            /// Returns cloned rows in the partition whose spatial key intersects
            /// the envelope, sorted by primary key.
            pub fn #intersecting_fn(&self, #(#prefix_params,)* __envelope: &#envelope_ty) -> ::std::vec::Vec<#row_name> {
                let __partition = match self.#idx_name.get(#prefix_lookup_key) {
                    ::std::option::Option::Some(p) => p,
                    ::std::option::Option::None => return ::std::vec::Vec::new(),
                };
                let __pks: ::std::vec::Vec<#key_ty> = __partition.intersecting(__envelope);
                __pks.into_iter()
                    .filter_map(|__pk| self.rows.get(&__pk).map(|__e| __e.row.clone()))
                    .collect()
            }

            /// Iterator variant.
            pub fn #iter_intersecting_fn(&self, #(#prefix_params,)* __envelope: &#envelope_ty) -> ::std::boxed::Box<dyn ::std::iter::Iterator<Item = &#row_name> + '_> {
                let __partition = match self.#idx_name.get(#prefix_lookup_key) {
                    ::std::option::Option::Some(p) => p,
                    ::std::option::Option::None => return ::std::boxed::Box::new(::std::iter::empty()),
                };
                let __pks: ::std::vec::Vec<#key_ty> = __partition.intersecting(__envelope);
                ::std::boxed::Box::new(
                    __pks.into_iter()
                        .filter_map(move |__pk| self.rows.get(&__pk).map(|__e| &__e.row))
                )
            }

            /// Count variant — no allocation (beyond R-tree traversal).
            pub fn #count_intersecting_fn(&self, #(#prefix_params,)* __envelope: &#envelope_ty) -> usize {
                match self.#idx_name.get(#prefix_lookup_key) {
                    ::std::option::Option::Some(p) => p.count_intersecting(__envelope),
                    ::std::option::Option::None => 0,
                }
            }
        }
    } else {
        // Single-field spatial: existing query shape.
        let intersecting_fn = format_ident!("intersecting_{}", idx.name);
        let iter_intersecting_fn = format_ident!("iter_intersecting_{}", idx.name);
        let count_intersecting_fn = format_ident!("count_intersecting_{}", idx.name);
        let field_ty = &idx.fields[0].1;
        let envelope_ty = quote! { <#field_ty as ::tabulosity::MaybeSpatialKey>::Key };

        quote! {
            /// Returns cloned rows whose spatial key intersects the given envelope,
            /// sorted by primary key.
            pub fn #intersecting_fn(&self, __envelope: &#envelope_ty) -> ::std::vec::Vec<#row_name> {
                let __pks: ::std::vec::Vec<#key_ty> = self.#idx_name.intersecting(__envelope);
                __pks.into_iter()
                    .filter_map(|__pk| self.rows.get(&__pk).map(|__e| __e.row.clone()))
                    .collect()
            }

            /// Returns a boxed iterator over references to rows whose spatial key
            /// intersects the given envelope, sorted by primary key.
            pub fn #iter_intersecting_fn(&self, __envelope: &#envelope_ty) -> ::std::boxed::Box<dyn ::std::iter::Iterator<Item = &#row_name> + '_> {
                let __pks: ::std::vec::Vec<#key_ty> = self.#idx_name.intersecting(__envelope);
                ::std::boxed::Box::new(
                    __pks.into_iter()
                        .filter_map(move |__pk| self.rows.get(&__pk).map(|__e| &__e.row))
                )
            }

            /// Returns the count of rows whose spatial key intersects the given envelope.
            pub fn #count_intersecting_fn(&self, __envelope: &#envelope_ty) -> usize {
                self.#idx_name.count_intersecting(__envelope)
            }
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
    pk_info: &PkInfo,
    idx_name: &Ident,
    row_name: &Ident,
) -> TokenStream {
    let row_lookup = pk_info.row_lookup_from_entry(n);

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
            pk_info,
            idx_name,
            row_name,
            &row_lookup,
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
    pk_info: &PkInfo,
    idx_name: &Ident,
    _row_name: &Ident,
    row_lookup: &TokenStream,
) -> TokenStream {
    let empty_ret = quote! {
        return ::std::boxed::Box::new(::std::iter::empty());
    };

    // We need PK bounds for each PK field.
    let mut pk_bounds_checks = Vec::new();
    let mut pk_start_elems = Vec::new();
    let mut pk_end_elems = Vec::new();
    let mut pk_seen_suffixes = std::collections::BTreeSet::new();

    for (i, (_, pk_ty)) in pk_info.fields.iter().enumerate() {
        let suffix = type_suffix(pk_ty);
        let bounds_name = format_ident!("_bounds_{}", suffix);
        let min_var = format_ident!("__pk_min_{}", i);
        let max_var = format_ident!("__pk_max_{}", i);

        if pk_seen_suffixes.insert(suffix) {
            pk_bounds_checks.push(quote! {
                let ::std::option::Option::Some((#min_var, #max_var)) = &self.#bounds_name else {
                    #empty_ret
                };
            });
        } else {
            let first_idx = pk_info
                .fields
                .iter()
                .position(|(_, ty)| type_suffix(ty) == type_suffix(pk_ty))
                .unwrap();
            let first_min = format_ident!("__pk_min_{}", first_idx);
            let first_max = format_ident!("__pk_max_{}", first_idx);
            pk_bounds_checks.push(quote! {
                let #min_var = #first_min;
                let #max_var = #first_max;
            });
        }

        pk_start_elems.push(quote! { #min_var.clone() });
        pk_end_elems.push(quote! { #max_var.clone() });
    }

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
        .chain(pk_start_elems.iter().cloned())
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
        .chain(pk_end_elems.iter().cloned())
        .collect();

    // Generate post-filter for fields num_exact..n.
    let needs_post_filter = num_exact < n;

    if needs_post_filter {
        let qb_clones: Vec<TokenStream> = (num_exact..n)
            .map(|i| {
                let clone_var = format_ident!("__qbc{}", i);
                let qbn = &qb_names[i];
                quote! { let #clone_var = #qbn.clone(); }
            })
            .collect();

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

        let qb_clones_desc: Vec<TokenStream> = (num_exact..n)
            .map(|i| {
                let clone_var = format_ident!("__qbc{}", i);
                let qbn = &qb_names[i];
                quote! { let #clone_var = #qbn.clone(); }
            })
            .collect();

        quote! {
            #(#pk_bounds_checks)*
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
                            .map(|__entry| #row_lookup)
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
                            .map(|__entry| #row_lookup)
                            .skip(__opts.offset)
                    )
                }
            }
        }
    } else {
        quote! {
            #(#pk_bounds_checks)*
            let __start = (#(#start_elems),*);
            let __end = (#(#end_elems),*);
            match __opts.order {
                ::tabulosity::QueryOrder::Asc => ::std::boxed::Box::new(
                    self.#idx_name.range(__start..=__end)
                        .map(|__entry| #row_lookup)
                        .skip(__opts.offset)
                ),
                ::tabulosity::QueryOrder::Desc => ::std::boxed::Box::new(
                    self.#idx_name.range(__start..=__end)
                        .rev()
                        .map(|__entry| #row_lookup)
                        .skip(__opts.offset)
                ),
            }
        }
    }
}

// =============================================================================
// Hash index query codegen
// =============================================================================

/// Generate query dispatch for hash indexes.
///
/// Hash indexes support:
/// - All fields Exact → O(1) hash lookup
/// - All fields MatchAll → iterate all entries
/// - Any field Range → runtime panic
/// - Mixed Exact/MatchAll → iterate + filter
#[allow(clippy::too_many_arguments)]
fn gen_hash_query_dispatch(
    n: usize,
    qb_names: &[Ident],
    _field_tys: &[&Type],
    _pk_info: &PkInfo,
    idx_name: &Ident,
    _row_name: &Ident,
    is_unique: bool,
    primary_storage: PrimaryStorageKind,
    index_name: &str,
) -> TokenStream {
    // Check for any Range query bounds → panic.
    let range_checks: Vec<TokenStream> = (0..n)
        .map(|i| {
            let qbn = &qb_names[i];
            let idx_name_str = index_name;
            quote! {
                if matches!(#qbn, ::tabulosity::QueryBound::Range { .. }) {
                    panic!("range queries are not supported on hash index `{}`; use Exact or MatchAll", #idx_name_str);
                }
            }
        })
        .collect();

    // Build the all-exact check.
    let exact_vars: Vec<Ident> = (0..n).map(|i| format_ident!("__v{}", i)).collect();
    let exact_pattern: Vec<TokenStream> = exact_vars
        .iter()
        .map(|v| quote! { ::tabulosity::QueryBound::Exact(#v) })
        .collect();

    let hash_key_from_exact = if n == 1 {
        let v = &exact_vars[0];
        quote! { #v }
    } else {
        let refs: Vec<TokenStream> = exact_vars.iter().map(|v| quote! { #v.clone() }).collect();
        quote! { (#(#refs),*) }
    };

    let exact_body = if is_unique {
        // Unique: get PK from index, look up row.
        quote! {
            match self.#idx_name.get(&#hash_key_from_exact) {
                ::std::option::Option::Some(__pk) => {
                    match self.rows.get(__pk).map(|__e| &__e.row) {
                        ::std::option::Option::Some(__row) => {
                            ::std::boxed::Box::new(::std::iter::once(__row).skip(__opts.offset))
                        }
                        ::std::option::Option::None => {
                            ::std::boxed::Box::new(::std::iter::empty())
                        }
                    }
                }
                ::std::option::Option::None => {
                    ::std::boxed::Box::new(::std::iter::empty())
                }
            }
        }
    } else {
        // Non-unique: get OneOrMany, iterate PKs, look up rows.
        let om_iter = gen_one_or_many_iter(primary_storage);
        quote! {
            match self.#idx_name.get(&#hash_key_from_exact) {
                ::std::option::Option::Some(__om) => {
                    ::std::boxed::Box::new(
                        __om.#om_iter()
                            .filter_map(|__pk| self.rows.get(__pk).map(|__e| &__e.row))
                            .skip(__opts.offset)
                    )
                }
                ::std::option::Option::None => {
                    ::std::boxed::Box::new(::std::iter::empty())
                }
            }
        }
    };

    // MatchAll body: iterate all index entries.
    let match_all_body = if is_unique {
        quote! {
            ::std::boxed::Box::new(
                self.#idx_name.values()
                    .filter_map(|__pk| self.rows.get(__pk).map(|__e| &__e.row))
                    .skip(__opts.offset)
            )
        }
    } else {
        let om_iter = gen_one_or_many_iter(primary_storage);
        quote! {
            ::std::boxed::Box::new(
                self.#idx_name.values()
                    .flat_map(|__om| __om.#om_iter())
                    .filter_map(|__pk| self.rows.get(__pk).map(|__e| &__e.row))
                    .skip(__opts.offset)
            )
        }
    };

    // Partial match body (for compound indexes): iterate + filter.
    // Only needed when n > 1 (compound), otherwise exact/matchall covers all.
    let partial_body = if n > 1 {
        // Iterate all entries, filter by exact fields.
        let filter_checks: Vec<TokenStream> = (0..n)
            .map(|i| {
                let qbn = &qb_names[i];
                let tuple_idx = syn::Index::from(i);
                quote! {
                    (match &#qbn {
                        ::tabulosity::QueryBound::Exact(__fv) => __k.#tuple_idx == *__fv,
                        ::tabulosity::QueryBound::MatchAll => true,
                        ::tabulosity::QueryBound::Range { .. } => unreachable!(),
                    })
                }
            })
            .collect();
        let combined_filter = quote! { #(#filter_checks)&&* };

        if is_unique {
            quote! {
                ::std::boxed::Box::new(
                    self.#idx_name.iter()
                        .filter(move |(__k, _)| #combined_filter)
                        .filter_map(|(_, __pk)| self.rows.get(__pk).map(|__e| &__e.row))
                        .skip(__opts.offset)
                )
            }
        } else {
            let om_iter = gen_one_or_many_iter(primary_storage);
            quote! {
                ::std::boxed::Box::new(
                    self.#idx_name.iter()
                        .filter(move |(__k, _)| #combined_filter)
                        .flat_map(|(_, __om)| __om.#om_iter())
                        .filter_map(|__pk| self.rows.get(__pk).map(|__e| &__e.row))
                        .skip(__opts.offset)
                )
            }
        }
    } else {
        // Single-field: no partial matches possible.
        quote! { unreachable!() }
    };

    // Build the match expression.
    if n == 1 {
        let qbn = &qb_names[0];
        quote! {
            #(#range_checks)*
            match &#qbn {
                #(#exact_pattern)|* => {
                    #exact_body
                }
                ::tabulosity::QueryBound::MatchAll => {
                    #match_all_body
                }
                _ => unreachable!(),
            }
        }
    } else {
        // For compound: check if all are exact, all are matchall, or mixed.
        let all_exact_checks: Vec<TokenStream> = qb_names
            .iter()
            .map(|qbn| quote! { matches!(#qbn, ::tabulosity::QueryBound::Exact(_)) })
            .collect();
        let all_matchall_checks: Vec<TokenStream> = qb_names
            .iter()
            .map(|qbn| quote! { matches!(#qbn, ::tabulosity::QueryBound::MatchAll) })
            .collect();

        // Extract exact values for the all-exact branch.
        let exact_extractions: Vec<TokenStream> = (0..n)
            .map(|i| {
                let qbn = &qb_names[i];
                let v = &exact_vars[i];
                quote! {
                    let #v = match &#qbn {
                        ::tabulosity::QueryBound::Exact(__v) => __v,
                        _ => unreachable!(),
                    };
                }
            })
            .collect();

        quote! {
            #(#range_checks)*
            if #(#all_exact_checks)&&* {
                #(#exact_extractions)*
                #exact_body
            } else if #(#all_matchall_checks)&&* {
                #match_all_body
            } else {
                #partial_body
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
    pk_info: &PkInfo,
    row_name: &Ident,
    checksummed: bool,
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
        // Spatial indexes don't have IntoQuery-based _query_ helpers;
        // skip modify_each generation for them.
        .filter(|idx| idx.kind != IndexKind::Spatial)
        .map(|idx| gen_modify_each_method(idx, pk_info, row_name, &indexed_fields, checksummed))
        .collect()
}

fn gen_modify_each_method(
    idx: &ResolvedIndex,
    pk_info: &PkInfo,
    row_name: &Ident,
    indexed_fields: &[&Ident],
    checksummed: bool,
) -> TokenStream {
    let key_ty = pk_info.key_type();
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
    let row_ident = format_ident!("__row");

    let (pk_snap_stmts, pk_assert_stmts) =
        pk_info.gen_modify_unchecked_pk_checks(&row_ident, &method_name_str);
    let snap_stmts: Vec<TokenStream> = pk_snap_stmts
        .into_iter()
        .chain(indexed_fields.iter().map(|fi| {
            let snap_name = format_ident!("__snap_{}", fi);
            quote! {
                #[cfg(debug_assertions)]
                let #snap_name = __row.#fi.clone();
            }
        }))
        .collect();

    let assert_stmts: Vec<TokenStream> = pk_assert_stmts
        .into_iter()
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

    // CRC dirtying after each row mutation (checksummed tables only).
    let crc_dirty_each = if checksummed {
        quote! {
            if let ::std::option::Option::Some(__old_crc) = __entry.crc {
                if let ::std::option::Option::Some(ref mut __xor) = self._crc_xor {
                    *__xor ^= __old_crc;
                }
                self._crc_dirty.push(__pk.clone());
                __entry.crc = ::std::option::Option::None;
            }
            // If crc is already None, row is already dirty — no-op.
        }
    } else {
        quote! {}
    };

    quote! {
        /// Mutates all matching rows in place via closure, bypassing index
        /// maintenance. In debug builds, asserts that the primary key and all
        /// indexed fields are unchanged after each closure call. Returns the
        /// number of rows modified.
        pub fn #modify_each_fn(
            &mut self,
            #(#params_into_query,)*
            opts: ::tabulosity::QueryOpts,
            mut f: impl ::std::ops::FnMut(&#key_ty, &mut #row_name),
        ) -> usize {
            #(#into_query_calls)*
            let __pks: ::std::vec::Vec<#key_ty> = self.#helper_fn(#(#qb_forwards,)* opts)
                .map(|__r| __r.pk_val())
                .collect();
            let mut __count = 0usize;
            for __pk in __pks {
                let __entry = self.rows.get_mut(&__pk).unwrap();
                let __row = &mut __entry.row;
                #(#snap_stmts)*
                f(&__pk, __row);
                #[cfg(debug_assertions)]
                {
                    #(#assert_stmts)*
                }
                #crc_dirty_each
                __count += 1;
            }
            __count
        }
    }
}

// =============================================================================
// Serde codegen
// =============================================================================

/// Generate serialize_field calls for each hash index.
fn gen_hash_idx_serialize(hash_indexes: &[&ResolvedIndex]) -> Vec<TokenStream> {
    hash_indexes
        .iter()
        .map(|idx| {
            let idx_name = format_ident!("idx_{}", idx.name);
            let idx_name_str = format!("idx_{}", idx.name);
            quote! {
                state.serialize_field(#idx_name_str, &self.#idx_name)?;
            }
        })
        .collect()
}

/// Generate Option variable declarations for each hash index in the visitor.
fn gen_hash_idx_var_decls(hash_indexes: &[&ResolvedIndex]) -> Vec<TokenStream> {
    hash_indexes
        .iter()
        .map(|idx| {
            let var_name = format_ident!("__idx_{}", idx.name);
            // The type is inferred from the assignment in visit_map.
            // We use Option<_> so the type can be inferred from next_value().
            quote! {
                let mut #var_name: ::std::option::Option<_> = ::std::option::Option::None;
            }
        })
        .collect()
}

/// Generate the post-row-load block: assigns hash indexes from deserialized
/// data and calls the appropriate rebuild method. Returns a single TokenStream.
///
/// When there are no hash indexes, just calls `post_deser_rebuild_indexes()`.
/// When there are hash indexes and all were present in the serialized data,
/// assigns them directly and calls `post_deser_rebuild_indexes()` (BTree only).
/// When some hash index fields are missing (backward compat with old saves),
/// calls `manual_rebuild_all_indexes()` to rebuild everything from rows.
fn gen_hash_idx_assign_and_rebuild(hash_indexes: &[&ResolvedIndex]) -> TokenStream {
    if hash_indexes.is_empty() {
        return quote! { table.post_deser_rebuild_indexes(); };
    }

    let var_names: Vec<Ident> = hash_indexes
        .iter()
        .map(|idx| format_ident!("__idx_{}", idx.name))
        .collect();
    let idx_fields: Vec<Ident> = hash_indexes
        .iter()
        .map(|idx| format_ident!("idx_{}", idx.name))
        .collect();

    let all_present_checks: Vec<TokenStream> =
        var_names.iter().map(|v| quote! { #v.is_some() }).collect();
    let assign_stmts: Vec<TokenStream> = var_names
        .iter()
        .zip(idx_fields.iter())
        .map(|(var, field)| {
            quote! { table.#field = #var.unwrap(); }
        })
        .collect();

    quote! {
        if #(#all_present_checks)&&* {
            // All hash index fields present — assign directly.
            #(#assign_stmts)*
            // Rebuild BTree indexes only (hash indexes already assigned).
            table.post_deser_rebuild_indexes();
        } else {
            // Some hash index fields missing (backward compat with old saves).
            // Rebuild ALL indexes including hash from rows.
            table.manual_rebuild_all_indexes();
        }
    }
}

/// Generate field name string literals for the FIELDS const array.
fn gen_hash_idx_field_names_const(hash_indexes: &[&ResolvedIndex]) -> Vec<String> {
    hash_indexes
        .iter()
        .map(|idx| format!("idx_{}", idx.name))
        .collect()
}

/// Generate deserialize match arms and field assignment for hash indexes.
fn gen_hash_idx_deserialize(hash_indexes: &[&ResolvedIndex]) -> Vec<TokenStream> {
    hash_indexes
        .iter()
        .map(|idx| {
            let idx_name_str = format!("idx_{}", idx.name);
            let var_name = format_ident!("__idx_{}", idx.name);
            quote! {
                #idx_name_str => {
                    if #var_name.is_some() {
                        return ::std::result::Result::Err(
                            ::serde::de::Error::duplicate_field(#idx_name_str)
                        );
                    }
                    #var_name = ::std::option::Option::Some(map.next_value()?);
                }
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn gen_serde_impls(
    table_name: &Ident,
    row_name: &Ident,
    pk_info: &PkInfo,
    table_name_str: &str,
    is_auto_increment: bool,
    nonpk_auto_field: &Option<(Ident, Type)>,
    hash_indexes: &[&ResolvedIndex],
    primary_storage: PrimaryStorageKind,
    row_entry_name: &Ident,
) -> TokenStream {
    // When hash indexes exist and are serialized directly,
    // post_deser_rebuild_indexes() handles BTree and Spatial indexes
    // (not Hash — those are deserialized directly).
    // When no hash indexes, same method covers everything.
    let rebuild_call = quote! { table.post_deser_rebuild_indexes(); };

    // TokenStream for constructing a RowEntry from a row in deserialization.
    let row_entry_wrap = quote! { #row_entry_name::new };

    // Generate serialize/deserialize statements for hash index fields.
    let hash_idx_ser = gen_hash_idx_serialize(hash_indexes);
    let hash_idx_deser = gen_hash_idx_deserialize(hash_indexes);

    if is_auto_increment {
        let pk_ty = &pk_info.fields[0].1;
        let pk_ident = &pk_info.fields[0].0;
        gen_serde_impls_auto(
            table_name,
            row_name,
            pk_ty,
            pk_ident,
            table_name_str,
            &rebuild_call,
            primary_storage,
            &hash_idx_ser,
            &hash_idx_deser,
            hash_indexes,
            &row_entry_wrap,
        )
    } else if let Some((auto_ident, auto_ty)) = nonpk_auto_field {
        gen_serde_impls_nonpk_auto(
            table_name,
            row_name,
            pk_info,
            auto_ident,
            auto_ty,
            table_name_str,
            &rebuild_call,
            &hash_idx_ser,
            &hash_idx_deser,
            hash_indexes,
            &row_entry_wrap,
        )
    } else {
        gen_serde_impls_plain(
            table_name,
            row_name,
            pk_info,
            table_name_str,
            &rebuild_call,
            &hash_idx_ser,
            &hash_idx_deser,
            hash_indexes,
            &row_entry_wrap,
        )
    }
}

/// Non-auto tables: serialize as bare JSON array (no hash indexes) or
/// struct with `rows` + `idx_*` fields (with hash indexes).
#[allow(clippy::too_many_arguments)]
fn gen_serde_impls_plain(
    table_name: &Ident,
    row_name: &Ident,
    pk_info: &PkInfo,
    table_name_str: &str,
    rebuild_call: &TokenStream,
    hash_idx_ser: &[TokenStream],
    hash_idx_deser: &[TokenStream],
    hash_indexes: &[&ResolvedIndex],
    row_entry_wrap: &TokenStream,
) -> TokenStream {
    let extract_key = pk_info.extract_key_from_row();

    if hash_indexes.is_empty() {
        // No hash indexes — serialize as flat JSON array.
        // Deserialize accepts BOTH:
        //   - `[row1, row2, ...]` (current format)
        //   - `{"next_id": N, "rows": [row1, ...], ...}` (old auto-PK format,
        //     for backward-compatible loading of saves created before the table
        //     was converted from auto-PK to natural/compound PK)
        let visitor_name = format_ident!("__{}PlainVisitor", table_name);
        let table_name_str_owned = table_name_str.to_string();
        quote! {
            #[cfg(feature = "serde")]
            impl ::serde::Serialize for #table_name
            where
                #row_name: ::serde::Serialize,
            {
                fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
                    use ::serde::ser::SerializeSeq;
                    let mut seq = serializer.serialize_seq(::std::option::Option::Some(self.rows.len()))?;
                    for __entry in self.rows.values() {
                        let row = &__entry.row;
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
                    struct #visitor_name;

                    impl #visitor_name {
                        fn build_table<E: ::serde::de::Error>(rows: ::std::vec::Vec<#row_name>) -> ::std::result::Result<#table_name, E> {
                            let mut table = #table_name::new();
                            for row in rows {
                                let pk = #extract_key;
                                if table.rows.contains_key(&pk) {
                                    return ::std::result::Result::Err(E::custom(
                                        ::std::format!("duplicate key in {}: {:?}", #table_name_str, pk),
                                    ));
                                }
                                table.rows.insert(pk, (#row_entry_wrap)(row));
                            }
                            #rebuild_call
                            ::std::result::Result::Ok(table)
                        }
                    }

                    impl<'de> ::serde::de::Visitor<'de> for #visitor_name
                    where
                        #row_name: ::serde::Deserialize<'de>,
                    {
                        type Value = #table_name;

                        fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                            formatter.write_str(#table_name_str_owned)
                        }

                        // Current format: flat JSON array of rows.
                        fn visit_seq<A: ::serde::de::SeqAccess<'de>>(
                            self,
                            mut seq: A,
                        ) -> ::std::result::Result<Self::Value, A::Error> {
                            let mut rows = ::std::vec::Vec::new();
                            while let ::std::option::Option::Some(row) = seq.next_element::<#row_name>()? {
                                rows.push(row);
                            }
                            Self::build_table(rows)
                        }

                        // Old auto-PK format: `{"next_id": N, "rows": [...]}`.
                        // Extract the "rows" array and ignore everything else.
                        fn visit_map<A: ::serde::de::MapAccess<'de>>(
                            self,
                            mut map: A,
                        ) -> ::std::result::Result<Self::Value, A::Error> {
                            let mut rows_vec: ::std::option::Option<::std::vec::Vec<#row_name>> = ::std::option::Option::None;
                            while let ::std::option::Option::Some(key) = map.next_key::<::std::string::String>()? {
                                if key == "rows" {
                                    if rows_vec.is_some() {
                                        return ::std::result::Result::Err(::serde::de::Error::duplicate_field("rows"));
                                    }
                                    rows_vec = ::std::option::Option::Some(map.next_value()?);
                                } else {
                                    // Skip unknown fields (next_id, etc.).
                                    let _ = map.next_value::<::serde::de::IgnoredAny>()?;
                                }
                            }
                            let rows = rows_vec.unwrap_or_default();
                            Self::build_table(rows)
                        }
                    }

                    deserializer.deserialize_any(#visitor_name)
                }
            }
        }
    } else {
        // Has hash indexes — struct format with rows + idx_* fields.
        let n_fields = 1 + hash_indexes.len(); // rows + idx_*
        let visitor_name = format_ident!("__{}SerdeVisitor", table_name);
        let table_name_str_owned = table_name_str.to_string();

        let idx_var_decls = gen_hash_idx_var_decls(hash_indexes);
        let assign_and_rebuild = gen_hash_idx_assign_and_rebuild(hash_indexes);

        quote! {
            #[cfg(feature = "serde")]
            impl ::serde::Serialize for #table_name
            where
                #row_name: ::serde::Serialize,
            {
                fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
                    use ::serde::ser::SerializeStruct;
                    let mut state = serializer.serialize_struct(#table_name_str, #n_fields)?;
                    let rows_vec: ::std::vec::Vec<&#row_name> = self.rows.values().map(|__e| &__e.row).collect();
                    state.serialize_field("rows", &rows_vec)?;
                    #(#hash_idx_ser)*
                    state.end()
                }
            }

            #[cfg(feature = "serde")]
            impl<'de> ::serde::Deserialize<'de> for #table_name
            where
                #row_name: ::serde::Deserialize<'de>,
            {
                fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {
                    struct #visitor_name;

                    impl<'de> ::serde::de::Visitor<'de> for #visitor_name
                    where
                        #row_name: ::serde::Deserialize<'de>,
                    {
                        type Value = #table_name;

                        fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                            formatter.write_str(#table_name_str_owned)
                        }

                        // New format: struct with "rows" + "idx_*" fields.
                        fn visit_map<A: ::serde::de::MapAccess<'de>>(
                            self,
                            mut map: A,
                        ) -> ::std::result::Result<Self::Value, A::Error> {
                            let mut rows_vec: ::std::option::Option<::std::vec::Vec<#row_name>> = ::std::option::Option::None;
                            #(#idx_var_decls)*

                            while let ::std::option::Option::Some(key) = map.next_key::<::std::string::String>()? {
                                match key.as_str() {
                                    "rows" => {
                                        if rows_vec.is_some() {
                                            return ::std::result::Result::Err(::serde::de::Error::duplicate_field("rows"));
                                        }
                                        rows_vec = ::std::option::Option::Some(map.next_value()?);
                                    }
                                    #(#hash_idx_deser)*
                                    _ => {
                                        let _ = map.next_value::<::serde::de::IgnoredAny>()?;
                                    }
                                }
                            }

                            let rows = rows_vec.ok_or_else(|| ::serde::de::Error::missing_field("rows"))?;

                            let mut table = #table_name::new();
                            for row in rows {
                                let pk = #extract_key;
                                if table.rows.contains_key(&pk) {
                                    return ::std::result::Result::Err(::serde::de::Error::custom(
                                        ::std::format!("duplicate key in {}: {:?}", #table_name_str_owned, pk),
                                    ));
                                }
                                table.rows.insert(pk, (#row_entry_wrap)(row));
                            }
                            // Assign deserialized hash indexes and rebuild as needed.
                            #assign_and_rebuild

                            ::std::result::Result::Ok(table)
                        }

                        // Backward compat: old format was a flat array of rows.
                        fn visit_seq<A: ::serde::de::SeqAccess<'de>>(
                            self,
                            mut seq: A,
                        ) -> ::std::result::Result<Self::Value, A::Error> {
                            let mut table = #table_name::new();
                            while let ::std::option::Option::Some(row) = seq.next_element::<#row_name>()? {
                                let pk = #extract_key;
                                if table.rows.contains_key(&pk) {
                                    return ::std::result::Result::Err(::serde::de::Error::custom(
                                        ::std::format!("duplicate key in {}: {:?}", #table_name_str_owned, pk),
                                    ));
                                }
                                table.rows.insert(pk, (#row_entry_wrap)(row));
                            }
                            // No hash index data — rebuild everything from rows.
                            table.manual_rebuild_all_indexes();
                            ::std::result::Result::Ok(table)
                        }
                    }

                    // Use deserialize_any so serde can dispatch to visit_seq (old format)
                    // or visit_map (new format) based on the data.
                    deserializer.deserialize_any(#visitor_name)
                }
            }
        }
    }
}

/// Auto-increment tables serialize as `{"next_id": N, "rows": [...]}`.
#[allow(clippy::too_many_arguments)]
fn gen_serde_impls_auto(
    table_name: &Ident,
    row_name: &Ident,
    pk_ty: &Type,
    pk_ident: &Ident,
    table_name_str: &str,
    _rebuild_call: &TokenStream,
    _primary_storage: PrimaryStorageKind,
    hash_idx_ser: &[TokenStream],
    hash_idx_deser: &[TokenStream],
    hash_indexes: &[&ResolvedIndex],
    row_entry_wrap: &TokenStream,
) -> TokenStream {
    let table_name_str_owned = table_name_str.to_string();
    let visitor_name = format_ident!("__{}SerdeVisitor", table_name);
    let n_fields = 2 + hash_indexes.len(); // next_id + rows + idx_*

    let idx_var_decls = gen_hash_idx_var_decls(hash_indexes);
    let assign_and_rebuild = gen_hash_idx_assign_and_rebuild(hash_indexes);
    let idx_field_names_const = gen_hash_idx_field_names_const(hash_indexes);

    quote! {
        #[cfg(feature = "serde")]
        impl ::serde::Serialize for #table_name
        where
            #row_name: ::serde::Serialize,
            #pk_ty: ::serde::Serialize,
        {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
                use ::serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(#table_name_str, #n_fields)?;
                state.serialize_field("next_id", &self.next_id)?;
                let rows_vec: ::std::vec::Vec<&#row_name> = self.rows.values().map(|__e| &__e.row).collect();
                state.serialize_field("rows", &rows_vec)?;
                #(#hash_idx_ser)*
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
                        #(#idx_var_decls)*

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
                                #(#hash_idx_deser)*
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
                            table.rows.insert(pk, (#row_entry_wrap)(row));
                        }
                        // Assign deserialized hash indexes and rebuild as needed.
                        #assign_and_rebuild

                        // Defensively set next_id to max(deserialized, max_pk_successor).
                        let mut effective_next_id = deserialized_next_id;
                        for __entry in table.rows.values() {
                            let max_successor = <#pk_ty as ::tabulosity::AutoIncrementable>::successor(&__entry.row.#pk_ident);
                            if max_successor > effective_next_id {
                                effective_next_id = max_successor;
                            }
                        }
                        table.next_id = effective_next_id;

                        ::std::result::Result::Ok(table)
                    }
                }

                const FIELDS: &[&str] = &["next_id", "rows", #(#idx_field_names_const),*];
                deserializer.deserialize_struct(#table_name_str_owned, FIELDS, #visitor_name)
            }
        }
    }
}

/// Non-PK auto-increment tables serialize as `{"next_<field>": N, "rows": [...]}`.
///
/// On deserialization, if `next_<field>` is missing (e.g., loading an old save where
/// the field used to be the auto-PK), the table computes `max(field) + 1` across all
/// loaded rows. The counter is then defensively set to `max(deserialized, computed)`.
#[allow(clippy::too_many_arguments)]
fn gen_serde_impls_nonpk_auto(
    table_name: &Ident,
    row_name: &Ident,
    pk_info: &PkInfo,
    auto_ident: &Ident,
    auto_ty: &Type,
    table_name_str: &str,
    _rebuild_call: &TokenStream,
    hash_idx_ser: &[TokenStream],
    hash_idx_deser: &[TokenStream],
    hash_indexes: &[&ResolvedIndex],
    row_entry_wrap: &TokenStream,
) -> TokenStream {
    let table_name_str_owned = table_name_str.to_string();
    let counter_name = format_ident!("next_{}", auto_ident);
    let counter_name_str = format!("next_{}", auto_ident);
    let visitor_name = format_ident!("__{}SerdeVisitor", table_name);
    let n_fields = 2 + hash_indexes.len();

    let extract_key = pk_info.extract_key_from_row();

    let idx_var_decls = gen_hash_idx_var_decls(hash_indexes);
    let assign_and_rebuild = gen_hash_idx_assign_and_rebuild(hash_indexes);
    let idx_field_names_const = gen_hash_idx_field_names_const(hash_indexes);

    quote! {
        #[cfg(feature = "serde")]
        impl ::serde::Serialize for #table_name
        where
            #row_name: ::serde::Serialize,
            #auto_ty: ::serde::Serialize,
        {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
                use ::serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(#table_name_str, #n_fields)?;
                state.serialize_field(#counter_name_str, &self.#counter_name)?;
                let rows_vec: ::std::vec::Vec<&#row_name> = self.rows.values().map(|__e| &__e.row).collect();
                state.serialize_field("rows", &rows_vec)?;
                #(#hash_idx_ser)*
                state.end()
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> ::serde::Deserialize<'de> for #table_name
        where
            #row_name: ::serde::Deserialize<'de>,
            #auto_ty: ::serde::Deserialize<'de> + ::tabulosity::AutoIncrementable,
        {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {
                struct #visitor_name;

                impl<'de> ::serde::de::Visitor<'de> for #visitor_name
                where
                    #row_name: ::serde::Deserialize<'de>,
                    #auto_ty: ::serde::Deserialize<'de> + ::tabulosity::AutoIncrementable,
                {
                    type Value = #table_name;

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str(#table_name_str_owned)
                    }

                    fn visit_map<A: ::serde::de::MapAccess<'de>>(
                        self,
                        mut map: A,
                    ) -> ::std::result::Result<Self::Value, A::Error> {
                        let mut counter: ::std::option::Option<#auto_ty> = ::std::option::Option::None;
                        let mut rows_vec: ::std::option::Option<::std::vec::Vec<#row_name>> = ::std::option::Option::None;
                        #(#idx_var_decls)*

                        while let ::std::option::Option::Some(key) = map.next_key::<::std::string::String>()? {
                            match key.as_str() {
                                #counter_name_str => {
                                    if counter.is_some() {
                                        return ::std::result::Result::Err(::serde::de::Error::duplicate_field(#counter_name_str));
                                    }
                                    counter = ::std::option::Option::Some(map.next_value()?);
                                }
                                "rows" => {
                                    if rows_vec.is_some() {
                                        return ::std::result::Result::Err(::serde::de::Error::duplicate_field("rows"));
                                    }
                                    rows_vec = ::std::option::Option::Some(map.next_value()?);
                                }
                                #(#hash_idx_deser)*
                                _ => {
                                    let _ = map.next_value::<::serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        let rows = rows_vec.ok_or_else(|| ::serde::de::Error::missing_field("rows"))?;

                        let mut table = #table_name::new();
                        for row in rows {
                            let pk = #extract_key;
                            if table.rows.contains_key(&pk) {
                                return ::std::result::Result::Err(::serde::de::Error::custom(
                                    ::std::format!("duplicate key in {}: {:?}", #table_name_str_owned, pk),
                                ));
                            }
                            table.rows.insert(pk, (#row_entry_wrap)(row));
                        }
                        // Assign deserialized hash indexes and rebuild as needed.
                        #assign_and_rebuild

                        // Compute max(field) + 1 from loaded rows.
                        let mut computed_next = <#auto_ty as ::tabulosity::AutoIncrementable>::first();
                        for __entry in table.rows.values() {
                            let successor = <#auto_ty as ::tabulosity::AutoIncrementable>::successor(&__entry.row.#auto_ident);
                            if successor > computed_next {
                                computed_next = successor;
                            }
                        }

                        // Use deserialized counter if present, else computed.
                        // Defensively take max of both.
                        let effective = match counter {
                            ::std::option::Option::Some(c) => {
                                if computed_next > c { computed_next } else { c }
                            }
                            ::std::option::Option::None => computed_next,
                        };
                        table.#counter_name = effective;

                        ::std::result::Result::Ok(table)
                    }
                }

                const FIELDS: &[&str] = &[#counter_name_str, "rows", #(#idx_field_names_const),*];
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
