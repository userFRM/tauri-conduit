#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Proc macros for conduit: `#[derive(Encode, Decode)]` for binary codecs
//! and `#[command]` for Tauri-style named-parameter handlers.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
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
                ::conduit_core::Encode::encode(&self.#ident, buf);
            }
        })
        .collect();

    let size_terms: Vec<_> = fields
        .named
        .iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            quote! {
                ::conduit_core::Encode::encode_size(&self.#ident)
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
        impl ::conduit_core::Encode for #name {
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
///
/// Also emits a `MIN_SIZE` constant (sum of each field's `MIN_SIZE`) and
/// an upfront bounds check that short-circuits before any per-field work.
fn impl_decode(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    reject_generics(input)?;
    let name = &input.ident;
    let fields = named_fields(input)?;

    let field_types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();

    let decode_stmts: Vec<_> = fields
        .named
        .iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            quote! {
                let #ident = {
                    let (__cdec_v__, __cdec_n__) = ::conduit_core::Decode::decode(&__cdec_src__[__cdec_off__..])?;
                    __cdec_off__ += __cdec_n__;
                    __cdec_v__
                };
            }
        })
        .collect();

    let field_names: Vec<_> = fields
        .named
        .iter()
        .map(|f| f.ident.as_ref().unwrap())
        .collect();

    // Build MIN_SIZE as sum of field MIN_SIZEs
    let min_size_expr = if field_types.is_empty() {
        quote! { 0 }
    } else {
        let tys = &field_types;
        quote! { 0 #(+ <#tys as ::conduit_core::Decode>::MIN_SIZE)* }
    };

    Ok(quote! {
        impl ::conduit_core::Decode for #name {
            const MIN_SIZE: usize = #min_size_expr;

            fn decode(__cdec_src__: &[u8]) -> Option<(Self, usize)> {
                if __cdec_src__.len() < Self::MIN_SIZE {
                    return None;
                }
                let mut __cdec_off__ = 0usize;
                #(#decode_stmts)*
                Some((Self { #(#field_names),* }, __cdec_off__))
            }
        }
    })
}

// ---------------------------------------------------------------------------
// #[command] attribute macro
// ---------------------------------------------------------------------------

/// Attribute macro that transforms a function into a conduit command handler.
///
/// Preserves the original function and generates a hidden handler struct
/// (`__conduit_handler_{fn_name}`) implementing [`conduit_core::ConduitHandler`].
/// Use [`handler!`] to obtain the handler struct for registration.
///
/// This is conduit's 1:1 equivalent of `#[tauri::command]`. The macro supports:
///
/// - **Named parameters** — generates a hidden args struct with
///   `#[derive(Deserialize)]` and `#[serde(rename_all = "camelCase")]`.
///   Rust snake_case parameters are automatically converted to camelCase
///   in JSON, matching `#[tauri::command]` behavior.
/// - **`State<T>` injection** — parameters whose type path ends in `State`
///   are extracted from the context (which must be an `AppHandle<Wry>`).
/// - **`AppHandle` injection** — parameters whose type path ends in `AppHandle`.
/// - **`Window`/`WebviewWindow` injection** — parameters whose type path ends
///   in `Window` or `WebviewWindow`, resolved via `app_handle.get_webview_window(label)`.
/// - **`Webview` injection** — parameters whose type path ends in `Webview`,
///   resolved via `app_handle.get_webview(label)`.
/// - **`Result<T, E>` returns** — errors are converted via `Display` into
///   `conduit_core::Error::Handler`.
/// - **`async` functions** — truly async, spawned on the tokio runtime
///   (not `block_on`).
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
///
/// # Error handling
///
/// When a `Result`-returning handler returns `Err(e)`, the error's
/// `Display` text is sent to the frontend as a JSON error response.
/// This matches `#[tauri::command]` behavior. Be careful about what
/// information your error types expose via `Display`.
///
/// # Limitations
///
/// - **`tauri::Wry` only**: Generated handlers assume `tauri::Wry` as the
///   runtime backend. This is the default (and typically only) runtime in
///   Tauri v2.
/// - **Multiple `State<T>` params**: Each `State<T>` must use a distinct
///   concrete type `T`. Tauri's state system is keyed by `TypeId`, so two
///   params with the same `T` will receive the same instance.
/// - **Name-based injection detection**: `State`, `AppHandle`, `Window`,
///   `WebviewWindow`, and `Webview` are identified by the last path segment
///   of the type. Any user type with these names will be misinterpreted as
///   a Tauri injectable type. Rename your types to avoid false matches.
/// - **Name-based Result detection**: The return type is detected as
///   `Result` by checking the last path segment. Type aliases like
///   `type MyResult<T> = Result<T, E>` are NOT detected as Result returns
///   and will be serialized directly instead of unwrapping `Ok`/`Err`.
/// - **Window/Webview require label**: `Window` and `Webview` injection
///   requires the frontend to send the `X-Conduit-Webview` header (handled
///   automatically by the TS client). If no label is available, the handler
///   returns an error.
/// - **No `impl` block support**: The macro generates struct definitions
///   at the call site, which is illegal inside `impl` blocks. Only use
///   `#[command]` on free-standing functions.
#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[command] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }
    let func = parse_macro_input!(item as ItemFn);
    match impl_conduit_command(func) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Check if a type is `State<...>` by looking at the last path segment.
///
/// **Limitation**: This matches any type whose last path segment is `State`,
/// not just `tauri::State`. If you have a custom type named `State`, rename
/// it to avoid being treated as an injectable Tauri state parameter.
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

/// Check if a type is `AppHandle<...>` by looking at the last path segment.
fn is_app_handle_type(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(type_ref) = ty {
        return is_app_handle_type(&type_ref.elem);
    }
    if let syn::Type::Path(type_path) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            return seg.ident == "AppHandle";
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

/// Check if a type is `Window` or `WebviewWindow` by looking at the last path segment.
///
/// Both `Window` and `WebviewWindow` are treated identically — the generated
/// code calls `app_handle.get_webview_window(label)` which returns a
/// `WebviewWindow` (the unified type in Tauri v2).
fn is_window_type(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(type_ref) = ty {
        return is_window_type(&type_ref.elem);
    }
    if let syn::Type::Path(type_path) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            return seg.ident == "Window" || seg.ident == "WebviewWindow";
        }
    }
    false
}

/// Check if a type is `Webview` by looking at the last path segment.
fn is_webview_type(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(type_ref) = ty {
        return is_webview_type(&type_ref.elem);
    }
    if let syn::Type::Path(type_path) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            return seg.ident == "Webview";
        }
    }
    false
}

