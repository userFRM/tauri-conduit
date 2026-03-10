#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! # tauri-plugin-conduit
//!
//! Tauri v2 plugin for conduit — binary IPC over the `conduit://` custom
//! protocol.
//!
//! Registers a `conduit://` custom protocol for zero-overhead in-process
//! binary dispatch. Supports both sync and async handlers via
//! [`ConduitHandler`](conduit_core::ConduitHandler). No network surface.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use conduit::{command, handler};
//!
//! #[command]
//! fn greet(name: String) -> String {
//!     format!("Hello, {name}!")
//! }
//!
//! #[command]
//! async fn fetch_user(state: State<'_, Db>, id: u64) -> Result<User, String> {
//!     state.get_user(id).await.map_err(|e| e.to_string())
//! }
//!
//! tauri::Builder::default()
//!     .plugin(
//!         tauri_plugin_conduit::init()
//!             .handler("greet", handler!(greet))
//!             .handler("fetch_user", handler!(fetch_user))
//!             .channel("telemetry")
//!             .build()
//!     )
//!     .run(tauri::generate_context!())
//!     .unwrap();
//! ```

/// Re-export the `#[command]` attribute macro from `conduit-derive`.
///
/// This is conduit's equivalent of `#[tauri::command]`. Use it for
/// named-parameter handlers:
///
/// ```rust,ignore
/// use conduit::{command, handler};
///
/// #[command]
/// fn greet(name: String, greeting: String) -> String {
///     format!("{greeting}, {name}!")
/// }
/// ```
pub use conduit_derive::command;

/// Re-export the `handler!()` macro from `conduit-derive`.
///
/// Resolves a `#[command]` function name to its conduit handler struct
/// for registration:
///
/// ```rust,ignore
/// tauri_plugin_conduit::init()
///     .handler("greet", handler!(greet))
///     .build()
/// ```
pub use conduit_derive::handler;

use std::collections::HashMap;
use std::sync::Arc;

use conduit_core::{
    ChannelBuffer, ConduitHandler, Decode, Encode, HandlerResponse, Queue, RingBuffer, Router,
};
use subtle::ConstantTimeEq;
use tauri::plugin::{Builder as TauriPluginBuilder, TauriPlugin};
use tauri::{AppHandle, Emitter, Manager, Runtime};

// ---------------------------------------------------------------------------
// Helper: safe HTTP response builder
// ---------------------------------------------------------------------------

/// Build an HTTP response, falling back to a minimal 500 if construction fails.
fn make_response(status: u16, content_type: &str, body: Vec<u8>) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(body)
        .unwrap_or_else(|_| {
            http::Response::builder()
                .status(500)
                .body(b"internal error".to_vec())
                .expect("fallback response must not fail")
        })
}

/// Build a JSON error response: `{"error": "message"}`.
///
/// Uses `sonic_rs` for proper RFC 8259 escaping of all control characters,
/// newlines, quotes, and backslashes — not just `\` and `"`.
fn make_error_response(status: u16, message: &str) -> http::Response<Vec<u8>> {
    #[derive(serde::Serialize)]
    struct ErrorBody<'a> {
        error: &'a str,
    }
    let body = conduit_core::sonic_rs::to_vec(&ErrorBody { error: message })
        .unwrap_or_else(|_| br#"{"error":"internal error"}"#.to_vec());
    make_response(status, "application/json", body)
}

// ---------------------------------------------------------------------------
// BootstrapInfo — returned to JS via `conduit_bootstrap` command
// ---------------------------------------------------------------------------

/// Connection info returned to the frontend during bootstrap.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapInfo {
    /// Protocol version (currently `1`). Allows the TS client to verify
    /// protocol compatibility.
    #[serde(default = "default_protocol_version")]
    pub protocol_version: u8,
    /// Base URL for the custom protocol (e.g., `"conduit://localhost"`).
    pub protocol_base: String,
    /// Per-launch invoke key for custom protocol authentication (hex-encoded).
    ///
    /// **Security**: This key authenticates custom protocol requests. It is
    /// generated fresh each launch from 32 bytes of OS randomness and validated
    /// using constant-time comparison. The JS client includes it as the
    /// `X-Conduit-Key` header on every `conduit://` request.
    pub invoke_key: String,
    /// Available channel names.
    pub channels: Vec<String>,
}

fn default_protocol_version() -> u8 {
    1
}

impl std::fmt::Debug for BootstrapInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapInfo")
            .field("protocol_version", &self.protocol_version)
            .field("protocol_base", &self.protocol_base)
            .field("invoke_key", &"[REDACTED]")
            .field("channels", &self.channels)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// PluginState — managed Tauri state
// ---------------------------------------------------------------------------

/// Shared state for the conduit Tauri plugin.
///
/// Holds the router, named streaming channels, the per-launch invoke key,
/// and the app handle for emitting push notifications.
pub struct PluginState<R: Runtime> {
    dispatch: Arc<Router>,
    /// `#[command]`-generated handlers (sync and async via [`ConduitHandler`]).
    handlers: Arc<HashMap<String, Arc<dyn ConduitHandler>>>,
    /// Named channels for server→client streaming (lossy or ordered).
    channels: HashMap<String, Arc<ChannelBuffer>>,
    /// Tauri app handle for emitting events to the frontend.
    app_handle: AppHandle<R>,
    /// Per-launch invoke key (hex-encoded, 64 hex chars = 32 bytes).
    invoke_key: String,
    /// Raw invoke key bytes for constant-time comparison.
    invoke_key_bytes: [u8; 32],
}

impl<R: Runtime> std::fmt::Debug for PluginState<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginState")
            .field("channels", &self.channels.keys().collect::<Vec<_>>())
            .field("invoke_key", &"[REDACTED]")
            .finish()
    }
}

impl<R: Runtime> PluginState<R> {
    /// Get a channel by name (for pushing data from Rust handlers).
    pub fn channel(&self, name: &str) -> Option<&Arc<ChannelBuffer>> {
        self.channels.get(name)
    }

