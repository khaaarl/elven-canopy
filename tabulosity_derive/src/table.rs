//! Implementation of `#[derive(Table)]`.
//!
//! Generates a companion `{Name}Table` struct with:
//! - `rows: BTreeMap<PK, Row>` primary storage
//! - `idx_{field}: BTreeSet<(FieldType, PK)>` for each `#[indexed]` field
//! - Public read methods (get, get_ref, contains, len, is_empty, keys, etc.)
//! - `#[doc(hidden)] pub` mutation methods (_no_fk suffix)
//! - Per-index query methods (by_field, iter_by_field, count_by_field, range)
//! - `rebuild_indexes()` for deserialization
//! - `Serialize` / `Deserialize` impls (behind `#[cfg(feature = "serde")]`)
//!
//! Uses `parse.rs` for attribute extraction. All generated code uses fully
//! qualified paths (`::std::collections::BTreeMap`, `::tabulosity::Error`, etc.)
//! to avoid name conflicts in user code.
//!
//! **Error behavior note:** The table-level `Deserialize` impl fails fast on
//! the first duplicate PK (returning a plain serde error string). The
//! database-level `Deserialize` impl (in `database.rs`) instead collects all
//! duplicate-key errors across all tables plus FK violations into a structured
//! `DeserializeError`. This asymmetry is deliberate — standalone table deser
//! has no cross-table context for FK checks, while database deser can validate
//! everything and report all problems at once.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields};

use crate::parse::{self, ParsedField};

