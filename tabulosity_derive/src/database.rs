//! Implementation of `#[derive(Database)]`.
//!
//! Parses `#[table(singular = "...", fks(...))]` attributes on every field of
//! the database struct. Generates:
//!
//! - `new()` — creates all tables empty.
//! - `insert_{singular}`, `update_{singular}`, `upsert_{singular}`,
//!   `remove_{singular}` — safe write methods with FK validation.
//! - `Serialize` and `Deserialize` impls (behind `#[cfg(feature = "serde")]`).
//!   The `Deserialize` impl validates FK constraints and collects ALL errors
//!   (duplicate PKs + FK violations) into a `DeserializeError`, rather than
//!   failing fast on the first problem.
//!
//! FK validation on insert/update uses the `FkCheck` trait for uniform handling
//! of both `T` and `Option<T>` FK fields. Restrict-on-delete checks all inbound
//! FK references and collects ALL violations (no short-circuit).
//!
//! The `?` suffix on FK field names (e.g., `fks(assignee? = "creatures")`)
//! signals that the field is `Option<T>`, so the restrict check wraps the
//! target PK in `Some(...)` when querying the index.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Token};

/// A parsed FK declaration from `fks(field = "target_table")`.
struct FkDecl {
    field_name: Ident,
    is_optional: bool,
    target_table: String,
}

/// A parsed `#[table(singular = "...", fks(...))]` attribute.
struct TableAttr {
    singular: String,
    fks: Vec<FkDecl>,
}

/// A fully parsed table field with all metadata needed for codegen.
struct TableField {
    field_ident: Ident,
    attr: TableAttr,
}

/// Parse the contents of `fks(...)`.
struct FkList {
    fks: Vec<FkDecl>,
}

impl Parse for FkList {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut fks = Vec::new();
        while !input.is_empty() {
            let field_name: Ident = input.parse()?;
            let is_optional = input.peek(Token![?]);
            if is_optional {
                let _: Token![?] = input.parse()?;
            }
            let _: Token![=] = input.parse()?;
            let target: LitStr = input.parse()?;
            fks.push(FkDecl {
                field_name,
                is_optional,
                target_table: target.value(),
            });
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }
        Ok(FkList { fks })
    }
}

fn parse_table_attr(attr: &syn::Attribute) -> syn::Result<TableAttr> {
    let mut singular = None;
    let mut fks = Vec::new();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("singular") {
            let _: Token![=] = meta.input.parse()?;
            let lit: LitStr = meta.input.parse()?;
            singular = Some(lit.value());
            Ok(())
        } else if meta.path.is_ident("fks") {
            let content;
            syn::parenthesized!(content in meta.input);
            let fk_list: FkList = content.parse()?;
            fks = fk_list.fks;
            Ok(())
        } else {
            Err(meta.error("expected `singular` or `fks`"))
        }
    })?;

    let singular = singular.ok_or_else(|| {
        syn::Error::new_spanned(attr, "missing `singular = \"...\"` in #[table(...)]")
    })?;

    Ok(TableAttr { singular, fks })
}

