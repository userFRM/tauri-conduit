#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Derive macros for conduit-core's `Encode` and `Decode` traits, plus
//! the `#[conduit_command]` attribute macro for Tauri-style named-parameter
//! handlers.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, FnArg, ItemFn, Pat, parse_macro_input};

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
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_encode(&input) {
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
pub fn derive_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_decode(&input) {
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
fn impl_encode(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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
fn impl_decode(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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

// ---------------------------------------------------------------------------
// #[conduit_command] attribute macro
// ---------------------------------------------------------------------------

/// Attribute macro that transforms a function with named parameters into a
/// handler compatible with conduit's `command_json` / `command_json_result`
/// registration methods.
///
/// This provides Tauri-style ergonomics: write a function with named
/// parameters, and the macro generates a hidden args struct with
/// `#[derive(Deserialize)]` so the frontend can send `{ "name": "Alice",
/// "age": 30 }` as a JSON object with named fields.
///
/// # Usage
///
/// ```rust,ignore
/// use conduit_derive::conduit_command;
///
/// // Named parameters — frontend sends { "name": "Alice", "greeting": "Hi" }
/// #[conduit_command]
/// fn greet(name: String, greeting: String) -> String {
///     format!("{greeting}, {name}!")
/// }
///
/// // Result return — errors become conduit::Error::Handler
/// #[conduit_command]
/// fn divide(a: f64, b: f64) -> Result<f64, String> {
///     if b == 0.0 { Err("division by zero".into()) }
///     else { Ok(a / b) }
/// }
///
/// // Register with the plugin builder:
/// tauri_plugin_conduit::init()
///     .command_json("greet", greet)
///     .command_json_result("divide", divide)
///     .build()
/// ```
///
/// For zero-parameter functions, the macro generates a handler that takes
/// `()` (unit), compatible with `command_json("name", handler)` where the
/// frontend sends an empty body or `null`.
#[proc_macro_attribute]
pub fn conduit_command(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    match impl_conduit_command(func) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Convert a snake_case string to PascalCase.
fn pascal_case(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + chars.as_str()
                }
                None => String::new(),
            }
        })
        .collect()
}

/// Implementation of the `#[conduit_command]` attribute macro.
fn impl_conduit_command(func: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_name = &func.sig.ident;
    let fn_vis = &func.vis;
    let fn_output = &func.sig.output;
    let fn_attrs = &func.attrs;
    let fn_stmts = &func.block.stmts;

    // Reject self receivers.
    for arg in &func.sig.inputs {
        if matches!(arg, FnArg::Receiver(_)) {
            return Err(syn::Error::new_spanned(
                arg,
                "#[conduit_command] cannot be used on methods with `self`",
            ));
        }
    }

    // Collect parameter names and types.
    let mut param_names: Vec<&syn::Ident> = Vec::new();
    let mut param_types: Vec<&syn::Type> = Vec::new();

    for arg in &func.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                param_names.push(&pat_ident.ident);
                param_types.push(&*pat_type.ty);
            } else {
                return Err(syn::Error::new_spanned(
                    &pat_type.pat,
                    "#[conduit_command] requires named parameters (e.g. `name: String`)",
                ));
            }
        }
    }

    // Zero parameters: generate fn(()) handler.
    if param_names.is_empty() {
        return Ok(quote! {
            #(#fn_attrs)*
            #fn_vis fn #fn_name(_: ()) #fn_output {
                #(#fn_stmts)*
            }
        });
    }

    // Generate args struct name: __Conduit{FnName}Args.
    let struct_name = syn::Ident::new(
        &format!("__Conduit{}Args", pascal_case(&fn_name.to_string())),
        fn_name.span(),
    );

    Ok(quote! {
        #[doc(hidden)]
        #[derive(conduit_core::serde::Deserialize)]
        #[serde(crate = "conduit_core::serde")]
        #fn_vis struct #struct_name {
            #(#param_names: #param_types),*
        }

        #(#fn_attrs)*
        #fn_vis fn #fn_name(__args: #struct_name) #fn_output {
            let #struct_name { #(#param_names),* } = __args;
            #(#fn_stmts)*
        }
    })
}