    /// Push binary data to a named channel and notify JS listeners.
    ///
    /// After writing to the channel, emits both a global
    /// `conduit:data-available` event (payload = channel name) and a
    /// per-channel `conduit:data-available:{channel}` event. JS subscribers
    /// can listen on either.
    ///
    /// For lossy channels, oldest frames are silently dropped when the buffer
    /// is full. For reliable channels, returns an error if the buffer is full
    /// (backpressure).
    ///
    /// Returns an error string if the named channel was not registered via
    /// the builder or if a reliable channel is full.
    pub fn push(&self, channel: &str, data: &[u8]) -> Result<(), String> {
        let ch = self
            .channels
            .get(channel)
            .ok_or_else(|| format!("unknown channel: {channel}"))?;
        ch.push(data).map(|_| ()).map_err(|e| e.to_string())?;
        // Emit global event (backward-compatible with old JS code).
        if self
            .app_handle
            .emit("conduit:data-available", channel)
            .is_err()
        {
            #[cfg(debug_assertions)]
            eprintln!(
                "conduit: failed to emit global data-available event for channel '{channel}'"
            );
        }
        // Emit per-channel event.
        if self
            .app_handle
            .emit(&format!("conduit:data-available:{channel}"), channel)
            .is_err()
        {
            #[cfg(debug_assertions)]
            eprintln!(
                "conduit: failed to emit per-channel data-available event for channel '{channel}'"
            );
        }
        Ok(())
    }

    /// Return the list of registered channel names.
    pub fn channel_names(&self) -> Vec<String> {
        self.channels.keys().cloned().collect()
    }

    /// Validate an invoke key candidate using constant-time operations.
    fn validate_invoke_key(&self, candidate: &str) -> bool {
        validate_invoke_key_ct(&self.invoke_key_bytes, candidate)
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Return bootstrap info so the JS client knows how to reach the conduit
/// custom protocol.
///
/// May be called multiple times (e.g., after page reloads during development).
/// The invoke key is generated once at plugin setup and remains constant for
/// the lifetime of the app process. Repeated calls return the same key.
#[tauri::command]
fn conduit_bootstrap(
    state: tauri::State<'_, PluginState<tauri::Wry>>,
) -> Result<BootstrapInfo, String> {
    Ok(BootstrapInfo {
        protocol_version: 1,
        protocol_base: "conduit://localhost".to_string(),
        invoke_key: state.invoke_key.clone(),
        channels: state.channel_names(),
    })
}

/// Validate channel names and return those that exist.
///
/// This is a validation-only endpoint — no server-side subscription state is
/// tracked. The JS client uses the returned list to know which channels are
/// available. Actual data delivery happens via `conduit:data-available` events
/// and `conduit://localhost/drain/<channel>` protocol requests.
///
/// Unknown channel names are silently filtered out — only channels that
/// exist are returned.
#[tauri::command]
fn conduit_subscribe(
    state: tauri::State<'_, PluginState<tauri::Wry>>,
    channels: Vec<String>,
) -> Result<Vec<String>, String> {
    // Silently filter to only channels that exist.
    let valid: Vec<String> = channels
        .into_iter()
        .filter(|c| state.channels.contains_key(c.as_str()))
        .collect();
    Ok(valid)
}

// ---------------------------------------------------------------------------
// Channel kind (internal)
// ---------------------------------------------------------------------------

/// Internal enum for deferred channel construction.
enum ChannelKind {
    /// Lossy ring buffer with the given byte capacity.
    Lossy(usize),
    /// Reliable queue with the given max byte limit.
    Reliable(usize),
}

// ---------------------------------------------------------------------------
// Plugin builder
// ---------------------------------------------------------------------------

/// A deferred command registration closure.
type CommandRegistration = Box<dyn FnOnce(&Router) + Send>;

/// Builder for the conduit Tauri v2 plugin.
///
/// Collects command registrations and configuration, then produces a
/// [`TauriPlugin`] via [`build`](Self::build).
pub struct PluginBuilder {
    /// Deferred command registrations: (name, handler factory).
    commands: Vec<CommandRegistration>,
    /// `#[command]`-generated handlers (sync and async).
    handler_defs: Vec<(String, Arc<dyn ConduitHandler>)>,
    /// Named channels: (name, kind).
    channel_defs: Vec<(String, ChannelKind)>,
}

impl std::fmt::Debug for PluginBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginBuilder")
            .field("commands", &self.commands.len())
            .field("handlers", &self.handler_defs.len())
            .field("channel_defs_count", &self.channel_defs.len())
            .finish()
    }
}

/// Validate that a channel name matches `[a-zA-Z0-9_-]+`.
fn validate_channel_name(name: &str) {
    assert!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "conduit: invalid channel name '{}' — must match [a-zA-Z0-9_-]+",
        name
    );
}

/// Default channel capacity (64 KB).
const DEFAULT_CHANNEL_CAPACITY: usize = 64 * 1024;

impl PluginBuilder {
    /// Panic if a channel with the given name is already registered.
    fn assert_no_duplicate_channel(&self, name: &str) {
        if self.channel_defs.iter().any(|(n, _)| n == name) {
            panic!(
                "conduit: duplicate channel name '{}' — each channel must have a unique name",
                name
            );
        }
    }

