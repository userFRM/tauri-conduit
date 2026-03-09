#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! # tauri-plugin-conduit
//!
//! Tauri v2 plugin for conduit — binary IPC over the `conduit://` custom
//! protocol.
//!
//! Registers a `conduit://` custom protocol for zero-overhead in-process
//! binary dispatch via a synchronous handler table. No network surface.
//!
//! ## Usage
//!
//! ```rust,ignore
//! tauri::Builder::default()
//!     .plugin(
//!         tauri_plugin_conduit::init()
//!             .command("ping", |_| b"pong".to_vec())
//!             .command("get_ticks", handle_ticks)
//!             .channel("telemetry")           // ring buffer for streaming
//!             .build()
//!     )
//!     .run(tauri::generate_context!())
//!     .unwrap();
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use conduit_core::{RingBuffer, Router};
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

// ---------------------------------------------------------------------------
// BootstrapInfo — returned to JS via `conduit_bootstrap` command
// ---------------------------------------------------------------------------

/// Connection info returned to the frontend during bootstrap.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapInfo {
    /// Base URL for the custom protocol (e.g., `"conduit://localhost"`).
    pub protocol_base: String,
    /// Per-launch invoke key for custom protocol authentication (hex-encoded).
    pub invoke_key: String,
    /// Available ring buffer channel names.
    pub channels: Vec<String>,
}

impl std::fmt::Debug for BootstrapInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapInfo")
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
/// Holds the dispatch table, named ring buffer channels, the per-launch
/// invoke key, and the app handle for emitting push notifications.
pub struct PluginState<R: Runtime> {
    dispatch: Arc<Router>,
    /// Named ring buffer channels for server→client streaming.
    channels: HashMap<String, Arc<RingBuffer>>,
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
    /// Get a ring buffer channel by name (for pushing data from Rust handlers).
    pub fn channel(&self, name: &str) -> Option<&Arc<RingBuffer>> {
        self.channels.get(name)
    }

    /// Push binary data to a named ring buffer channel and notify JS listeners.
    ///
    /// After writing to the ring buffer, emits a `conduit:data-available` event
    /// with the channel name as payload. JS subscribers receive this event and
    /// auto-drain the binary data via the custom protocol endpoint.
    pub fn push(&self, channel: &str, data: &[u8]) -> Result<(), String> {
        let rb = self
            .channels
            .get(channel)
            .ok_or_else(|| format!("unknown channel: {channel}"))?;
        let _ = rb.push(data);
        let _ = self.app_handle.emit("conduit:data-available", channel);
        Ok(())
    }

    /// Return the list of registered channel names.
    pub fn channel_names(&self) -> Vec<String> {
        self.channels.keys().cloned().collect()
    }

    /// Validate an invoke key candidate using constant-time comparison.
    fn validate_invoke_key(&self, candidate: &str) -> bool {
        let candidate_bytes = match hex_decode(candidate) {
            Some(b) => b,
            None => return false,
        };
        if candidate_bytes.len() != 32 {
            return false;
        }
        let ok: bool = self
            .invoke_key_bytes
            .ct_eq(&candidate_bytes)
            .into();
        ok
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Return bootstrap info so the JS client knows how to reach the conduit
/// custom protocol.
#[tauri::command]
fn conduit_bootstrap(
    state: tauri::State<'_, PluginState<tauri::Wry>>,
) -> Result<BootstrapInfo, String> {
    Ok(BootstrapInfo {
        protocol_base: "conduit://localhost".to_string(),
        invoke_key: state.invoke_key.clone(),
        channels: state.channel_names(),
    })
}

/// Subscribe to a channel (or list of channels). Returns the list of channel
/// names that were successfully subscribed. The actual data delivery happens
/// via `conduit:data-available` events + protocol drain.
#[tauri::command]
fn conduit_subscribe(
    state: tauri::State<'_, PluginState<tauri::Wry>>,
    channels: Vec<String>,
) -> Result<Vec<String>, String> {
    // Validate that all requested channels exist.
    let mut subscribed = Vec::new();
    for ch in &channels {
        if state.channels.contains_key(ch) {
            subscribed.push(ch.clone());
        }
    }
    Ok(subscribed)
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
    /// Named ring buffer channels: (name, capacity in bytes).
    channel_defs: Vec<(String, usize)>,
}

impl std::fmt::Debug for PluginBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginBuilder")
            .field("commands", &self.commands.len())
            .field("channel_defs", &self.channel_defs)
            .finish()
    }
}

/// Default ring buffer capacity (64 KB).
const DEFAULT_CHANNEL_CAPACITY: usize = 64 * 1024;

