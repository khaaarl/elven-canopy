//! Implementation of `#[derive(CrcFeed)]`.
//!
//! Generates a `CrcFeed` impl that feeds each field in declaration order
//! into a `Crc32State`. Supports named structs and enums (unit, tuple,
//! and named variants). Enum variants feed their *positional index* as a
//! u32 LE discriminant (0 for the first variant, 1 for the second, etc.),
//! then variant fields in order. This is the declaration-order index, NOT
//! the Rust discriminant — `#[repr(u32)]` and explicit discriminant values
//! are ignored.
//!
//! Single-field tuple structs (newtypes) recurse directly into the inner
//! type without feeding a discriminant.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn derive(input: &DeriveInput) -> TokenStream {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let body = match &input.data {
        Data::Struct(data) => derive_struct_body(&data.fields),
        Data::Enum(data) => derive_enum_body(data),
        Data::Union(_) => {
            return syn::Error::new_spanned(input, "CrcFeed cannot be derived for unions")
                .to_compile_error();
        }
    };

    // Build a where clause that requires CrcFeed on all type params.
    let extra_bounds: Vec<TokenStream> = input
        .generics
        .type_params()
        .map(|tp| {
            let ident = &tp.ident;
            quote! { #ident: ::tabulosity::CrcFeed }
        })
        .collect();

    let combined_where = if extra_bounds.is_empty() {
        quote! { #where_clause }
    } else if let Some(wc) = where_clause {
        let existing = &wc.predicates;
        quote! { where #existing, #(#extra_bounds),* }
    } else {
        quote! { where #(#extra_bounds),* }
    };

    quote! {
        impl #impl_generics ::tabulosity::CrcFeed for #name #ty_generics #combined_where {
            fn crc_feed(&self, state: &mut ::tabulosity::Crc32State) {
                #body
            }
        }
    }
}

fn derive_struct_body(fields: &Fields) -> TokenStream {
    match fields {
        Fields::Named(named) => {
            let feeds: Vec<TokenStream> = named
                .named
                .iter()
                .map(|f| {
                    let ident = f.ident.as_ref().unwrap();
                    quote! { ::tabulosity::CrcFeed::crc_feed(&self.#ident, state); }
                })
                .collect();
            quote! { #(#feeds)* }
        }
        Fields::Unnamed(unnamed) => {
            let feeds: Vec<TokenStream> = unnamed
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let idx = syn::Index::from(i);
                    quote! { ::tabulosity::CrcFeed::crc_feed(&self.#idx, state); }
                })
                .collect();
            quote! { #(#feeds)* }
        }
        Fields::Unit => quote! {},
    }
}

fn derive_enum_body(data: &syn::DataEnum) -> TokenStream {
    let arms: Vec<TokenStream> = data
        .variants
        .iter()
        .enumerate()
        .map(|(disc, variant)| {
            let vident = &variant.ident;
            let disc_u32 = disc as u32;
            match &variant.fields {
                Fields::Unit => {
                    quote! {
                        Self::#vident => {
                            ::tabulosity::CrcFeed::crc_feed(&#disc_u32, state);
                        }
                    }
                }
                Fields::Unnamed(unnamed) => {
                    let bindings: Vec<syn::Ident> = (0..unnamed.unnamed.len())
                        .map(|i| quote::format_ident!("__f{}", i))
                        .collect();
                    let feeds: Vec<TokenStream> = bindings
                        .iter()
                        .map(|b| quote! { ::tabulosity::CrcFeed::crc_feed(#b, state); })
                        .collect();
                    quote! {
                        Self::#vident(#(#bindings),*) => {
                            ::tabulosity::CrcFeed::crc_feed(&#disc_u32, state);
                            #(#feeds)*
                        }
                    }
                }
                Fields::Named(named) => {
                    let field_idents: Vec<&syn::Ident> = named
                        .named
                        .iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();
                    let feeds: Vec<TokenStream> = field_idents
                        .iter()
                        .map(|fi| quote! { ::tabulosity::CrcFeed::crc_feed(#fi, state); })
                        .collect();
                    quote! {
                        Self::#vident { #(#field_idents),* } => {
                            ::tabulosity::CrcFeed::crc_feed(&#disc_u32, state);
                            #(#feeds)*
                        }
                    }
                }
            }
        })
        .collect();

    quote! {
        match self {
            #(#arms)*
        }
    }
}