/// Check if a type is `Option<...>` by looking at the last path segment.
fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            return seg.ident == "Option";
        }
    }
    false
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
/// Preserves the original function and generates a hidden handler struct
/// (`__conduit_handler_{fn_name}`) implementing [`conduit_core::ConduitHandler`].
/// This mirrors `#[tauri::command]` behavior: the function remains callable
/// directly, and the handler struct is used for registration via
/// `conduit::handler!(fn_name)`.
fn impl_conduit_command(func: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_name = &func.sig.ident;
    let fn_vis = &func.vis;
    let fn_sig = &func.sig;
    let fn_block = &func.block;
    let fn_attrs = &func.attrs;
    let is_async = func.sig.asyncness.is_some();

    if !func.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &func.sig.generics,
            "#[command] cannot be used on generic functions",
        ));
    }

    if func.sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &func.sig.generics.where_clause,
            "#[command] cannot be used on functions with where clauses",
        ));
    }

    for arg in &func.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            if matches!(&*pat_type.ty, syn::Type::ImplTrait(_)) {
                return Err(syn::Error::new_spanned(
                    &pat_type.ty,
                    "#[command] cannot be used with `impl Trait` parameters",
                ));
            }
        }
    }

    // Reject borrowed types on regular (non-State, non-AppHandle) parameters.
    for arg in &func.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            if !is_state_type(&pat_type.ty)
                && !is_app_handle_type(&pat_type.ty)
                && matches!(&*pat_type.ty, syn::Type::Reference(_))
            {
                return Err(syn::Error::new_spanned(
                    &pat_type.ty,
                    "#[command] parameters must be owned types (use String instead of &str)",
                ));
            }
        }
    }

    let handler_struct_name = format_ident!("__conduit_handler_{}", fn_name);

    // Separate State, AppHandle, Window/Webview, and regular params
    let mut state_params: Vec<(&syn::Ident, &syn::Type)> = Vec::new();
    let mut app_handle_params: Vec<(&syn::Ident, &syn::Type)> = Vec::new();
    let mut window_params: Vec<(&syn::Ident, &syn::Type)> = Vec::new();
    let mut webview_params: Vec<(&syn::Ident, &syn::Type)> = Vec::new();
    let mut regular_params: Vec<(&syn::Ident, &syn::Type)> = Vec::new();
    // Track all params in original order for the function call
    let mut all_param_names: Vec<&syn::Ident> = Vec::new();

    for arg in &func.sig.inputs {
        if let FnArg::Receiver(_) = arg {
            return Err(syn::Error::new_spanned(
                arg,
                "#[command] cannot be used on methods with `self`",
            ));
        }
        if let FnArg::Typed(pat_type) = arg {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                if pat_ident.by_ref.is_some() {
                    return Err(syn::Error::new_spanned(
                        &pat_type.pat,
                        "#[command] does not support `ref` parameter bindings",
                    ));
                }
                let param_name = &pat_ident.ident;
                let param_type = &*pat_type.ty;

                all_param_names.push(param_name);

                if is_state_type(param_type) {
                    state_params.push((param_name, param_type));
                } else if is_app_handle_type(param_type) {
                    app_handle_params.push((param_name, param_type));
                } else if is_window_type(param_type) {
                    window_params.push((param_name, param_type));
                } else if is_webview_type(param_type) {
                    webview_params.push((param_name, param_type));
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

    // Generate args struct for regular params
    let has_args = !regular_params.is_empty();
    let struct_name = format_ident!("__conduit_args_{}", fn_name);

    let regular_names: Vec<_> = regular_params.iter().map(|(n, _)| *n).collect();

    let has_state = !state_params.is_empty();
    let has_app_handle = !app_handle_params.is_empty();
    let has_window = !window_params.is_empty();
    let has_webview = !webview_params.is_empty();
    let needs_context = has_state || has_app_handle || has_window || has_webview;

    // Context extraction code (State, AppHandle, Window, Webview injection)
    let state_extraction = if needs_context {
        let state_stmts: Vec<proc_macro2::TokenStream> = state_params
            .iter()
            .map(|(name, ty)| {
                let inner_ty = extract_state_inner_type(ty);
                match inner_ty {
                    Some(inner) => {
                        quote! {
                            let #name: ::tauri::State<'_, #inner> = ::tauri::Manager::state(&*__app);
                        }
                    }
                    None => {
                        // Fallback: use the full type as-is
                        quote! {
                            let #name: #ty = ::tauri::Manager::state(&*__app);
                        }
                    }
                }
            })
            .collect();

        let app_handle_stmts: Vec<proc_macro2::TokenStream> = app_handle_params
            .iter()
            .map(|(name, _ty)| {
                quote! {
                    let #name = __app.clone();
                }
            })
            .collect();

        // Window/WebviewWindow injection: look up by webview label from HandlerContext
        let window_stmts: Vec<proc_macro2::TokenStream> = window_params
            .iter()
            .map(|(name, _ty)| {
                quote! {
                    let #name = {
                        let __label = __handler_ctx.webview_label.as_ref()
                            .ok_or_else(|| ::conduit_core::Error::Handler(
                                "Window injection requires X-Conduit-Webview header".into()
                            ))?;
                        ::tauri::Manager::get_webview_window(&*__app, __label)
                            .ok_or_else(|| ::conduit_core::Error::Handler(
                                ::std::format!("webview window '{}' not found", __label)
                            ))?
                    };
                }
            })
            .collect();

        // Webview injection
        let webview_stmts: Vec<proc_macro2::TokenStream> = webview_params
            .iter()
            .map(|(name, _ty)| {
                quote! {
                    let #name = {
                        let __label = __handler_ctx.webview_label.as_ref()
                            .ok_or_else(|| ::conduit_core::Error::Handler(
                                "Webview injection requires X-Conduit-Webview header".into()
                            ))?;
                        ::tauri::Manager::get_webview(&*__app, __label)
                            .ok_or_else(|| ::conduit_core::Error::Handler(
                                ::std::format!("webview '{}' not found", __label)
                            ))?
                    };
                }
            })
            .collect();

        let context_downcast = quote! {
            let __handler_ctx = __ctx
                .downcast_ref::<::conduit_core::HandlerContext>()
                .ok_or_else(|| ::conduit_core::Error::Handler(
                    "internal: handler context must be HandlerContext".into()
                ))?;
            let __app = __handler_ctx.app_handle
                .downcast_ref::<::tauri::AppHandle<::tauri::Wry>>()
                .ok_or_else(|| ::conduit_core::Error::Handler(
                    "internal: handler context app_handle must be AppHandle<Wry>".into()
                ))?;
        };

        quote! {
            #context_downcast
            #(#state_stmts)*
            #(#app_handle_stmts)*
            #(#window_stmts)*
            #(#webview_stmts)*
        }
    } else {
        quote! {}
    };

    // Args deserialization
    let args_deser = if has_args {
        quote! {
            let #struct_name { #(#regular_names),* } =
                ::conduit_core::sonic_rs::from_slice(&__payload)
                    .map_err(::conduit_core::Error::from)?;
        }
    } else {
        quote! {
            let _ = &__payload;
        }
    };

    // Function call — delegates to the preserved original function
    let fn_call = if is_async {
        quote! { #fn_name(#(#all_param_names),*).await }
    } else {
        quote! { #fn_name(#(#all_param_names),*) }
    };

    // Result handling
    let result_handling = if is_result {
        quote! {
            let __result = #fn_call;
            match __result {
                ::std::result::Result::Ok(__v) => {
                    ::conduit_core::sonic_rs::to_vec(&__v).map_err(::conduit_core::Error::from)
                }
                ::std::result::Result::Err(__e) => {
                    ::std::result::Result::Err(::conduit_core::Error::Handler(__e.to_string()))
                }
            }
        }
    } else {
        quote! {
            let __result = #fn_call;
            ::conduit_core::sonic_rs::to_vec(&__result).map_err(::conduit_core::Error::from)
        }
    };

    // Generate args struct definition (only if has regular params)
    let struct_def = if has_args {
        // Add #[serde(default)] on Option<T> fields so they can be omitted from JSON.
        let field_defs: Vec<proc_macro2::TokenStream> = regular_params
            .iter()
            .map(|(name, ty)| {
                if is_option_type(ty) {
                    quote! { #[serde(default)] #name: #ty }
                } else {
                    quote! { #name: #ty }
                }
            })
            .collect();
        quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            #[derive(::conduit_core::serde::Deserialize)]
            #[serde(crate = "::conduit_core::serde", rename_all = "camelCase")]
            struct #struct_name {
                #(#field_defs),*
            }
        }
    } else {
        quote! {}
    };

    // Generate the handler body — sync wraps in a closure, async in Box::pin
    let handler_body = if is_async {
        quote! {
            ::conduit_core::HandlerResponse::Async(::std::boxed::Box::pin(async move {
                #state_extraction
                #args_deser
                #result_handling
            }))
        }
    } else {
        quote! {
            ::conduit_core::HandlerResponse::Sync((|| -> ::std::result::Result<::std::vec::Vec<u8>, ::conduit_core::Error> {
                #state_extraction
                #args_deser
                #result_handling
            })())
        }
    };

    Ok(quote! {
        #struct_def

        // Preserved original function — callable directly in tests and non-conduit contexts.
        #(#fn_attrs)*
        #fn_vis #fn_sig #fn_block

        // Hidden handler struct for conduit registration.
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #fn_vis struct #handler_struct_name;

        impl ::conduit_core::ConduitHandler for #handler_struct_name {
            fn call(
                &self,
                __payload: ::std::vec::Vec<u8>,
                __ctx: ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>,
            ) -> ::conduit_core::HandlerResponse {
                #handler_body
            }
        }
    })
}

/// Resolve a `#[command]` function name to its generated handler struct.
///
/// Expands `handler!(foo)` to the hidden unit struct `__conduit_handler_foo`
/// that `#[command]` generates alongside the original function. The struct
/// implements [`conduit_core::ConduitHandler`] and is intended for
/// registration with `PluginBuilder::handler`.
///
/// # Requirements
///
/// The target function **must** have `#[command]` applied. If `#[command]`
/// is missing, the compiler will report "cannot find value
/// `__conduit_handler_foo` in this scope".
///
/// # Example
///
/// ```rust,ignore
/// use conduit::{command, handler};
///
/// #[command]
/// fn greet(name: String) -> String {
///     format!("Hello, {name}!")
/// }
///
/// // Register with the plugin builder:
/// tauri_plugin_conduit::init()
///     .handler("greet", handler!(greet))
///     .build()
/// ```
#[proc_macro]
pub fn handler(input: TokenStream) -> TokenStream {
    let mut path = parse_macro_input!(input as syn::Path);
    if let Some(last) = path.segments.last_mut() {
        last.ident = format_ident!("__conduit_handler_{}", last.ident);
    }
    quote! { #path }.into()
}