    /// Create a new, empty plugin builder.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            handler_defs: Vec::new(),
            channel_defs: Vec::new(),
        }
    }

    // -- Raw handlers -------------------------------------------------------

    /// Register a raw command handler (`Vec<u8>` in, `Vec<u8>` out).
    ///
    /// Command names correspond to the path segment in the
    /// `conduit://localhost/invoke/<cmd_name>` URL.
    pub fn command<F>(mut self, name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static,
    {
        let name = name.into();
        self.commands.push(Box::new(move |table: &Router| {
            table.register(name, handler);
        }));
        self
    }

    // -- ConduitHandler-based (#[command]-generated, sync or async) ----------

    /// Register a `#[conduit::command]`-generated handler.
    ///
    /// Works with both sync and async handlers. Sync handlers are dispatched
    /// inline. Async handlers are spawned on the tokio runtime — truly async,
    /// exactly like `#[tauri::command]`.
    ///
    /// ```rust,ignore
    /// use conduit::{command, handler};
    ///
    /// #[command]
    /// fn greet(name: String) -> String {
    ///     format!("Hello, {name}!")
    /// }
    ///
    /// #[command]
    /// async fn fetch_user(state: State<'_, Db>, id: u64) -> Result<User, String> {
    ///     state.get_user(id).await.map_err(|e| e.to_string())
    /// }
    ///
    /// tauri_plugin_conduit::init()
    ///     .handler("greet", handler!(greet))
    ///     .handler("fetch_user", handler!(fetch_user))
    ///     .build()
    /// ```
    pub fn handler(mut self, name: impl Into<String>, handler: impl ConduitHandler) -> Self {
        self.handler_defs.push((name.into(), Arc::new(handler)));
        self
    }

    /// Register a raw closure handler (legacy API).
    ///
    /// Accepts the same closure signature as the pre-`ConduitHandler` `.handler()`:
    /// `Fn(Vec<u8>, &dyn Any) -> Result<Vec<u8>, Error>`. This is a synchronous
    /// handler dispatched via `Router::register_with_context`.
    ///
    /// Use this for backward compatibility when migrating from closure-based
    /// registration. For new code, prefer [`handler`](Self::handler) with
    /// `#[conduit::command]` + `handler!()`.
    pub fn handler_raw<F>(mut self, name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(Vec<u8>, &dyn std::any::Any) -> Result<Vec<u8>, conduit_core::Error>
            + Send
            + Sync
            + 'static,
    {
        let name = name.into();
        self.commands.push(Box::new(move |table: &Router| {
            table.register_with_context(name, handler);
        }));
        self
    }

    // -- JSON handlers (Level 1) --------------------------------------------

    /// Typed JSON handler. Deserializes the request payload as `A` and
    /// serializes the response as `R`.
    ///
    /// Unlike Tauri's `#[tauri::command]`, this takes a single argument type
    /// (not named parameters) and does not support async or State injection.
    ///
    /// ```rust,ignore
    /// .command_json("greet", |name: String| format!("Hello, {name}!"))
    /// ```
    pub fn command_json<F, A, R>(mut self, name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(A) -> R + Send + Sync + 'static,
        A: serde::de::DeserializeOwned + 'static,
        R: serde::Serialize + 'static,
    {
        let name = name.into();
        self.commands.push(Box::new(move |table: &Router| {
            table.register_json(name, handler);
        }));
        self
    }

    /// Typed JSON handler that returns `Result<R, E>`.
    ///
    /// Like [`command_json`](Self::command_json), but the handler returns
    /// `Result<R, E>` where `E: Display`. On success, `R` is serialized to
    /// JSON. On error, the error's `Display` text is returned to the caller.
    ///
    /// For Tauri-style named parameters with `Result` returns, prefer
    /// [`handler`](Self::handler) with `#[conduit::command]` instead:
    ///
    /// ```rust,ignore
    /// use conduit::command;
    ///
    /// #[command]
    /// fn divide(a: f64, b: f64) -> Result<f64, String> {
    ///     if b == 0.0 { Err("division by zero".into()) }
    ///     else { Ok(a / b) }
    /// }
    ///
    /// // Preferred:
    /// .handler("divide", divide)
    /// ```
    pub fn command_json_result<F, A, R, E>(mut self, name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(A) -> Result<R, E> + Send + Sync + 'static,
        A: serde::de::DeserializeOwned + 'static,
        R: serde::Serialize + 'static,
        E: std::fmt::Display + 'static,
    {
        let name = name.into();
        self.commands.push(Box::new(move |table: &Router| {
            table.register_json_result(name, handler);
        }));
        self
    }

    // -- Binary handlers (Level 2) ------------------------------------------

    /// Register a typed binary command handler.
    ///
    /// The request payload is decoded via the [`Decode`] trait and the response
    /// is encoded via [`Encode`]. No JSON involved — raw bytes in, raw bytes
    /// out.
    ///
    /// ```rust,ignore
    /// .command_binary("process", |tick: MarketTick| tick)
    /// ```
    pub fn command_binary<F, A, Ret>(mut self, name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(A) -> Ret + Send + Sync + 'static,
        A: Decode + 'static,
        Ret: Encode + 'static,
    {
        let name = name.into();
        self.commands.push(Box::new(move |table: &Router| {
            table.register_binary(name, handler);
        }));
        self
    }

    // -- Lossy channels (default) -------------------------------------------

    /// Register a lossy channel with the default capacity (64 KB).
    ///
    /// Oldest frames are silently dropped when the buffer is full. Best for
    /// telemetry, game state, and real-time data where freshness matters more
    /// than completeness.
    ///
    /// # Panics
    ///
    /// Panics if the name is empty, contains characters outside `[a-zA-Z0-9_-]`,
    /// or duplicates an already-registered channel name.
    pub fn channel(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        validate_channel_name(&name);
        self.assert_no_duplicate_channel(&name);
        self.channel_defs
            .push((name, ChannelKind::Lossy(DEFAULT_CHANNEL_CAPACITY)));
        self
    }

    /// Register a lossy channel with a custom byte capacity.
    ///
    /// # Panics
    ///
    /// Panics if the name is empty, contains characters outside `[a-zA-Z0-9_-]`,
    /// or duplicates an already-registered channel name.
    pub fn channel_with_capacity(mut self, name: impl Into<String>, capacity: usize) -> Self {
        let name = name.into();
        validate_channel_name(&name);
        self.assert_no_duplicate_channel(&name);
        self.channel_defs.push((name, ChannelKind::Lossy(capacity)));
        self
    }

    // -- Reliable channels (guaranteed delivery) ----------------------------

    /// Register an ordered channel with the default capacity (64 KB).
    ///
    /// No frames are ever dropped. When the buffer is full,
    /// [`PluginState::push`] returns an error (backpressure). Best for
    /// transaction logs, control messages, and any data that must arrive
    /// intact and in order.
    ///
    /// # Panics
    ///
    /// Panics if the name is empty, contains characters outside `[a-zA-Z0-9_-]`,
    /// or duplicates an already-registered channel name.
    pub fn channel_ordered(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        validate_channel_name(&name);
        self.assert_no_duplicate_channel(&name);
        self.channel_defs
            .push((name, ChannelKind::Reliable(DEFAULT_CHANNEL_CAPACITY)));
        self
    }

    /// Register an ordered channel with a custom byte limit.
    ///
    /// A `max_bytes` of `0` means unbounded — the buffer grows without limit.
    ///
    /// # Panics
    ///
    /// Panics if the name is empty, contains characters outside `[a-zA-Z0-9_-]`,
    /// or duplicates an already-registered channel name.
    pub fn channel_ordered_with_capacity(
        mut self,
        name: impl Into<String>,
        max_bytes: usize,
    ) -> Self {
        let name = name.into();
        validate_channel_name(&name);
        self.assert_no_duplicate_channel(&name);
        self.channel_defs
            .push((name, ChannelKind::Reliable(max_bytes)));
        self
    }

    // -- Build --------------------------------------------------------------

    /// Build the Tauri v2 plugin.
    ///
    /// This consumes the builder and returns a [`TauriPlugin`] that can be
    /// passed to `tauri::Builder::plugin`.
    ///
    /// # Dispatch model
    ///
    /// Commands are dispatched through a two-tier system:
    ///
    /// 1. **`#[command]` handlers** (registered via [`.handler()`](Self::handler))
    ///    are checked first. These support named parameters, `State<T>` injection,
    ///    `Result` returns, and async — full parity with `#[tauri::command]`.
    ///
    /// 2. **Raw Router handlers** (registered via [`.command()`](Self::command),
    ///    [`.command_json()`](Self::command_json), [`.command_binary()`](Self::command_binary))
    ///    are the fallback. These are simpler `Vec<u8> -> Vec<u8>` functions
    ///    with no injection or async support.
    ///
    /// If a command name exists in both tiers, the `#[command]` handler takes
    /// priority and a debug warning is printed.
    pub fn build<R: Runtime>(self) -> TauriPlugin<R> {
        let commands = self.commands;
        let handler_defs = self.handler_defs;
        let channel_defs = self.channel_defs;

        TauriPluginBuilder::<R>::new("conduit")
            // --- Custom protocol: conduit://localhost/invoke/<cmd> ---
            // Uses the asynchronous variant so async #[command] handlers
            // are spawned on tokio (truly async, like #[tauri::command]).
            .register_asynchronous_uri_scheme_protocol("conduit", move |ctx, request, responder| {
                // Extract the managed PluginState from the app handle.
                let state: tauri::State<'_, PluginState<R>> = ctx.app_handle().state();

                let url = request.uri().to_string();

                // Extract path from URL: conduit://localhost/{action}/{target}
                // Use simple string splitting instead of full URL parsing —
                // the format is fixed and under our control.
                let path = url
                    .find("://")
                    .and_then(|i| url[i + 3..].find('/'))
                    .map(|i| {
                        let host_end = url.find("://").unwrap() + 3;
                        &url[host_end + i..]
                    })
                    .unwrap_or("/");
                let segments: Vec<&str> = path.trim_start_matches('/').splitn(2, '/').collect();

                if segments.len() != 2 {
                    responder.respond(make_error_response(
                        404,
                        "not found: expected /invoke/<cmd> or /drain/<channel>",
                    ));
                    return;
                }

                // Validate the invoke key from the X-Conduit-Key header.
                let key = match request.headers().get("X-Conduit-Key") {
                    Some(v) => match v.to_str() {
                        Ok(s) => s.to_string(),
                        Err(_) => {
                            responder
                                .respond(make_error_response(401, "invalid invoke key header"));
                            return;
                        }
                    },
                    None => {
                        responder.respond(make_error_response(401, "missing invoke key"));
                        return;
                    }
                };

                if !state.validate_invoke_key(&key) {
                    responder.respond(make_error_response(403, "invalid invoke key"));
                    return;
                }

                let action = segments[0];
                let raw_target = segments[1];

                // H6: Percent-decode the target and reject path traversal.
                let target = percent_decode(raw_target);
                if target.contains('/') {
                    responder.respond(make_error_response(400, "invalid command name"));
                    return;
                }

                match action {
                    "invoke" => {
                        let body = request.body().to_vec();

                        // 1) Check #[command]-generated handlers first (sync or async)
                        if let Some(handler) = state.handlers.get(&target) {
                            let handler = Arc::clone(handler);
                            let app_handle = ctx.app_handle().clone();
                            // Extract webview label from X-Conduit-Webview header (sent by JS client).
                            // NOTE: This header is client-provided and could be spoofed by JS
                            // running in the same webview. We validate the format to prevent
                            // injection attacks, but in a multi-webview app, code in one
                            // webview could impersonate another. This matches Tauri's own
                            // trust model where all JS in the webview is equally trusted.
                            let webview_label = request
                                .headers()
                                .get("X-Conduit-Webview")
                                .and_then(|v| v.to_str().ok())
                                .filter(|s| {
                                    !s.is_empty()
                                        && s.len() <= 128
                                        && s.bytes().all(|b| {
                                            b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
                                        })
                                })
                                .map(|s| s.to_string());
                            let handler_ctx = conduit_core::HandlerContext::new(
                                Arc::new(app_handle),
                                webview_label,
                            );
                            let ctx_any: Arc<dyn std::any::Any + Send + Sync> =
                                Arc::new(handler_ctx);

                            // SAFETY: AssertUnwindSafe is used here because:
                            // - `body` is a Vec<u8> (unwind-safe by itself)
                            // - `ctx_any` is an Arc (unwind-safe)
                            // - conduit's own locks use poison-recovery helpers (lock_or_recover)
                            // - User-defined handler state may be left inconsistent after panic,
                            //   but this is inherent to catch_unwind and documented as a limitation.
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    handler.call(body, ctx_any)
                                }));

                            match result {
                                Ok(HandlerResponse::Sync(Ok(bytes))) => {
                                    responder.respond(make_response(
                                        200,
                                        "application/octet-stream",
                                        bytes,
                                    ));
                                }
                                Ok(HandlerResponse::Sync(Err(e))) => {
                                    let status = error_to_status(&e);
                                    responder
                                        .respond(make_error_response(status, &sanitize_error(&e)));
                                }
                                Ok(HandlerResponse::Async(future)) => {
                                    // Truly async — spawned on tokio, just like #[tauri::command].
                                    // Inner spawn provides panic isolation: if the future panics
                                    // during execution, the JoinHandle catches it and we respond
                                    // with a 500 instead of leaving the request hanging.
                                    tauri::async_runtime::spawn(async move {
                                        let handle = tauri::async_runtime::spawn(future);
                                        match handle.await {
                                            Ok(Ok(bytes)) => {
                                                responder.respond(make_response(
                                                    200,
                                                    "application/octet-stream",
                                                    bytes,
                                                ));
                                            }
                                            Ok(Err(e)) => {
                                                let status = error_to_status(&e);
                                                responder.respond(make_error_response(
                                                    status,
                                                    &sanitize_error(&e),
                                                ));
                                            }
                                            Err(_) => {
                                                // Panic during async handler execution
                                                responder.respond(make_error_response(
                                                    500,
                                                    "handler panicked",
                                                ));
                                            }
                                        }
                                    });
                                }
                                Err(_) => {
                                    // Panic caught by catch_unwind — keep as 500.
                                    responder.respond(make_error_response(500, "handler panicked"));
                                }
                            }
                        } else {
                            // 2) Fall back to legacy sync Router
                            let dispatch = Arc::clone(&state.dispatch);
                            let app_handle = ctx.app_handle().clone();
                            // SAFETY: AssertUnwindSafe is used here because:
                            // - `body` is a Vec<u8> (unwind-safe by itself)
                            // - `dispatch` is an Arc<Router> (unwind-safe)
                            // - `app_handle` is a cloned AppHandle (unwind-safe)
                            // - conduit's own locks use poison-recovery helpers (lock_or_recover)
                            // - User-defined handler state may be left inconsistent after panic,
                            //   but this is inherent to catch_unwind and documented as a limitation.
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    dispatch.call_with_context(&target, body, &app_handle)
                                }));
                            match result {
                                Ok(Ok(bytes)) => {
                                    responder.respond(make_response(
                                        200,
                                        "application/octet-stream",
                                        bytes,
                                    ));
                                }
                                Ok(Err(e)) => {
                                    let status = error_to_status(&e);
                                    responder
                                        .respond(make_error_response(status, &sanitize_error(&e)));
                                }
                                Err(_) => {
                                    // Panic caught by catch_unwind — keep as 500.
                                    responder.respond(make_error_response(500, "handler panicked"));
                                }
                            }
                        }
                    }
                    "drain" => match state.channel(&target) {
                        Some(ch) => {
                            let blob = ch.drain_all();
                            responder.respond(make_response(200, "application/octet-stream", blob));
                        }
                        None => {
                            responder.respond(make_error_response(
                                404,
                                &format!("unknown channel: {}", sanitize_name(&target)),
                            ));
                        }
                    },
                    _ => {
                        responder.respond(make_error_response(
                            404,
                            "not found: expected /invoke/<cmd> or /drain/<channel>",
                        ));
                    }
                }
            })
            // --- Register Tauri IPC commands ---
            .invoke_handler(tauri::generate_handler![
                conduit_bootstrap,
                conduit_subscribe,
            ])
            // --- Plugin setup: create state, register commands ---
            .setup(move |app, _api| {
                let dispatch = Arc::new(Router::new());

                // Register all old-style commands that were added via the builder.
                for register_fn in commands {
                    register_fn(&dispatch);
                }

                // Build the #[command] handler map, checking for collisions
                // with Router commands.
                let mut handler_map = HashMap::new();
                for (name, handler) in handler_defs {
                    if dispatch.has(&name) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "conduit: warning: handler '{name}' shadows a Router command \
                             with the same name — the #[command] handler takes priority"
                        );
                    }
                    handler_map.insert(name, handler);
                }
                let handlers = Arc::new(handler_map);

                // Create named channels.
                let mut channels = HashMap::new();
                for (name, kind) in channel_defs {
                    let buf = match kind {
                        ChannelKind::Lossy(cap) => ChannelBuffer::Lossy(RingBuffer::new(cap)),
                        ChannelKind::Reliable(max_bytes) => {
                            ChannelBuffer::Reliable(Queue::new(max_bytes))
                        }
                    };
                    channels.insert(name, Arc::new(buf));
                }

                // Generate the per-launch invoke key.
                let invoke_key_bytes = generate_invoke_key_bytes();
                let invoke_key = hex_encode(&invoke_key_bytes);

                // Obtain the app handle for emitting events.
                let app_handle = app.app_handle().clone();

                let state = PluginState {
                    dispatch,
                    handlers,
                    channels,
                    app_handle,
                    invoke_key,
                    invoke_key_bytes,
                };

                app.manage(state);

                Ok(())
            })
            .build()
    }
}

