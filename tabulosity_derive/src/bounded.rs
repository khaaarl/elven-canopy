//! Implementation of `#[derive(Bounded)]`.
//!
//! Only supports single-field tuple structs (newtypes). Generates a `Bounded`
//! impl that delegates `MIN`/`MAX` to the inner type's constants.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn derive(input: &DeriveInput) -> TokenStream {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let field_ty = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                &fields.unnamed.first().unwrap().ty
            }
            _ => {
                return syn::Error::new_spanned(
                    name,
                    "Bounded can only be derived for single-field tuple structs",
                )
                .to_compile_error();
            }
        },
        _ => {
            return syn::Error::new_spanned(
                name,
                "Bounded can only be derived for single-field tuple structs",
            )
            .to_compile_error();
        }
    };

    quote! {
        impl #impl_generics ::tabulosity::Bounded for #name #ty_generics #where_clause {
            const MIN: Self = #name(<#field_ty as ::tabulosity::Bounded>::MIN);
            const MAX: Self = #name(<#field_ty as ::tabulosity::Bounded>::MAX);
        }

        impl #impl_generics ::tabulosity::AutoIncrementable for #name #ty_generics #where_clause
        where
            #field_ty: ::tabulosity::AutoIncrementable,
        {
            fn first() -> Self {
                #name(<#field_ty as ::tabulosity::AutoIncrementable>::first())
            }
            fn successor(&self) -> Self {
                #name(::tabulosity::AutoIncrementable::successor(&self.0))
            }
        }
    }
}
