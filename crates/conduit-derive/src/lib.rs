#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Proc macros for conduit: `#[derive(Encode, Decode)]` for binary codecs
//! and `#[command]` for Tauri-style named-parameter handlers.

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
// #[command] attribute macro
// ---------------------------------------------------------------------------

/// Attribute macro that transforms a function into a context-aware conduit
/// handler with the signature:
///
/// ```text
/// fn name(Vec<u8>, &dyn Any) -> Result<Vec<u8>, conduit_core::Error>
/// ```
///
/// This is conduit's equivalent of `#[tauri::command]`. The macro supports:
///
/// - **Named parameters** — generates a hidden args struct with
///   `#[derive(Deserialize)]`.
/// - **`State<T>` injection** — parameters whose type path ends in `State`
///   are extracted from the context (which must be an `AppHandle<Wry>`).
/// - **`Result<T, E>` returns** — errors are converted via `Display` into
///   `conduit_core::Error::Handler`.
/// - **`async` functions** — wrapped with
///   `tokio::runtime::Handle::current().block_on()`.
///
/// # Examples
///
/// ```rust,ignore
/// use conduit::command;
///
/// // Named parameters — frontend sends { "name": "Alice", "greeting": "Hi" }
/// #[command]
/// fn greet(name: String, greeting: String) -> String {
///     format!("{greeting}, {name}!")
/// }
///
/// // Result return — errors become conduit_core::Error::Handler
/// #[command]
/// fn divide(a: f64, b: f64) -> Result<f64, String> {
///     if b == 0.0 { Err("division by zero".into()) }
///     else { Ok(a / b) }
/// }
///
/// // State injection + async + Result
/// #[command]
/// async fn fetch_user(state: State<'_, Db>, id: u64) -> Result<User, String> {
///     state.get_user(id).await.map_err(|e| e.to_string())
/// }
/// ```
#[proc_macro_attribute]
pub fn command(_attr: TokenStream, item: TokenStream) -> TokenStream {
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

/// Check if a type is `State<...>` by looking at the last path segment.
fn is_state_type(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(type_ref) = ty {
        // Handle &State<...> (reference to State)
        return is_state_type(&type_ref.elem);
    }
    if let syn::Type::Path(type_path) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            return seg.ident == "State";
        }
    }
    false
}

