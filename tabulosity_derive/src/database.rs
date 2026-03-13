//! Implementation of `#[derive(Database)]`.
//!
//! Parses `#[table(singular = "...", fks(...))]` attributes on every field of
//! the database struct. Generates:
//!
//! - `new()` — creates all tables empty.
//! - `insert_{singular}`, `update_{singular}`, `upsert_{singular}`,
//!   `remove_{singular}` — safe write methods with FK validation.
//! - `modify_unchecked_{singular}`, `modify_unchecked_range_{singular}`,
//!   `modify_unchecked_all_{singular}` — closure-based in-place mutation that
//!   delegates to the table's `modify_unchecked*` methods (no FK re-check).
//! - `Serialize` and `Deserialize` impls (behind `#[cfg(feature = "serde")]`).
//!   The `Deserialize` impl validates FK constraints and collects ALL errors
//!   (duplicate PKs + FK violations) into a `DeserializeError`, rather than
//!   failing fast on the first problem. Missing tables in the serialized data
//!   default to empty, making additive schema changes (new tables) work without
//!   migration code.
//!
//! ## Schema versioning
//!
//! An optional struct-level `#[schema_version(N)]` attribute (where N is a u64)
//! enables schema versioning. When present:
//! - Serialization includes a `"schema_version": N` field in the JSON output.
//! - Deserialization requires the version field and rejects mismatches.
//!
//! When absent, no version field is serialized or expected.
//!
//! FK validation on insert/update uses the `FkCheck` trait for uniform handling
//! of both `T` and `Option<T>` FK fields.
//!
//! On delete, each inbound FK has an `on_delete` action:
//! - **restrict** (default): block deletion if any references exist.
//! - **cascade**: auto-delete dependent rows via the database-level `remove_*`.
//! - **nullify**: set the FK field to `None` (compile error if not `Option<T>`).
//!
//! The `?` suffix on FK field names (e.g., `fks(assignee? = "creatures")`)
//! signals that the field is `Option<T>`, so the restrict check wraps the
//! target PK in `Some(...)` when querying the index.
//!
//! ## Parent-PK FK (`pk` keyword)
//!
//! The `pk` keyword (e.g., `fks(id = "creatures" pk)`) marks an FK field as
//! also being the child table's primary key, creating a 1:1 relationship.
//! On delete, restrict/cascade checks use `contains()` instead of
//! `count_by_{field}()` since there is no secondary index on the PK.
//! The `pk` keyword is incompatible with `?` (optional) and `on_delete nullify`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Token};

/// What happens to dependent rows when the referenced row is deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OnDeleteAction {
    /// Block deletion if references exist (default).
    Restrict,
    /// Auto-delete dependent rows.
    Cascade,
    /// Set FK field to `None` (only valid for `Option<T>` fields).
    Nullify,
}

/// A parsed FK declaration from `fks(field = "target_table" [pk] [on_delete ...])`.
struct FkDecl {
    field_name: Ident,
    is_optional: bool,
    /// When true, this FK field is also the child table's primary key,
    /// creating a 1:1 relationship. Delete checks use `contains()` instead
    /// of `count_by_{field}()`, since there is no secondary index on the PK.
    is_pk: bool,
    target_table: String,
    on_delete: OnDeleteAction,
}

/// A parsed `#[table(singular = "...", fks(...), auto)]` attribute.
struct TableAttr {
    singular: String,
    fks: Vec<FkDecl>,
    is_auto: bool,
}

/// A fully parsed table field with all metadata needed for codegen.
struct TableField {
    field_ident: Ident,
    attr: TableAttr,
}