pub fn derive(input: &DeriveInput) -> TokenStream {
    let db_name = &input.ident;

    let raw_fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return syn::Error::new_spanned(
                    db_name,
                    "Database can only be derived for structs with named fields",
                )
                .to_compile_error();
            }
        },
        _ => {
            return syn::Error::new_spanned(db_name, "Database can only be derived for structs")
                .to_compile_error();
        }
    };

    // Parse all table fields.
    let mut table_fields: Vec<TableField> = Vec::new();
    for field in raw_fields {
        let field_ident = field.ident.clone().expect("named field");
        let table_attr = field.attrs.iter().find(|a| a.path().is_ident("table"));

        let table_attr = match table_attr {
            Some(a) => match parse_table_attr(a) {
                Ok(ta) => ta,
                Err(e) => return e.to_compile_error(),
            },
            None => {
                return syn::Error::new_spanned(
                    &field_ident,
                    "all fields in a Database struct must have #[table(singular = \"...\")]",
                )
                .to_compile_error();
            }
        };

        table_fields.push(TableField {
            field_ident,
            attr: table_attr,
        });
    }

    // Build inverse FK map: for each target table, collect all inbound FK refs.
    let mut inbound_fks: std::collections::BTreeMap<String, Vec<(&Ident, &Ident, bool)>> =
        std::collections::BTreeMap::new();

    for tf in &table_fields {
        for fk in &tf.attr.fks {
            inbound_fks
                .entry(fk.target_table.clone())
                .or_default()
                .push((&tf.field_ident, &fk.field_name, fk.is_optional));
        }
    }

    // Generate new().
    let field_inits = table_fields.iter().map(|tf| {
        let ident = &tf.field_ident;
        let table_ty = &raw_fields
            .iter()
            .find(|f| f.ident.as_ref() == Some(&tf.field_ident))
            .unwrap()
            .ty;
        quote! { #ident: <#table_ty>::new() }
    });

    // Generate write methods for each table.
    let write_methods: Vec<TokenStream> = table_fields
        .iter()
        .map(|tf| {
            let table_ident = &tf.field_ident;
            let singular = &tf.attr.singular;
            let insert_fn = format_ident!("insert_{}", singular);
            let update_fn = format_ident!("update_{}", singular);
            let upsert_fn = format_ident!("upsert_{}", singular);
            let remove_fn = format_ident!("remove_{}", singular);
            let table_name_str = format!("{}s", singular);

            let table_ty = &raw_fields
                .iter()
                .find(|f| f.ident.as_ref() == Some(&tf.field_ident))
                .unwrap()
                .ty;

            // Derive row type name from table type name by stripping "Table".
            let table_ty_str = quote!(#table_ty).to_string();
            let row_ty_str = table_ty_str.trim_end_matches("Table");
            let row_ty = format_ident!("{}", row_ty_str);

            // FK checks for insert/update/upsert — same logic, generated 3 times
            // because quote iterators are consumed.
            let gen_fk_checks = || -> Vec<TokenStream> {
                tf.attr
                    .fks
                    .iter()
                    .map(|fk| {
                        let fk_field = &fk.field_name;
                        let fk_field_str = fk_field.to_string();
                        let target_table = format_ident!("{}", fk.target_table);
                        let target_table_str = &fk.target_table;

                        quote! {
                            if !::tabulosity::FkCheck::check_fk(&row.#fk_field, |k| self.#target_table.contains(k)) {
                                return ::std::result::Result::Err(::tabulosity::Error::FkTargetNotFound {
                                    table: #table_name_str,
                                    field: #fk_field_str,
                                    referenced_table: #target_table_str,
                                    key: ::std::format!("{:?}", row.#fk_field),
                                });
                            }
                        }
                    })
                    .collect()
            };

            let fk_checks_insert = gen_fk_checks();
            let fk_checks_update = gen_fk_checks();
            let fk_checks_upsert = gen_fk_checks();

            // Restrict-on-delete: check all inbound FK references.
            let target_table_name = tf.field_ident.to_string();
            let restrict_checks: Vec<TokenStream> = inbound_fks
                .get(&target_table_name)
                .map(|refs| {
                    refs.iter()
                        .map(|(src_table_ident, fk_field, is_optional)| {
                            let count_fn = format_ident!("count_by_{}", fk_field);
                            let src_table_str = src_table_ident.to_string();
                            let fk_field_str = fk_field.to_string();

                            if *is_optional {
                                quote! {
                                    let count = self.#src_table_ident.#count_fn(&::std::option::Option::Some(id.clone()));
                                    if count > 0 {
                                        violations.push((#src_table_str, #fk_field_str, count));
                                    }
                                }
                            } else {
                                quote! {
                                    let count = self.#src_table_ident.#count_fn(id);
                                    if count > 0 {
                                        violations.push((#src_table_str, #fk_field_str, count));
                                    }
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            quote! {
                pub fn #insert_fn(&mut self, row: #row_ty) -> ::std::result::Result<(), ::tabulosity::Error> {
                    #(#fk_checks_insert)*
                    self.#table_ident.insert_no_fk(row)
                }

                pub fn #update_fn(&mut self, row: #row_ty) -> ::std::result::Result<(), ::tabulosity::Error> {
                    #(#fk_checks_update)*
                    self.#table_ident.update_no_fk(row)
                }

                pub fn #upsert_fn(&mut self, row: #row_ty) -> ::std::result::Result<(), ::tabulosity::Error> {
                    #(#fk_checks_upsert)*
                    self.#table_ident.upsert_no_fk(row);
                    ::std::result::Result::Ok(())
                }

                pub fn #remove_fn(&mut self, id: &<#table_ty as ::tabulosity::TableMeta>::Key) -> ::std::result::Result<#row_ty, ::tabulosity::Error> {
                    if !self.#table_ident.contains(id) {
                        return ::std::result::Result::Err(::tabulosity::Error::NotFound {
                            table: #table_name_str,
                            key: ::std::format!("{:?}", id),
                        });
                    }

                    let mut violations: ::std::vec::Vec<(&'static str, &'static str, usize)> = ::std::vec::Vec::new();
                    #(#restrict_checks)*

                    if !violations.is_empty() {
                        return ::std::result::Result::Err(::tabulosity::Error::FkViolation {
                            table: #table_name_str,
                            key: ::std::format!("{:?}", id),
                            referenced_by: violations,
                        });
                    }

                    self.#table_ident.remove_no_fk(id)
                }
            }
        })
        .collect();

    let serde_impls = generate_serde_impls(db_name, &table_fields, raw_fields);

    quote! {
        impl #db_name {
            /// Creates a new database with all tables empty.
            pub fn new() -> Self {
                Self {
                    #(#field_inits,)*
                }
            }

            #(#write_methods)*
        }

        impl ::std::default::Default for #db_name {
            fn default() -> Self {
                Self::new()
            }
        }

        #serde_impls
    }
}

fn generate_serde_impls(
    db_name: &Ident,
    table_fields: &[TableField],
    raw_fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
) -> TokenStream {
    let field_count = table_fields.len();
    let db_name_str = db_name.to_string();

    // Collect info for each field.
    let field_infos: Vec<_> = table_fields
        .iter()
        .map(|tf| {
            let field_ident = &tf.field_ident;
            let field_name_str = field_ident.to_string();
            let table_ty = &raw_fields
                .iter()
                .find(|f| f.ident.as_ref() == Some(&tf.field_ident))
                .unwrap()
                .ty;
            let table_ty_str = quote!(#table_ty).to_string();
            let row_ty_str = table_ty_str.trim_end_matches("Table");
            let row_ty = format_ident!("{}", row_ty_str);
            let table_name_str = format!("{}s", &tf.attr.singular);

            (
                field_ident.clone(),
                field_name_str,
                table_ty.clone(),
                row_ty,
                table_name_str,
                &tf.attr.fks,
            )
        })
        .collect();

    // --- Serialize impl ---

    let serialize_fields: Vec<TokenStream> = field_infos
        .iter()
        .map(|(field_ident, field_name_str, _, _, _, _)| {
            quote! {
                state.serialize_field(#field_name_str, &self.#field_ident)?;
            }
        })
        .collect();

    let row_tys_ser: Vec<_> = field_infos
        .iter()
        .map(|(_, _, _, row_ty, _, _)| row_ty.clone())
        .collect();

    // --- Deserialize impl ---

    // Field name strings for the visitor.
    let field_name_strs: Vec<_> = field_infos
        .iter()
        .map(|(_, s, _, _, _, _)| s.clone())
        .collect();

    // Local variable names for deserialized Vec<Row>.
    let vec_var_names: Vec<Ident> = field_infos
        .iter()
        .map(|(fi, _, _, _, _, _)| format_ident!("__vec_{}", fi))
        .collect();

    // Local variable names for built tables.
    let table_var_names: Vec<Ident> = field_infos
        .iter()
        .map(|(fi, _, _, _, _, _)| format_ident!("__table_{}", fi))
        .collect();

    let row_tys_de: Vec<_> = field_infos
        .iter()
        .map(|(_, _, _, row_ty, _, _)| row_ty.clone())
        .collect();
    let table_tys_de: Vec<_> = field_infos
        .iter()
        .map(|(_, _, table_ty, _, _, _)| table_ty.clone())
        .collect();
    let table_name_strs: Vec<_> = field_infos
        .iter()
        .map(|(_, _, _, _, tns, _)| tns.clone())
        .collect();
    // Map access: deserialize each field as Vec<Row>.
    let map_access_arms: Vec<TokenStream> = field_infos
        .iter()
        .enumerate()
        .map(|(i, (_, field_name_str, _, row_ty, _, _))| {
            let vec_var = &vec_var_names[i];
            quote! {
                #field_name_str => {
                    if #vec_var.is_some() {
                        return ::std::result::Result::Err(
                            ::serde::de::Error::duplicate_field(#field_name_str),
                        );
                    }
                    #vec_var = ::std::option::Option::Some(
                        map.next_value::<::std::vec::Vec<#row_ty>>()?
                    );
                }
            }
        })
        .collect();

    // Build tables from vecs, collecting duplicate key errors.
    let build_tables: Vec<TokenStream> = field_infos
        .iter()
        .enumerate()
        .map(|(i, (_, field_name_str, _, _, _, _))| {
            let vec_var = &vec_var_names[i];
            let table_var = &table_var_names[i];
            let table_ty = &table_tys_de[i];
            quote! {
                let __rows = #vec_var.ok_or_else(|| {
                    ::serde::de::Error::missing_field(#field_name_str)
                })?;
                let mut #table_var = <#table_ty>::new();
                for __row in __rows {
                    if let ::std::result::Result::Err(e) = #table_var.insert_no_fk(__row) {
                        __errors.push(e);
                    }
                }
            }
        })
        .collect();

    // FK validation: iterate all rows in each table, check FKs against target tables.
    let fk_checks: Vec<TokenStream> = field_infos
        .iter()
        .enumerate()
        .filter_map(|(i, (_, _, _, _, _, fks))| {
            if fks.is_empty() {
                return None;
            }
            let table_var = &table_var_names[i];
            let table_name_str = &table_name_strs[i];

            let per_fk_checks: Vec<TokenStream> = fks
                .iter()
                .map(|fk| {
                    let fk_field = &fk.field_name;
                    let fk_field_str = fk_field.to_string();
                    // Find the index of the target table field.
                    let target_idx = field_infos
                        .iter()
                        .position(|(fi, _, _, _, _, _)| fi == &fk.target_table)
                        .unwrap_or_else(|| {
                            panic!(
                                "FK {}.{} references table '{}' which is not a field in the database struct",
                                fk.target_table, fk_field, fk.target_table,
                            )
                        });
                    let target_table_var = &table_var_names[target_idx];
                    let target_table_str = &fk.target_table;

                    quote! {
                        if !::tabulosity::FkCheck::check_fk(&__row.#fk_field, |__k| #target_table_var.contains(__k)) {
                            __errors.push(::tabulosity::Error::FkTargetNotFound {
                                table: #table_name_str,
                                field: #fk_field_str,
                                referenced_table: #target_table_str,
                                key: ::std::format!("{:?}", __row.#fk_field),
                            });
                        }
                    }
                })
                .collect();

            Some(quote! {
                for __row in #table_var.iter_all() {
                    #(#per_fk_checks)*
                }
            })
        })
        .collect();

    // Construct the database from built tables.
    let construct_fields: Vec<TokenStream> = field_infos
        .iter()
        .enumerate()
        .map(|(i, (field_ident, _, _, _, _, _))| {
            let table_var = &table_var_names[i];
            quote! { #field_ident: #table_var }
        })
        .collect();

    let visitor_name = format_ident!("__{}Visitor", db_name);
    let fields_const = format_ident!("__FIELDS_{}", db_name.to_string().to_uppercase());

    quote! {
        #[cfg(feature = "serde")]
        impl ::serde::Serialize for #db_name
        where
            #(#row_tys_ser: ::serde::Serialize,)*
        {
            fn serialize<__S: ::serde::Serializer>(
                &self,
                serializer: __S,
            ) -> ::std::result::Result<__S::Ok, __S::Error> {
                use ::serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(#db_name_str, #field_count)?;
                #(#serialize_fields)*
                state.end()
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> ::serde::Deserialize<'de> for #db_name
        where
            #(#row_tys_de: ::serde::Deserialize<'de>,)*
        {
            fn deserialize<__D: ::serde::Deserializer<'de>>(
                deserializer: __D,
            ) -> ::std::result::Result<Self, __D::Error> {
                const #fields_const: &[&str] = &[#(#field_name_strs),*];

                struct #visitor_name;

                impl<'de> ::serde::de::Visitor<'de> for #visitor_name
                where
                    #(#row_tys_de: ::serde::Deserialize<'de>,)*
                {
                    type Value = #db_name;

                    fn expecting(
                        &self,
                        formatter: &mut ::std::fmt::Formatter<'_>,
                    ) -> ::std::fmt::Result {
                        formatter.write_str(#db_name_str)
                    }

                    fn visit_map<__A: ::serde::de::MapAccess<'de>>(
                        self,
                        mut map: __A,
                    ) -> ::std::result::Result<Self::Value, __A::Error> {
                        #(let mut #vec_var_names: ::std::option::Option<::std::vec::Vec<#row_tys_de>> = ::std::option::Option::None;)*

                        while let ::std::option::Option::Some(__key) = map.next_key::<::std::string::String>()? {
                            match __key.as_str() {
                                #(#map_access_arms)*
                                _ => {
                                    let _ = map.next_value::<::serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        let mut __errors: ::std::vec::Vec<::tabulosity::Error> = ::std::vec::Vec::new();

                        // Build tables, collecting duplicate key errors.
                        #(#build_tables)*

                        // Validate FK constraints.
                        #(#fk_checks)*

                        if !__errors.is_empty() {
                            return ::std::result::Result::Err(::serde::de::Error::custom(
                                ::tabulosity::DeserializeError { errors: __errors },
                            ));
                        }

                        ::std::result::Result::Ok(#db_name {
                            #(#construct_fields,)*
                        })
                    }
                }

                deserializer.deserialize_struct(
                    #db_name_str,
                    #fields_const,
                    #visitor_name,
                )
            }
        }
    }
}