impl Default for PluginBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Public init function
// ---------------------------------------------------------------------------

/// Create a new conduit plugin builder.
///
/// This is the main entry point for using the conduit Tauri plugin:
///
/// ```rust,ignore
/// use conduit::command;
///
/// #[command]
/// fn greet(name: String) -> String {
///     format!("Hello, {name}!")
/// }
///
/// #[command]
/// async fn fetch_data(url: String) -> Result<Vec<u8>, String> {
///     reqwest::get(&url).await.map_err(|e| e.to_string())?
///         .bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
/// }
///
/// tauri::Builder::default()
///     .plugin(
///         tauri_plugin_conduit::init()
///             .handler("greet", handler!(greet))
///             .handler("fetch_data", handler!(fetch_data))
///             .channel("telemetry")
///             .build()
///     )
///     .run(tauri::generate_context!())
///     .unwrap();
/// ```
pub fn init() -> PluginBuilder {
    PluginBuilder::new()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a [`conduit_core::Error`] to the appropriate HTTP status code.
fn error_to_status(e: &conduit_core::Error) -> u16 {
    match e {
        conduit_core::Error::UnknownCommand(_) => 404,
        conduit_core::Error::UnknownChannel(_) => 404,
        conduit_core::Error::AuthFailed => 403,
        conduit_core::Error::DecodeFailed => 400,
        conduit_core::Error::PayloadTooLarge(_) => 413,
        conduit_core::Error::Handler(_) => 500,
        conduit_core::Error::Serialize(_) => 500,
        conduit_core::Error::ChannelFull => 500,
    }
}

/// Truncate a user-supplied name to 64 bytes and strip control characters
/// to prevent log injection and oversized error messages.
///
/// Truncation respects UTF-8 character boundaries — the output is always
/// valid UTF-8 with at most 64 bytes of text content.
fn sanitize_name(name: &str) -> String {
    let truncated = if name.len() > 64 {
        // Walk back from byte 64 to find a valid char boundary.
        let mut end = 64;
        while end > 0 && !name.is_char_boundary(end) {
            end -= 1;
        }
        &name[..end]
    } else {
        name
    };
    truncated.chars().filter(|c| !c.is_control()).collect()
}

/// Format a [`conduit_core::Error`] for inclusion in HTTP error responses,
/// sanitizing any embedded user-supplied names (command or channel names).
fn sanitize_error(e: &conduit_core::Error) -> String {
    match e {
        conduit_core::Error::UnknownCommand(name) => {
            format!("unknown command: {}", sanitize_name(name))
        }
        conduit_core::Error::UnknownChannel(name) => {
            format!("unknown channel: {}", sanitize_name(name))
        }
        other => other.to_string(),
    }
}

/// Percent-decode a URL path segment (e.g., `hello%20world` → `hello world`).
///
/// This is a minimal implementation — no new dependency needed. Only `%XX`
/// sequences with valid hex digits are decoded; all other bytes pass through
/// unchanged.
fn percent_decode(input: &str) -> String {
    let mut result = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                result.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

/// Convert a single ASCII hex character to its 4-bit numeric value.
///
/// Unlike [`hex_digit_ct`], this does NOT need to be constant-time — it is
/// used for URL percent-decoding, not security-critical key validation.
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Generate 32 random bytes for the per-launch invoke key.
fn generate_invoke_key_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("conduit: failed to generate invoke key");
    bytes
}

/// Hex-encode a byte slice (no per-byte allocation).
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        hex.push(HEX[(b >> 4) as usize] as char);
        hex.push(HEX[(b & 0x0f) as usize] as char);
    }
    hex
}