pub fn derive(input: &DeriveInput) -> TokenStream {
    let row_name = &input.ident;
    let vis = &input.vis;
    let table_name = format_ident!("{}Table", row_name);
    let table_name_str = format!("{}s", to_snake_case(&row_name.to_string()));

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => parse::parse_fields(named),
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

    // Find indexed fields.
    let indexed: Vec<&ParsedField> = fields.iter().filter(|f| f.is_indexed).collect();

    // Generate index field declarations for companion struct.
    let idx_field_decls = indexed.iter().map(|f| {
        let idx_name = format_ident!("idx_{}", f.ident);
        let field_ty = &f.ty;
        quote! {
            #idx_name: ::std::collections::BTreeSet<(#field_ty, #pk_ty)>
        }
    });

    // Generate index initialization for new().
    let idx_field_inits = indexed.iter().map(|f| {
        let idx_name = format_ident!("idx_{}", f.ident);
        quote! { #idx_name: ::std::collections::BTreeSet::new() }
    });

    // Generate index insertion for insert.
    let idx_insert = indexed.iter().map(|f| {
        let idx_name = format_ident!("idx_{}", f.ident);
        let field_ident = &f.ident;
        quote! {
            self.#idx_name.insert((row.#field_ident.clone(), row.#pk_ident.clone()));
        }
    });

    // Generate index update for update (compare old and new).
    let idx_update = indexed.iter().map(|f| {
        let idx_name = format_ident!("idx_{}", f.ident);
        let field_ident = &f.ident;
        quote! {
            if old_row.#field_ident != row.#field_ident {
                self.#idx_name.remove(&(old_row.#field_ident.clone(), pk.clone()));
                self.#idx_name.insert((row.#field_ident.clone(), pk.clone()));
            }
        }
    });

    // Generate index update for upsert-update path.
    let idx_upsert_update = indexed.iter().map(|f| {
        let idx_name = format_ident!("idx_{}", f.ident);
        let field_ident = &f.ident;
        quote! {
            if old_row.#field_ident != row.#field_ident {
                self.#idx_name.remove(&(old_row.#field_ident.clone(), pk.clone()));
                self.#idx_name.insert((row.#field_ident.clone(), pk.clone()));
            }
        }
    });

    // Generate index insertion for upsert-insert path.
    let idx_upsert_insert = indexed.iter().map(|f| {
        let idx_name = format_ident!("idx_{}", f.ident);
        let field_ident = &f.ident;
        quote! {
            self.#idx_name.insert((row.#field_ident.clone(), row.#pk_ident.clone()));
        }
    });

    // Generate index removal for remove.
    let idx_remove = indexed.iter().map(|f| {
        let idx_name = format_ident!("idx_{}", f.ident);
        let field_ident = &f.ident;
        quote! {
            self.#idx_name.remove(&(row.#field_ident.clone(), pk.clone()));
        }
    });

    // Generate rebuild_indexes body.
    let idx_clear_and_rebuild = indexed.iter().map(|f| {
        let idx_name = format_ident!("idx_{}", f.ident);
        let field_ident = &f.ident;
        quote! {
            self.#idx_name.clear();
            for (pk, row) in &self.rows {
                self.#idx_name.insert((row.#field_ident.clone(), pk.clone()));
            }
        }
    });

    // Generate per-index query methods.
    let idx_query_methods = indexed.iter().map(|f| {
        let field_ty = &f.ty;
        let idx_name = format_ident!("idx_{}", f.ident);
        let by_fn = format_ident!("by_{}", f.ident);
        let iter_by_fn = format_ident!("iter_by_{}", f.ident);
        let count_by_fn = format_ident!("count_by_{}", f.ident);
        let by_range_fn = format_ident!("by_{}_range", f.ident);
        let iter_by_range_fn = format_ident!("iter_by_{}_range", f.ident);
        let count_by_range_fn = format_ident!("count_by_{}_range", f.ident);

        quote! {
            /// Returns all rows matching the given value, in PK order within
            /// the group (cloned).
            pub fn #by_fn(&self, val: &#field_ty) -> ::std::vec::Vec<#row_name> {
                let start = (val.clone(), <#pk_ty as ::tabulosity::Bounded>::MIN);
                let end = (val.clone(), <#pk_ty as ::tabulosity::Bounded>::MAX);
                self.#idx_name.range(start..=end)
                    .map(|(_, pk)| self.rows.get(pk).unwrap().clone())
                    .collect()
            }

            /// Iterates over references to all rows matching the given value,
            /// in PK order within the group.
            pub fn #iter_by_fn(&self, val: &#field_ty) -> impl ::std::iter::Iterator<Item = &#row_name> {
                let start = (val.clone(), <#pk_ty as ::tabulosity::Bounded>::MIN);
                let end = (val.clone(), <#pk_ty as ::tabulosity::Bounded>::MAX);
                self.#idx_name.range(start..=end)
                    .map(|(_, pk)| self.rows.get(pk).unwrap())
            }

            /// Counts rows matching the given value without cloning.
            pub fn #count_by_fn(&self, val: &#field_ty) -> usize {
                let start = (val.clone(), <#pk_ty as ::tabulosity::Bounded>::MIN);
                let end = (val.clone(), <#pk_ty as ::tabulosity::Bounded>::MAX);
                self.#idx_name.range(start..=end).count()
            }

            /// Returns all rows in the given range of field values (cloned).
            pub fn #by_range_fn<R>(&self, range: R) -> ::std::vec::Vec<#row_name>
            where
                R: ::std::ops::RangeBounds<#field_ty>,
            {
                let start = ::tabulosity::map_start_bound::<#field_ty, #pk_ty>(range.start_bound());
                let end = ::tabulosity::map_end_bound::<#field_ty, #pk_ty>(range.end_bound());
                self.#idx_name.range((start, end))
                    .map(|(_, pk)| self.rows.get(pk).unwrap().clone())
                    .collect()
            }

            /// Iterates over references to rows in the given range of field values.
            pub fn #iter_by_range_fn<R>(&self, range: R) -> impl ::std::iter::Iterator<Item = &#row_name>
            where
                R: ::std::ops::RangeBounds<#field_ty>,
            {
                let start = ::tabulosity::map_start_bound::<#field_ty, #pk_ty>(range.start_bound());
                let end = ::tabulosity::map_end_bound::<#field_ty, #pk_ty>(range.end_bound());
                self.#idx_name.range((start, end))
                    .map(|(_, pk)| self.rows.get(pk).unwrap())
            }

            /// Counts rows in the given range of field values without cloning.
            pub fn #count_by_range_fn<R>(&self, range: R) -> usize
            where
                R: ::std::ops::RangeBounds<#field_ty>,
            {
                let start = ::tabulosity::map_start_bound::<#field_ty, #pk_ty>(range.start_bound());
                let end = ::tabulosity::map_end_bound::<#field_ty, #pk_ty>(range.end_bound());
                self.#idx_name.range((start, end)).count()
            }
        }
    });

    let table_name_str_ref = &table_name_str;

    quote! {
        /// Companion table struct generated by `#[derive(Table)]`.
        #vis struct #table_name {
            rows: ::std::collections::BTreeMap<#pk_ty, #row_name>,
            #(#idx_field_decls,)*
        }

        impl #table_name {
            /// Creates a new empty table.
            pub fn new() -> Self {
                Self {
                    rows: ::std::collections::BTreeMap::new(),
                    #(#idx_field_inits,)*
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

            #(#idx_query_methods)*

            // --- Mutation methods (doc-hidden, used by Database derive) ---

            /// Inserts a row. Returns `Err(DuplicateKey)` if the PK already exists.
            #[doc(hidden)]
            pub fn insert_no_fk(&mut self, row: #row_name) -> ::std::result::Result<(), ::tabulosity::Error> {
                let pk = row.#pk_ident.clone();
                if self.rows.contains_key(&pk) {
                    return ::std::result::Result::Err(::tabulosity::Error::DuplicateKey {
                        table: #table_name_str_ref,
                        key: ::std::format!("{:?}", pk),
                    });
                }
                #(#idx_insert)*
                self.rows.insert(pk, row);
                ::std::result::Result::Ok(())
            }

            /// Updates a row. Returns `Err(NotFound)` if the PK is missing.
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
                #(#idx_update)*
                self.rows.insert(pk, row);
                ::std::result::Result::Ok(())
            }

            /// Upserts a row — inserts if missing, updates if existing. Infallible.
            #[doc(hidden)]
            pub fn upsert_no_fk(&mut self, row: #row_name) {
                let pk = row.#pk_ident.clone();
                if let Some(old_row) = self.rows.get(&pk).cloned() {
                    #(#idx_upsert_update)*
                    self.rows.insert(pk, row);
                } else {
                    #(#idx_upsert_insert)*
                    self.rows.insert(pk, row);
                }
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

            /// Rebuilds all secondary indexes from the row data.
            #[doc(hidden)]
            pub fn rebuild_indexes(&mut self) {
                #(#idx_clear_and_rebuild)*
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
                            ::std::format!("duplicate key in {}: {:?}", #table_name_str_ref, pk),
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