/// An inbound FK reference from a source table to a target table.
/// Used during delete codegen to generate restrict/cascade/nullify logic.
struct InboundFk<'a> {
    src_table: &'a Ident,
    fk_field: &'a Ident,
    is_optional: bool,
    is_pk: bool,
    on_delete: OnDeleteAction,
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

            // Parse optional `pk` keyword.
            let is_pk = if input.peek(Ident) && !input.peek2(Token![?]) && !input.peek2(Token![=]) {
                // Peek at the identifier to check if it's "pk".
                let fork = input.fork();
                let kw: Ident = fork.parse()?;
                if kw == "pk" {
                    let _: Ident = input.parse()?; // consume
                    true
                } else {
                    false
                }
            } else {
                false
            };

            // Parse optional `on_delete cascade|nullify|restrict`.
            let on_delete = if input.peek(Ident)
                && !input.peek2(Token![?])
                && !input.peek2(Token![=])
            {
                let kw: Ident = input.parse()?;
                if kw != "on_delete" {
                    return Err(syn::Error::new(
                        kw.span(),
                        format!("expected `on_delete` or `,`, got `{kw}`"),
                    ));
                }
                let action: Ident = input.parse()?;
                match action.to_string().as_str() {
                    "restrict" => OnDeleteAction::Restrict,
                    "cascade" => OnDeleteAction::Cascade,
                    "nullify" => OnDeleteAction::Nullify,
                    other => {
                        return Err(syn::Error::new(
                            action.span(),
                            format!("expected `restrict`, `cascade`, or `nullify`, got `{other}`"),
                        ));
                    }
                }
            } else {
                OnDeleteAction::Restrict
            };

            fks.push(FkDecl {
                field_name,
                is_optional,
                is_pk,
                target_table: target.value(),
                on_delete,
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
    let mut is_auto = false;

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
        } else if meta.path.is_ident("auto") {
            is_auto = true;
            Ok(())
        } else {
            Err(meta.error("expected `singular`, `fks`, or `auto`"))
        }
    })?;

    let singular = singular.ok_or_else(|| {
        syn::Error::new_spanned(attr, "missing `singular = \"...\"` in #[table(...)]")
    })?;

    Ok(TableAttr {
        singular,
        fks,
        is_auto,
    })
}

/// Parse an optional `#[schema_version(N)]` attribute from the struct's attrs.
fn parse_schema_version(attrs: &[syn::Attribute]) -> Option<u64> {
    for attr in attrs {
        if attr.path().is_ident("schema_version") {
            let version: syn::LitInt = attr
                .parse_args()
                .expect("#[schema_version] requires an integer literal, e.g. #[schema_version(1)]");
            return Some(
                version
                    .base10_parse::<u64>()
                    .expect("#[schema_version] must be a valid u64"),
            );
        }
    }
    None
}