/// Hex-decode a string into bytes. Returns `None` on invalid input.
///
/// This is the non-constant-time version used for non-security paths.
/// For invoke key validation, see [`hex_digit_ct`] and the constant-time
/// path in [`PluginState::validate_invoke_key`].
#[cfg(test)]
fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks(2) {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        bytes.push((hi << 4) | lo);
    }
    Some(bytes)
}

/// Convert a single ASCII hex character to its 4-bit numeric value.
#[cfg(test)]
fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Validate an invoke key candidate using constant-time operations.
///
/// The length check (must be exactly 64 hex chars) is not constant-time
/// because the expected length is public knowledge. The hex decode and
/// byte comparison are fully constant-time: no early returns on invalid
/// characters, and the comparison always runs even if decode failed.
fn validate_invoke_key_ct(expected: &[u8; 32], candidate: &str) -> bool {
    let candidate_bytes = candidate.as_bytes();

    // Length is not secret — always 64 hex chars for 32 bytes.
    if candidate_bytes.len() != 64 {
        return false;
    }

    // Constant-time hex decode: always process all 32 byte pairs.
    let mut decoded = [0u8; 32];
    let mut all_valid = 1u8;

    for i in 0..32 {
        let (hi_val, hi_ok) = hex_digit_ct(candidate_bytes[i * 2]);
        let (lo_val, lo_ok) = hex_digit_ct(candidate_bytes[i * 2 + 1]);
        decoded[i] = (hi_val << 4) | lo_val;
        all_valid &= hi_ok & lo_ok;
    }

    // Always compare, even if some hex digits were invalid.
    let cmp_ok: bool = expected.ct_eq(&decoded).into();

    // Combine with bitwise AND — no short-circuit.
    (all_valid == 1) & cmp_ok
}

