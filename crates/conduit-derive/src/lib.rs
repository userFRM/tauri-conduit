#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Derive macros for conduit-core's `Encode` and `Decode` traits.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// Derive the `Encode` trait for a struct with named fields.
///
/// Generates a `conduit_core::Encode` implementation that encodes each
/// field in declaration order by delegating to the field type's own
/// `Encode` impl.
///
/// # Example
///
/// ```rust,ignore
/// use conduit_derive::Encode;
///
/// #[derive(Encode)]
/// struct MarketTick {
///     timestamp: i64,
///     price: f64,
///     volume: f64,
///     side: u8,
/// }
/// ```
#[proc_macro_derive(Encode)]
pub fn derive_wire_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_wire_encode(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive the `Decode` trait for a struct with named fields.
///
/// Generates a `conduit_core::Decode` implementation that decodes each
/// field in declaration order by delegating to the field type's own
/// `Decode` impl, tracking the cumulative byte offset.
///
/// # Example
///
/// ```rust,ignore
/// use conduit_derive::Decode;
///
/// #[derive(Decode)]
/// struct MarketTick {
///     timestamp: i64,
///     price: f64,
///     volume: f64,
///     side: u8,
/// }
/// ```
#[proc_macro_derive(Decode)]
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
                "Encode / Decode can only be derived for structs with named fields",
            )),
        },
        Data::Enum(_) => Err(syn::Error::new_spanned(
            name,
            "Encode / Decode cannot be derived for enums",
        )),
        Data::Union(_) => Err(syn::Error::new_spanned(
            name,
            "Encode / Decode cannot be derived for unions",
        )),
    }
}

/// Reject generic structs with a compile error — wire encoding requires
/// a fixed, concrete layout.
fn reject_generics(input: &DeriveInput) -> syn::Result<()> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Encode / Decode cannot be derived for generic structs",
        ));
    }
    Ok(())
}

/// Generate the `Encode` impl: encodes each named field in declaration
/// order and sums their `encode_size()` for the total.
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
                conduit_core::Encode::encode(&self.#ident, buf);
            }
        })
        .collect();

    let size_terms: Vec<_> = fields
        .named
        .iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            quote! {
                conduit_core::Encode::encode_size(&self.#ident)
            }
        })
        .collect();

    // Handle the zero-field edge case: encode_size returns 0.
    let size_expr = if size_terms.is_empty() {
        quote! { 0 }
    } else {
        let first = &size_terms[0];
        let rest = &size_terms[1..];
        quote! { #first #(+ #rest)* }
    };

    Ok(quote! {
        impl conduit_core::Encode for #name {
            fn encode(&self, buf: &mut Vec<u8>) {
                #(#encode_stmts)*
            }

            fn encode_size(&self) -> usize {
                #size_expr
            }
        }
    })
}

/// Generate the `Decode` impl: decodes each named field in declaration
/// order, tracking the cumulative byte offset through the input slice.
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
                let (#ident, __n) = conduit_core::Decode::decode(&__data[__offset..])?;
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
        impl conduit_core::Decode for #name {
            fn decode(__data: &[u8]) -> Option<(Self, usize)> {
                let mut __offset = 0usize;
                #(#decode_stmts)*
                Some((Self { #(#field_names),* }, __offset))
            }
        }
    })
}