/// Extract the inner type `T` from `State<'_, T>`.
///
/// Returns the second generic argument (skipping the lifetime).
fn extract_state_inner_type(ty: &syn::Type) -> Option<&syn::Type> {
    // Unwrap references first
    let ty = if let syn::Type::Reference(type_ref) = ty {
        &*type_ref.elem
    } else {
        ty
    };

    if let syn::Type::Path(type_path) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            if seg.ident == "State" {
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    // Find the first type argument (skip lifetimes)
                    for arg in &args.args {
                        if let syn::GenericArgument::Type(inner_ty) = arg {
                            return Some(inner_ty);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Check if the return type is `Result<...>`.
fn is_result_return(output: &syn::ReturnType) -> bool {
    match output {
        syn::ReturnType::Default => false,
        syn::ReturnType::Type(_, ty) => {
            if let syn::Type::Path(type_path) = ty.as_ref() {
                if let Some(seg) = type_path.path.segments.last() {
                    return seg.ident == "Result";
                }
            }
            false
        }
    }
}

/// Implementation of the `#[command]` attribute macro.
///
/// Generates a function with signature:
/// `fn name(__payload: Vec<u8>, __ctx: &dyn Any) -> Result<Vec<u8>, conduit_core::Error>`
fn impl_conduit_command(func: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_name = &func.sig.ident;
    let fn_vis = &func.vis;
    let fn_attrs = &func.attrs;
    let fn_stmts = &func.block.stmts;
    let is_async = func.sig.asyncness.is_some();

    // Separate State params from regular params
    let mut state_params: Vec<(&syn::Ident, &syn::Type)> = Vec::new();
    let mut regular_params: Vec<(&syn::Ident, &syn::Type)> = Vec::new();

    for arg in &func.sig.inputs {
        if let FnArg::Receiver(_) = arg {
            return Err(syn::Error::new_spanned(
                arg,
                "#[command] cannot be used on methods with `self`",
            ));
        }
        if let FnArg::Typed(pat_type) = arg {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                let param_name = &pat_ident.ident;
                let param_type = &*pat_type.ty;

                if is_state_type(param_type) {
                    state_params.push((param_name, param_type));
                } else {
                    regular_params.push((param_name, param_type));
                }
            } else {
                return Err(syn::Error::new_spanned(
                    &pat_type.pat,
                    "#[command] requires named parameters",
                ));
            }
        }
    }

    // Detect Result return type
    let is_result = is_result_return(&func.sig.output);

    // Capture the original return type for closure annotation
    let fn_output = &func.sig.output;

    // Generate args struct for regular params
    let has_args = !regular_params.is_empty();
    let struct_name = syn::Ident::new(
        &format!("__Conduit{}Args", pascal_case(&fn_name.to_string())),
        fn_name.span(),
    );

    let regular_names: Vec<_> = regular_params.iter().map(|(n, _)| *n).collect();
    let regular_types: Vec<_> = regular_params.iter().map(|(_, t)| *t).collect();

    let has_state = !state_params.is_empty();

    // State extraction code
    let state_extraction = if has_state {
        let state_stmts: Vec<proc_macro2::TokenStream> = state_params
            .iter()
            .map(|(name, ty)| {
                let inner_ty = extract_state_inner_type(ty);
                match inner_ty {
                    Some(inner) => {
                        quote! {
                            let #name: ::tauri::State<'_, #inner> = ::tauri::Manager::state(__app);
                        }
                    }
                    None => {
                        // Fallback: use the full type as-is
                        quote! {
                            let #name: #ty = ::tauri::Manager::state(__app);
                        }
                    }
                }
            })
            .collect();
        quote! {
            let __app = __ctx
                .downcast_ref::<::tauri::AppHandle<::tauri::Wry>>()
                .ok_or_else(|| ::conduit_core::Error::Handler(
                    "internal: handler context must be AppHandle<Wry>".into()
                ))?;
            #(#state_stmts)*
        }
    } else {
        quote! {}
    };

    // Args deserialization
    let args_deser = if has_args {
        quote! {
            let #struct_name { #(#regular_names),* } =
                ::sonic_rs::from_slice(&__payload)
                    .map_err(::conduit_core::Error::from)?;
        }
    } else {
        quote! {
            // Accept empty body or null — no deserialization needed.
            let _ = &__payload;
        }
    };

    // Body execution (async vs sync)
    let body_exec = if is_async {
        quote! {
            ::tokio::runtime::Handle::current().block_on(async move {
                #(#fn_stmts)*
            })
        }
    } else {
        quote! {
            (|| #fn_output { #(#fn_stmts)* })()
        }
    };

    // Result handling
    let result_handling = if is_result {
        quote! {
            let __result = #body_exec;
            match __result {
                ::std::result::Result::Ok(__v) => {
                    ::sonic_rs::to_vec(&__v).map_err(::conduit_core::Error::from)
                }
                ::std::result::Result::Err(__e) => {
                    ::std::result::Result::Err(::conduit_core::Error::Handler(__e.to_string()))
                }
            }
        }
    } else {
        quote! {
            let __result = #body_exec;
            ::sonic_rs::to_vec(&__result).map_err(::conduit_core::Error::from)
        }
    };

    // Generate struct definition (only if has args)
    let struct_def = if has_args {
        quote! {
            #[doc(hidden)]
            #[derive(::conduit_core::serde::Deserialize)]
            #[serde(crate = "conduit_core::serde")]
            #fn_vis struct #struct_name {
                #(#regular_names: #regular_types),*
            }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #struct_def

        #(#fn_attrs)*
        #fn_vis fn #fn_name(
            __payload: ::std::vec::Vec<u8>,
            __ctx: &dyn ::std::any::Any,
        ) -> ::std::result::Result<::std::vec::Vec<u8>, ::conduit_core::Error> {
            #state_extraction
            #args_deser
            #result_handling
        }
    })
}