/// Constant-time hex digit decode for security-critical paths.
///
/// Returns `(value, valid)` where `valid` is `1` if the byte is a valid
/// hex character and `0` otherwise. All operations are bitwise — no
/// comparisons, no branches. When invalid, `value` is `0`.
///
/// Uses subtraction + sign-bit masking on `i16` to produce range-check
/// masks without any comparison operators that could compile to branches.
fn hex_digit_ct(b: u8) -> (u8, u8) {
    // Promote to i16 so wrapping_sub produces a sign bit we can extract.
    let b = b as i16;

    // Check if b is in '0'..='9'  (0x30..=0x39)
    let d = b.wrapping_sub(0x30); // b - '0'
    // d >= 0 && d < 10: (!d) is negative iff d >= 0; (d-10) is negative iff d < 10.
    // Combining via AND and extracting the sign bit gives us a mask.
    let digit_mask = ((!d) & (d.wrapping_sub(10))) >> 15;
    let digit_mask = (digit_mask & 1) as u8;

    // Check if b is in 'a'..='f'  (0x61..=0x66)
    let l = b.wrapping_sub(0x61); // b - 'a'
    let lower_mask = ((!l) & (l.wrapping_sub(6))) >> 15;
    let lower_mask = (lower_mask & 1) as u8;

    // Check if b is in 'A'..='F'  (0x41..=0x46)
    let u = b.wrapping_sub(0x41); // b - 'A'
    let upper_mask = ((!u) & (u.wrapping_sub(6))) >> 15;
    let upper_mask = (upper_mask & 1) as u8;

    let val = ((d as u8 & 0x0f) & digit_mask.wrapping_neg())
        .wrapping_add((l as u8).wrapping_add(10) & lower_mask.wrapping_neg())
        .wrapping_add((u as u8).wrapping_add(10) & upper_mask.wrapping_neg());
    let valid = digit_mask | lower_mask | upper_mask;

    (val, valid)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- hex encode/decode tests --

    #[test]
    fn hex_encode_roundtrip() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_encode(&[0x00, 0xff]), "00ff");
    }

    #[test]
    fn hex_decode_valid() {
        assert_eq!(hex_decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(hex_decode(""), Some(vec![]));
        assert_eq!(hex_decode("00ff"), Some(vec![0x00, 0xff]));
    }

    #[test]
    fn hex_decode_uppercase() {
        assert_eq!(hex_decode("DEADBEEF"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(hex_decode("DeAdBeEf"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }

    #[test]
    fn hex_decode_odd_length() {
        assert_eq!(hex_decode("abc"), None);
        assert_eq!(hex_decode("a"), None);
    }

    #[test]
    fn hex_decode_invalid_chars() {
        assert_eq!(hex_decode("zz"), None);
        assert_eq!(hex_decode("gg"), None);
        assert_eq!(hex_decode("0x"), None);
    }

    #[test]
    fn hex_roundtrip_32_bytes() {
        let original = generate_invoke_key_bytes();
        let encoded = hex_encode(&original);
        assert_eq!(encoded.len(), 64);
        let decoded = hex_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    // -- constant-time hex tests --

    #[test]
    fn hex_digit_ct_valid_chars() {
        for b in b'0'..=b'9' {
            let (val, valid) = hex_digit_ct(b);
            assert_eq!(valid, 1, "digit {b} should be valid");
            assert_eq!(val, b - b'0');
        }
        for b in b'a'..=b'f' {
            let (val, valid) = hex_digit_ct(b);
            assert_eq!(valid, 1, "lower {b} should be valid");
            assert_eq!(val, b - b'a' + 10);
        }
        for b in b'A'..=b'F' {
            let (val, valid) = hex_digit_ct(b);
            assert_eq!(valid, 1, "upper {b} should be valid");
            assert_eq!(val, b - b'A' + 10);
        }
    }

    #[test]
    fn hex_digit_ct_invalid_chars() {
        for &b in &[b'g', b'z', b'G', b'Z', b' ', b'\0', b'/', b':', b'@', b'`'] {
            let (_val, valid) = hex_digit_ct(b);
            assert_eq!(valid, 0, "char {b} should be invalid");
        }
    }

    #[test]
    fn hex_digit_ct_matches_hex_digit() {
        for b in 0..=255u8 {
            let ct_result = hex_digit_ct(b);
            let std_result = hex_digit(b);
            match std_result {
                Some(v) => {
                    assert_eq!(ct_result.1, 1, "mismatch at {b}: ct says invalid");
                    assert_eq!(ct_result.0, v, "value mismatch at {b}");
                }
                None => {
                    assert_eq!(ct_result.1, 0, "mismatch at {b}: ct says valid");
                }
            }
        }
    }

    // -- make_response tests --

    #[test]
    fn make_response_200() {
        let resp = make_response(200, "application/octet-stream", b"hello".to_vec());
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.body(), b"hello");
    }

    #[test]
    fn make_response_404() {
        let resp = make_response(404, "text/plain", b"not found".to_vec());
        assert_eq!(resp.status(), 404);
        assert_eq!(resp.body(), b"not found");
    }

    // -- State<T> injection tests --

    #[command]
    fn with_state(state: tauri::State<'_, String>, name: String) -> String {
        format!("{}: {name}", state.as_str())
    }

    #[test]
    fn state_injection_wrong_context_returns_error() {
        use conduit_core::ConduitHandler;
        use conduit_derive::handler;

        let payload = serde_json::to_vec(&serde_json::json!({ "name": "test" })).unwrap();
        let wrong_ctx: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());

        match handler!(with_state).call(payload, wrong_ctx) {
            conduit_core::HandlerResponse::Sync(Err(conduit_core::Error::Handler(msg))) => {
                assert!(
                    msg.contains("handler context must be HandlerContext"),
                    "unexpected error message: {msg}"
                );
            }
            _ => panic!("expected Sync(Err(Handler))"),
        }
    }

    #[test]
    fn original_state_function_preserved() {
        // The original function with_state is preserved and callable directly.
        // We can't call it without an actual Tauri State, but we can verify
        // the function exists and has the right signature by taking a reference.
        let _fn_ref: fn(tauri::State<'_, String>, String) -> String = with_state;
    }

    // -- validate_invoke_key tests --

    #[test]
    fn validate_invoke_key_correct() {
        let key = [0xab_u8; 32];
        let hex = hex_encode(&key);
        assert!(validate_invoke_key_ct(&key, &hex));
    }

    #[test]
    fn validate_invoke_key_wrong_key() {
        let key = [0xab_u8; 32];
        let wrong = hex_encode(&[0x00_u8; 32]);
        assert!(!validate_invoke_key_ct(&key, &wrong));
    }

    #[test]
    fn validate_invoke_key_wrong_length() {
        let key = [0xab_u8; 32];
        assert!(!validate_invoke_key_ct(&key, "abcdef"));
        assert!(!validate_invoke_key_ct(&key, ""));
        assert!(!validate_invoke_key_ct(&key, &"a".repeat(63)));
        assert!(!validate_invoke_key_ct(&key, &"a".repeat(65)));
    }

    #[test]
    fn validate_invoke_key_invalid_hex() {
        let key = [0xab_u8; 32];
        // 64 chars but invalid hex
        assert!(!validate_invoke_key_ct(&key, &"zz".repeat(32)));
        assert!(!validate_invoke_key_ct(&key, &"gg".repeat(32)));
    }

    #[test]
    fn validate_invoke_key_uppercase_accepted() {
        let key = [0xab_u8; 32];
        let hex = hex_encode(&key);
        // hex_digit_ct handles uppercase, so uppercase of a valid key should match
        assert!(validate_invoke_key_ct(&key, &hex.to_uppercase()));
    }

    #[test]
    fn validate_invoke_key_random_roundtrip() {
        let key = generate_invoke_key_bytes();
        let hex = hex_encode(&key);
        assert!(validate_invoke_key_ct(&key, &hex));
    }

    // -- make_error_response tests --

    #[test]
    fn make_error_response_json_format() {
        let resp = make_error_response(500, "something failed");
        assert_eq!(resp.status(), 500);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["error"], "something failed");
    }

    #[test]
    fn make_error_response_escapes_special_chars() {
        let resp = make_error_response(400, r#"bad "input" with \ slash"#);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["error"], r#"bad "input" with \ slash"#);
    }

    // -- percent_decode tests --

    #[test]
    fn percent_decode_no_encoding() {
        assert_eq!(percent_decode("hello"), "hello");
        assert_eq!(percent_decode("foo-bar_baz"), "foo-bar_baz");
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("%2F"), "/");
        assert_eq!(percent_decode("%2f"), "/");
    }

    #[test]
    fn percent_decode_multiple() {
        assert_eq!(percent_decode("a%20b%20c"), "a b c");
        assert_eq!(percent_decode("%41%42%43"), "ABC");
    }

    #[test]
    fn percent_decode_incomplete_sequence() {
        // Incomplete %XX at end — pass through unchanged.
        assert_eq!(percent_decode("hello%2"), "hello%2");
        assert_eq!(percent_decode("hello%"), "hello%");
    }

    #[test]
    fn percent_decode_invalid_hex() {
        // Invalid hex chars after % — pass through unchanged.
        assert_eq!(percent_decode("hello%GG"), "hello%GG");
        assert_eq!(percent_decode("%ZZ"), "%ZZ");
    }

    #[test]
    fn percent_decode_empty() {
        assert_eq!(percent_decode(""), "");
    }

    // -- sanitize_name tests --

    #[test]
    fn sanitize_name_short() {
        assert_eq!(sanitize_name("hello"), "hello");
    }

    #[test]
    fn sanitize_name_truncates_long() {
        let long = "a".repeat(100);
        assert_eq!(sanitize_name(&long).len(), 64);
    }

    #[test]
    fn sanitize_name_strips_control_chars() {
        assert_eq!(sanitize_name("hello\x00world"), "helloworld");
        assert_eq!(sanitize_name("foo\nbar\rbaz"), "foobarbaz");
    }

    #[test]
    fn sanitize_name_multibyte_utf8() {
        // "a" repeated 63 times + "é" (2 bytes: 0xC3 0xA9) = 65 bytes total.
        // Byte 64 is the second byte of "é", not a char boundary.
        // Must not panic — should truncate to the last valid boundary (63 'a's).
        let name = format!("{}{}", "a".repeat(63), "é");
        assert_eq!(name.len(), 65);
        let sanitized = sanitize_name(&name);
        assert_eq!(sanitized, "a".repeat(63));

        // 4-byte character crossing the 64-byte boundary.
        let name = format!("{}🦀", "a".repeat(62)); // 62 + 4 = 66 bytes
        assert_eq!(name.len(), 66);
        let sanitized = sanitize_name(&name);
        assert_eq!(sanitized, "a".repeat(62));

        // Exactly 64 bytes of ASCII — no truncation needed.
        let name = "a".repeat(64);
        assert_eq!(sanitize_name(&name), "a".repeat(64));
    }

    // -- error_to_status tests --

    #[test]
    fn error_to_status_mapping() {
        use conduit_core::Error;
        assert_eq!(error_to_status(&Error::UnknownCommand("x".into())), 404);
        assert_eq!(error_to_status(&Error::UnknownChannel("x".into())), 404);
        assert_eq!(error_to_status(&Error::AuthFailed), 403);
        assert_eq!(error_to_status(&Error::DecodeFailed), 400);
        assert_eq!(error_to_status(&Error::PayloadTooLarge(999)), 413);
        assert_eq!(error_to_status(&Error::Handler("x".into())), 500);
        assert_eq!(error_to_status(&Error::ChannelFull), 500);
    }

    // -- channel validation tests --

    #[test]
    fn validate_channel_name_valid() {
        validate_channel_name("telemetry");
        validate_channel_name("my-channel");
        validate_channel_name("my_channel");
        validate_channel_name("Channel123");
        validate_channel_name("a");
    }

    #[test]
    #[should_panic(expected = "invalid channel name")]
    fn validate_channel_name_empty() {
        validate_channel_name("");
    }

    #[test]
    #[should_panic(expected = "invalid channel name")]
    fn validate_channel_name_spaces() {
        validate_channel_name("my channel");
    }

    #[test]
    #[should_panic(expected = "invalid channel name")]
    fn validate_channel_name_special_chars() {
        validate_channel_name("my.channel");
    }

    #[test]
    #[should_panic(expected = "duplicate channel name")]
    fn duplicate_channel_panics() {
        PluginBuilder::new()
            .channel("telemetry")
            .channel("telemetry");
    }

    #[test]
    #[should_panic(expected = "duplicate channel name")]
    fn duplicate_channel_different_kinds_panics() {
        PluginBuilder::new().channel("data").channel_ordered("data");
    }
}