pub fn derive(input: &DeriveInput) -> TokenStream {
    let db_name = &input.ident;
    let schema_version = parse_schema_version(&input.attrs);

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

    // Validate FK attribute combinations.
    for tf in &table_fields {
        for fk in &tf.attr.fks {
            if fk.on_delete == OnDeleteAction::Nullify && !fk.is_optional {
                return syn::Error::new_spanned(
                    &fk.field_name,
                    format!(
                        "on_delete nullify requires an optional FK field (`{}?`), \
                         but `{}` is a bare FK",
                        fk.field_name, fk.field_name
                    ),
                )
                .to_compile_error();
            }
            if fk.is_pk && fk.is_optional {
                return syn::Error::new_spanned(
                    &fk.field_name,
                    format!(
                        "`pk` FK cannot be optional (`{}?`): a primary key is never null",
                        fk.field_name,
                    ),
                )
                .to_compile_error();
            }
            if fk.is_pk && fk.on_delete == OnDeleteAction::Nullify {
                return syn::Error::new_spanned(
                    &fk.field_name,
                    format!(
                        "`pk` FK `{}` cannot use on_delete nullify: a primary key cannot be set to None",
                        fk.field_name,
                    ),
                )
                .to_compile_error();
            }
        }
    }

    // Validate: no cascade cycles. Build directed graph of cascade edges
    // (target_table → source_table) and DFS for back edges.
    {
        let mut cascade_graph: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for tf in &table_fields {
            for fk in &tf.attr.fks {
                if fk.on_delete == OnDeleteAction::Cascade {
                    cascade_graph
                        .entry(fk.target_table.clone())
                        .or_default()
                        .push(tf.field_ident.to_string());
                }
            }
        }
        if let Some(cycle) = detect_cycle(&cascade_graph) {
            return syn::Error::new_spanned(
                db_name,
                format!("cascade cycle detected: {}", cycle.join(" → ")),
            )
            .to_compile_error();
        }
    }

    // Build inverse FK map: for each target table, collect all inbound FK refs.
    let mut inbound_fks: std::collections::BTreeMap<String, Vec<InboundFk<'_>>> =
        std::collections::BTreeMap::new();

    for tf in &table_fields {
        for fk in &tf.attr.fks {
            inbound_fks
                .entry(fk.target_table.clone())
                .or_default()
                .push(InboundFk {
                    src_table: &tf.field_ident,
                    fk_field: &fk.field_name,
                    is_optional: fk.is_optional,
                    is_pk: fk.is_pk,
                    on_delete: fk.on_delete,
                });
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

            let modify_unchecked_fn = format_ident!("modify_unchecked_{}", singular);
            let modify_unchecked_range_fn =
                format_ident!("modify_unchecked_range_{}", singular);
            let modify_unchecked_all_fn = format_ident!("modify_unchecked_all_{}", singular);

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

            // Partition inbound FK refs by on_delete action.
            let target_table_name = tf.field_ident.to_string();
            let inbound = inbound_fks.get(&target_table_name);

            // 1. Restrict checks.
            let restrict_checks: Vec<TokenStream> = inbound
                .map(|refs| {
                    refs.iter()
                        .filter(|ifk| ifk.on_delete == OnDeleteAction::Restrict)
                        .map(|ifk| {
                            let src_table_ident = ifk.src_table;
                            let fk_field = ifk.fk_field;
                            let src_table_str = src_table_ident.to_string();
                            let fk_field_str = fk_field.to_string();

                            if ifk.is_pk {
                                // PK FK: use contains() — at most one child row.
                                quote! {
                                    if self.#src_table_ident.contains(id) {
                                        violations.push((#src_table_str, #fk_field_str, 1));
                                    }
                                }
                            } else if ifk.is_optional {
                                let count_fn = format_ident!("count_by_{}", fk_field);
                                quote! {
                                    let count = self.#src_table_ident.#count_fn(&::std::option::Option::Some(id.clone()), ::tabulosity::QueryOpts::ASC);
                                    if count > 0 {
                                        violations.push((#src_table_str, #fk_field_str, count));
                                    }
                                }
                            } else {
                                let count_fn = format_ident!("count_by_{}", fk_field);
                                quote! {
                                    let count = self.#src_table_ident.#count_fn(id, ::tabulosity::QueryOpts::ASC);
                                    if count > 0 {
                                        violations.push((#src_table_str, #fk_field_str, count));
                                    }
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            // 2. Cascade deletes.
            let cascade_stmts: Vec<TokenStream> = inbound
                .map(|refs| {
                    refs.iter()
                        .filter(|ifk| ifk.on_delete == OnDeleteAction::Cascade)
                        .map(|ifk| {
                            let src_table_ident = ifk.src_table;
                            let fk_field = ifk.fk_field;
                            let src_tf = table_fields
                                .iter()
                                .find(|t| t.field_ident == *src_table_ident)
                                .unwrap();
                            let src_remove_fn = format_ident!("remove_{}", src_tf.attr.singular);

                            if ifk.is_pk {
                                // PK FK: at most one child row — delete directly by PK.
                                quote! {
                                    if self.#src_table_ident.contains(id) {
                                        self.#src_remove_fn(id)?;
                                    }
                                }
                            } else {
                                let iter_fn = format_ident!("iter_by_{}", fk_field);
                                let src_table_ty = &raw_fields
                                    .iter()
                                    .find(|f| f.ident.as_ref() == Some(src_table_ident))
                                    .unwrap()
                                    .ty;

                                let query_val = if ifk.is_optional {
                                    quote! { &::std::option::Option::Some(id.clone()) }
                                } else {
                                    quote! { id }
                                };

                                quote! {
                                    {
                                        let __cascade_pks: ::std::vec::Vec<<#src_table_ty as ::tabulosity::TableMeta>::Key> =
                                            self.#src_table_ident.#iter_fn(#query_val, ::tabulosity::QueryOpts::ASC)
                                                .map(|__r| __r.pk_ref().clone())
                                                .collect();
                                        for __cpk in __cascade_pks {
                                            self.#src_remove_fn(&__cpk)?;
                                        }
                                    }
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            // 3. Nullify updates.
            let nullify_stmts: Vec<TokenStream> = inbound
                .map(|refs| {
                    refs.iter()
                        .filter(|ifk| ifk.on_delete == OnDeleteAction::Nullify)
                        .map(|ifk| {
                            let src_table_ident = ifk.src_table;
                            let fk_field = ifk.fk_field;
                            let iter_fn = format_ident!("iter_by_{}", fk_field);
                            let src_table_ty = &raw_fields
                                .iter()
                                .find(|f| f.ident.as_ref() == Some(src_table_ident))
                                .unwrap()
                                .ty;

                            quote! {
                                {
                                    let __nullify_pks: ::std::vec::Vec<<#src_table_ty as ::tabulosity::TableMeta>::Key> =
                                        self.#src_table_ident.#iter_fn(&::std::option::Option::Some(id.clone()), ::tabulosity::QueryOpts::ASC)
                                            .map(|__r| __r.pk_ref().clone())
                                            .collect();
                                    for __npk in __nullify_pks {
                                        let mut __row = self.#src_table_ident.get(&__npk).unwrap();
                                        __row.#fk_field = ::std::option::Option::None;
                                        self.#src_table_ident.update_no_fk(__row).unwrap();
                                    }
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            // Auto-increment insert method (only for auto tables).
            let auto_insert_method = if tf.attr.is_auto {
                let insert_auto_fn = format_ident!("insert_{}_auto", singular);
                let fk_checks_auto = gen_fk_checks();
                quote! {
                    pub fn #insert_auto_fn(
                        &mut self,
                        f: impl ::std::ops::FnOnce(<#table_ty as ::tabulosity::TableMeta>::Key) -> #row_ty,
                    ) -> ::std::result::Result<<#table_ty as ::tabulosity::TableMeta>::Key, ::tabulosity::Error> {
                        let pk = self.#table_ident.next_id();
                        let row = f(pk.clone());
                        #(#fk_checks_auto)*
                        self.#table_ident.insert_no_fk(row)?;
                        ::std::result::Result::Ok(pk)
                    }
                }
            } else {
                quote! {}
            };

            quote! {
                pub fn #insert_fn(&mut self, row: #row_ty) -> ::std::result::Result<(), ::tabulosity::Error> {
                    #(#fk_checks_insert)*
                    self.#table_ident.insert_no_fk(row)
                }

                #auto_insert_method

                pub fn #update_fn(&mut self, row: #row_ty) -> ::std::result::Result<(), ::tabulosity::Error> {
                    #(#fk_checks_update)*
                    self.#table_ident.update_no_fk(row)
                }

                pub fn #upsert_fn(&mut self, row: #row_ty) -> ::std::result::Result<(), ::tabulosity::Error> {
                    #(#fk_checks_upsert)*
                    self.#table_ident.upsert_no_fk(row)
                }

                pub fn #modify_unchecked_fn(
                    &mut self,
                    pk: &<#table_ty as ::tabulosity::TableMeta>::Key,
                    f: impl ::std::ops::FnOnce(&mut #row_ty),
                ) -> ::std::result::Result<(), ::tabulosity::Error> {
                    self.#table_ident.modify_unchecked(pk, f)
                }

                pub fn #modify_unchecked_range_fn<__R: ::std::ops::RangeBounds<<#table_ty as ::tabulosity::TableMeta>::Key>>(
                    &mut self,
                    range: __R,
                    f: impl ::std::ops::FnMut(&<#table_ty as ::tabulosity::TableMeta>::Key, &mut #row_ty),
                ) -> usize {
                    self.#table_ident.modify_unchecked_range(range, f)
                }

                pub fn #modify_unchecked_all_fn(
                    &mut self,
                    f: impl ::std::ops::FnMut(&<#table_ty as ::tabulosity::TableMeta>::Key, &mut #row_ty),
                ) -> usize {
                    self.#table_ident.modify_unchecked_all(f)
                }

                pub fn #remove_fn(&mut self, id: &<#table_ty as ::tabulosity::TableMeta>::Key) -> ::std::result::Result<(), ::tabulosity::Error> {
                    if !self.#table_ident.contains(id) {
                        return ::std::result::Result::Err(::tabulosity::Error::NotFound {
                            table: #table_name_str,
                            key: ::std::format!("{:?}", id),
                        });
                    }

                    // Phase 1: Restrict checks.
                    let mut violations: ::std::vec::Vec<(&'static str, &'static str, usize)> = ::std::vec::Vec::new();
                    #(#restrict_checks)*

                    if !violations.is_empty() {
                        return ::std::result::Result::Err(::tabulosity::Error::FkViolation {
                            table: #table_name_str,
                            key: ::std::format!("{:?}", id),
                            referenced_by: violations,
                        });
                    }

                    // Phase 2: Cascade deletes.
                    #(#cascade_stmts)*

                    // Phase 3: Nullify updates.
                    #(#nullify_stmts)*

                    // Phase 4: Remove the row.
                    self.#table_ident.remove_no_fk(id)?;
                    ::std::result::Result::Ok(())
                }
            }
        })
        .collect();

    let serde_impls = generate_serde_impls(db_name, &table_fields, raw_fields, schema_version);

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
    schema_version: Option<u64>,
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
                tf.attr.is_auto,
            )
        })
        .collect();

    // --- Serialize impl ---

    let ser_field_count = if schema_version.is_some() {
        field_count + 1
    } else {
        field_count
    };

    let serialize_version: TokenStream = if let Some(ver) = schema_version {
        quote! {
            state.serialize_field("schema_version", &#ver)?;
        }
    } else {
        quote! {}
    };

    let serialize_fields: Vec<TokenStream> = field_infos
        .iter()
        .map(|(field_ident, field_name_str, _, _, _, _, _)| {
            quote! {
                state.serialize_field(#field_name_str, &self.#field_ident)?;
            }
        })
        .collect();

    let row_tys_ser: Vec<_> = field_infos
        .iter()
        .map(|(_, _, _, row_ty, _, _, _)| row_ty.clone())
        .collect();

    // --- Deserialize impl ---

    // Field name strings for the visitor.
    let field_name_strs: Vec<_> = field_infos
        .iter()
        .map(|(_, s, _, _, _, _, _)| s.clone())
        .collect();

    // Local variable names for deserialized Vec<Row>.
    let vec_var_names: Vec<Ident> = field_infos
        .iter()
        .map(|(fi, _, _, _, _, _, _)| format_ident!("__vec_{}", fi))
        .collect();

    // Local variable names for built tables.
    let table_var_names: Vec<Ident> = field_infos
        .iter()
        .map(|(fi, _, _, _, _, _, _)| format_ident!("__table_{}", fi))
        .collect();

    let row_tys_de: Vec<_> = field_infos
        .iter()
        .map(|(_, _, _, row_ty, _, _, _)| row_ty.clone())
        .collect();
    let table_tys_de: Vec<_> = field_infos
        .iter()
        .map(|(_, _, table_ty, _, _, _, _)| table_ty.clone())
        .collect();
    let table_name_strs: Vec<_> = field_infos
        .iter()
        .map(|(_, _, _, _, tns, _, _)| tns.clone())
        .collect();
    // Map access: deserialize each field. Non-auto tables are deserialized as
    // Vec<Row>; auto tables are deserialized directly as the table type (since
    // they serialize as `{"next_id": N, "rows": [...]}`).
    let map_access_arms: Vec<TokenStream> = field_infos
        .iter()
        .enumerate()
        .map(|(i, (_, field_name_str, _, row_ty, _, _, is_auto))| {
            let vec_var = &vec_var_names[i];
            if *is_auto {
                let table_ty = &table_tys_de[i];
                quote! {
                    #field_name_str => {
                        if #vec_var.is_some() {
                            return ::std::result::Result::Err(
                                ::serde::de::Error::duplicate_field(#field_name_str),
                            );
                        }
                        #vec_var = ::std::option::Option::Some(
                            map.next_value::<#table_ty>()?
                        );
                    }
                }
            } else {
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
            }
        })
        .collect();

    // Build tables from vecs, collecting duplicate key errors. Missing tables
    // default to empty — this makes additive schema changes (new tables) work
    // without migration code.
    let build_tables: Vec<TokenStream> = field_infos
        .iter()
        .enumerate()
        .map(|(i, (_, _field_name_str, _, _, _, _, is_auto))| {
            let vec_var = &vec_var_names[i];
            let table_var = &table_var_names[i];
            let table_ty = &table_tys_de[i];
            if *is_auto {
                quote! {
                    let mut #table_var = #vec_var.unwrap_or_else(|| <#table_ty>::new());
                }
            } else {
                quote! {
                    let __rows = #vec_var.unwrap_or_default();
                    let mut #table_var = <#table_ty>::new();
                    for __row in __rows {
                        if let ::std::result::Result::Err(e) = #table_var.insert_no_fk(__row) {
                            __errors.push(e);
                        }
                    }
                }
            }
        })
        .collect();

    // FK validation: iterate all rows in each table, check FKs against target tables.
    let fk_checks: Vec<TokenStream> = field_infos
        .iter()
        .enumerate()
        .filter_map(|(i, (_, _, _, _, _, fks, _))| {
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
                        .position(|(fi, _, _, _, _, _, _)| fi == &fk.target_table)
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
        .map(|(i, (field_ident, _, _, _, _, _, _))| {
            let table_var = &table_var_names[i];
            quote! { #field_ident: #table_var }
        })
        .collect();

    // Variable declarations for the visitor. Auto tables use Option<TableType>,
    // non-auto tables use Option<Vec<Row>>.
    let vec_var_decls: Vec<TokenStream> = field_infos
        .iter()
        .enumerate()
        .map(|(i, (_, _, _, _, _, _, is_auto))| {
            let vec_var = &vec_var_names[i];
            if *is_auto {
                let table_ty = &table_tys_de[i];
                quote! {
                    let mut #vec_var: ::std::option::Option<#table_ty> = ::std::option::Option::None;
                }
            } else {
                let row_ty = &row_tys_de[i];
                quote! {
                    let mut #vec_var: ::std::option::Option<::std::vec::Vec<#row_ty>> = ::std::option::Option::None;
                }
            }
        })
        .collect();

    // Extra where bounds for auto tables (their Serialize/Deserialize impls
    // have additional constraints beyond just Row: Serialize/Deserialize).
    let auto_ser_bounds: Vec<TokenStream> = field_infos
        .iter()
        .filter(|(_, _, _, _, _, _, is_auto)| *is_auto)
        .map(|(_, _, table_ty, _, _, _, _)| {
            quote! { #table_ty: ::serde::Serialize, }
        })
        .collect();
    let auto_de_bounds: Vec<TokenStream> = field_infos
        .iter()
        .filter(|(_, _, _, _, _, _, is_auto)| *is_auto)
        .map(|(_, _, table_ty, _, _, _, _)| {
            quote! { #table_ty: ::serde::Deserialize<'de>, }
        })
        .collect();

    let visitor_name = format_ident!("__{}Visitor", db_name);
    let fields_const = format_ident!("__FIELDS_{}", db_name.to_string().to_uppercase());

    // Schema version checking code for the Deserialize visitor.
    let version_check: TokenStream = if let Some(ver) = schema_version {
        quote! {
            if __ver != #ver {
                return ::std::result::Result::Err(::serde::de::Error::custom(
                    ::std::format!(
                        "schema version mismatch: expected {}, found {}",
                        #ver, __ver,
                    ),
                ));
            }
        }
    } else {
        quote! {}
    };

    let version_required_check: TokenStream = if schema_version.is_some() {
        quote! {
            if !__schema_version_seen {
                return ::std::result::Result::Err(
                    ::serde::de::Error::missing_field("schema_version"),
                );
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #[cfg(feature = "serde")]
        impl ::serde::Serialize for #db_name
        where
            #(#row_tys_ser: ::serde::Serialize,)*
            #(#auto_ser_bounds)*
        {
            fn serialize<__S: ::serde::Serializer>(
                &self,
                serializer: __S,
            ) -> ::std::result::Result<__S::Ok, __S::Error> {
                use ::serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(#db_name_str, #ser_field_count)?;
                #serialize_version
                #(#serialize_fields)*
                state.end()
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> ::serde::Deserialize<'de> for #db_name
        where
            #(#row_tys_de: ::serde::Deserialize<'de>,)*
            #(#auto_de_bounds)*
        {
            fn deserialize<__D: ::serde::Deserializer<'de>>(
                deserializer: __D,
            ) -> ::std::result::Result<Self, __D::Error> {
                const #fields_const: &[&str] = &[#(#field_name_strs),*];

                struct #visitor_name;

                impl<'de> ::serde::de::Visitor<'de> for #visitor_name
                where
                    #(#row_tys_de: ::serde::Deserialize<'de>,)*
                    #(#auto_de_bounds)*
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
                        #(#vec_var_decls)*
                        let mut __schema_version_seen: bool = false;

                        while let ::std::option::Option::Some(__key) = map.next_key::<::std::string::String>()? {
                            match __key.as_str() {
                                "schema_version" => {
                                    let __ver = map.next_value::<u64>()?;
                                    __schema_version_seen = true;
                                    #version_check
                                    let _ = __ver;
                                }
                                #(#map_access_arms)*
                                _ => {
                                    let _ = map.next_value::<::serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        #version_required_check

                        let mut __errors: ::std::vec::Vec<::tabulosity::Error> = ::std::vec::Vec::new();

                        // Build tables — missing tables default to empty.
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

/// DFS cycle detection on a directed graph. Returns `Some(cycle_path)` if a
/// cycle is found, `None` otherwise.
fn detect_cycle(graph: &std::collections::BTreeMap<String, Vec<String>>) -> Option<Vec<String>> {
    let mut visited = std::collections::BTreeSet::new();
    let mut on_stack = std::collections::BTreeSet::new();
    let mut path = Vec::new();

    for node in graph.keys() {
        if !visited.contains(node.as_str())
            && let Some(cycle) = dfs_cycle(node, graph, &mut visited, &mut on_stack, &mut path)
        {
            return Some(cycle);
        }
    }
    None
}

fn dfs_cycle<'a>(
    node: &'a str,
    graph: &'a std::collections::BTreeMap<String, Vec<String>>,
    visited: &mut std::collections::BTreeSet<&'a str>,
    on_stack: &mut std::collections::BTreeSet<&'a str>,
    path: &mut Vec<&'a str>,
) -> Option<Vec<String>> {
    visited.insert(node);
    on_stack.insert(node);
    path.push(node);

    if let Some(neighbors) = graph.get(node) {
        for next in neighbors {
            if !visited.contains(next.as_str()) {
                if let Some(cycle) = dfs_cycle(next, graph, visited, on_stack, path) {
                    return Some(cycle);
                }
            } else if on_stack.contains(next.as_str()) {
                // Found a cycle — extract the cycle portion from the path.
                let start = path.iter().position(|&n| n == next.as_str()).unwrap();
                let mut cycle: Vec<String> = path[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(next.clone());
                return Some(cycle);
            }
        }
    }

    path.pop();
    on_stack.remove(node);
    None
}