impl PluginBuilder {
    /// Create a new, empty plugin builder.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            channel_defs: Vec::new(),
        }
    }

    /// Register a synchronous command handler.
    ///
    /// The handler receives the raw request payload and must return the raw
    /// response payload. Command names correspond to the path segment in the
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

    /// Register a named ring buffer channel with the default capacity (64 KB).
    ///
    /// The JS client can subscribe to push notifications for this channel,
    /// or poll it directly via `conduit://localhost/drain/<name>`.
    pub fn channel(mut self, name: impl Into<String>) -> Self {
        self.channel_defs
            .push((name.into(), DEFAULT_CHANNEL_CAPACITY));
        self
    }

    /// Register a named ring buffer channel with a custom byte capacity.
    pub fn channel_with_capacity(mut self, name: impl Into<String>, capacity: usize) -> Self {
        self.channel_defs.push((name.into(), capacity));
        self
    }

    /// Build the Tauri v2 plugin.
    ///
    /// This consumes the builder and returns a [`TauriPlugin`] that can be
    /// passed to `tauri::Builder::plugin`.
    pub fn build<R: Runtime>(self) -> TauriPlugin<R> {
        let commands = self.commands;
        let channel_defs = self.channel_defs;

        TauriPluginBuilder::<R>::new("conduit")
            // --- Custom protocol: conduit://localhost/invoke/<cmd> ---
            .register_uri_scheme_protocol("conduit", move |ctx, request| {
                // Extract the managed PluginState from the app handle.
                let state: tauri::State<'_, PluginState<R>> = ctx.app_handle().state();

                let url = request.uri().to_string();

                // Parse the URL to extract the command name.
                // Expected format: conduit://localhost/invoke/<cmd_name>
                let parsed = match url::Url::parse(&url) {
                    Ok(u) => u,
                    Err(_) => {
                        return make_response(400, "text/plain", b"invalid URL".to_vec());
                    }
                };

                let path = parsed.path(); // e.g. "/invoke/ping"
                let segments: Vec<&str> = path
                    .trim_start_matches('/')
                    .splitn(2, '/')
                    .collect();

                if segments.len() != 2 {
                    return make_response(404, "text/plain", b"not found: expected /invoke/<cmd> or /drain/<channel>".to_vec());
                }

                // Validate the invoke key from the X-Conduit-Key header (common to all routes).
                let key = match request.headers().get("X-Conduit-Key") {
                    Some(v) => match v.to_str() {
                        Ok(s) => s.to_string(),
                        Err(_) => return make_response(401, "text/plain", b"invalid invoke key header".to_vec()),
                    },
                    None => return make_response(401, "text/plain", b"missing invoke key".to_vec()),
                };

                if !state.validate_invoke_key(&key) {
                    return make_response(403, "text/plain", b"invalid invoke key".to_vec());
                }

                let action = segments[0];
                let target = segments[1];

                match action {
                    "invoke" => {
                        let body = request.body().to_vec();

                        let dispatch = Arc::clone(&state.dispatch);
                        let result = std::panic::catch_unwind(
                            std::panic::AssertUnwindSafe(|| {
                                dispatch.call_or_error_bytes(target, body)
                            })
                        );

                        match result {
                            Ok(response_payload) => {
                                make_response(200, "application/octet-stream", response_payload)
                            }
                            Err(_) => {
                                make_response(500, "text/plain", b"handler panicked".to_vec())
                            }
                        }
                    }
                    "drain" => {
                        // Drain all frames from the named ring buffer channel.
                        match state.channel(target) {
                            Some(rb) => {
                                let blob = rb.drain_all();
                                make_response(200, "application/octet-stream", blob)
                            }
                            None => make_response(404, "text/plain", format!("unknown channel: {target}").into_bytes()),
                        }
                    }
                    _ => make_response(404, "text/plain", b"not found: expected /invoke/<cmd> or /drain/<channel>".to_vec()),
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

                // Register all commands that were added via the builder.
                for register_fn in commands {
                    register_fn(&dispatch);
                }

                // Create named ring buffer channels.
                let mut channels = HashMap::new();
                for (name, capacity) in channel_defs {
                    channels.insert(name, Arc::new(RingBuffer::new(capacity)));
                }

                // Generate the per-launch invoke key.
                let invoke_key_bytes = generate_invoke_key_bytes();
                let invoke_key = hex_encode(&invoke_key_bytes);

                // Obtain the app handle for emitting events.
                let app_handle = app.app_handle().clone();

                let state = PluginState {
                    dispatch,
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
/// tauri::Builder::default()
///     .plugin(
///         tauri_plugin_conduit::init()
///             .command("ping", |_| b"pong".to_vec())
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

/// Generate 32 random bytes for the per-launch invoke key.
fn generate_invoke_key_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("conduit: failed to generate invoke key");
    bytes
}

/// Hex-encode a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// Hex-decode a string into bytes. Returns `None` on invalid input.
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
fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
