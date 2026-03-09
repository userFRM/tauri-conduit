#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Derive macros for conduit-core's `WireEncode` and `WireDecode` traits.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// Derive the `WireEncode` trait for a struct with named fields.
///
/// Generates a `conduit_core::WireEncode` implementation that encodes each
/// field in declaration order by delegating to the field type's own
/// `WireEncode` impl.
///
/// # Example
///
/// ```rust,ignore
/// use conduit_derive::WireEncode;
///
/// #[derive(WireEncode)]
/// struct MarketTick {
///     timestamp: i64,
///     price: f64,
///     volume: f64,
///     side: u8,
/// }
/// ```
#[proc_macro_derive(WireEncode)]
pub fn derive_wire_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_wire_encode(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive the `WireDecode` trait for a struct with named fields.
///
/// Generates a `conduit_core::WireDecode` implementation that decodes each
/// field in declaration order by delegating to the field type's own
/// `WireDecode` impl, tracking the cumulative byte offset.
///
/// # Example
///
/// ```rust,ignore
/// use conduit_derive::WireDecode;
///
/// #[derive(WireDecode)]
/// struct MarketTick {
///     timestamp: i64,
///     price: f64,
///     volume: f64,
///     side: u8,
/// }
/// ```
#[proc_macro_derive(WireDecode)]
pub fn derive_wire_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_wire_decode(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Extract named fields from a `DeriveInput`, rejecting enums, unions, and
/// tuple/unit structs with a compile error.
fn named_fields(input: &DeriveInput) -> syn::Result<&syn::FieldsNamed> {
    let name = &input.ident;
    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => Ok(named),
            _ => Err(syn::Error::new_spanned(
                name,
                "WireEncode / WireDecode can only be derived for structs with named fields",
            )),
        },
        Data::Enum(_) => Err(syn::Error::new_spanned(
            name,
            "WireEncode / WireDecode cannot be derived for enums",
        )),
        Data::Union(_) => Err(syn::Error::new_spanned(
            name,
            "WireEncode / WireDecode cannot be derived for unions",
        )),
    }
}

fn reject_generics(input: &DeriveInput) -> syn::Result<()> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "WireEncode / WireDecode cannot be derived for generic structs",
        ));
    }
    Ok(())
}

fn impl_wire_encode(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    reject_generics(input)?;
    let name = &input.ident;
    let fields = named_fields(input)?;

    let encode_stmts: Vec<_> = fields
        .named
        .iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            quote! {
                conduit_core::WireEncode::wire_encode(&self.#ident, buf);
            }
        })
        .collect();

    let size_terms: Vec<_> = fields
        .named
        .iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            quote! {
                conduit_core::WireEncode::wire_size(&self.#ident)
            }
        })
        .collect();

    // Handle the zero-field edge case: wire_size returns 0.
    let size_expr = if size_terms.is_empty() {
        quote! { 0 }
    } else {
        let first = &size_terms[0];
        let rest = &size_terms[1..];
        quote! { #first #(+ #rest)* }
    };

    Ok(quote! {
        impl conduit_core::WireEncode for #name {
            fn wire_encode(&self, buf: &mut Vec<u8>) {
                #(#encode_stmts)*
            }

            fn wire_size(&self) -> usize {
                #size_expr
            }
        }
    })
}

fn impl_wire_decode(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    reject_generics(input)?;
    let name = &input.ident;
    let fields = named_fields(input)?;

    let decode_stmts: Vec<_> = fields
        .named
        .iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            quote! {
                let (#ident, __n) = conduit_core::WireDecode::wire_decode(&__data[__offset..])?;
                __offset += __n;
            }
        })
        .collect();

    let field_names: Vec<_> = fields
        .named
        .iter()
        .map(|f| f.ident.as_ref().unwrap())
        .collect();

    Ok(quote! {
        impl conduit_core::WireDecode for #name {
            fn wire_decode(__data: &[u8]) -> Option<(Self, usize)> {
                let mut __offset = 0usize;
                #(#decode_stmts)*
                Some((Self { #(#field_names),* }, __offset))
            }
        }
    })
}
