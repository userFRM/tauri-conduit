# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0] - 2026-03-10

### Added

- **`#[tauri_conduit::command]` attribute macro** with full `#[tauri::command]` parity: named parameters, `State<T>` injection, `AppHandle`/`Window`/`WebviewWindow`/`Webview` injection, `Result<T, E>` returns, and truly async handlers (tokio-spawned)
- **`handler!()` proc macro** to resolve command functions to their generated handler structs for registration
- **`ConduitHandler` trait** with `HandlerResponse::Sync` and `HandlerResponse::Async` variants
- **`HandlerContext`** wrapping `Arc<dyn Any>` app handle + optional webview label
- **`handler_raw()`** on `PluginBuilder` for backward-compatible closure-based handler registration (v1 migration path)
- **`Queue`** bounded buffer with guaranteed delivery and backpressure (no frame drops)
- **`ChannelBuffer`** enum unifying `RingBuffer` (Lossy) and `Queue` (Reliable)
- **`PushOutcome`** enum (`Accepted(usize)` / `TooLarge`) via opt-in `push_checked()` methods
- **`push_checked()`** on `RingBuffer`, `ChannelBuffer` for richer push outcome reporting
- **`channel_ordered()`** and `channel_ordered_with_capacity()` on `PluginBuilder` for reliable channels
- **Async protocol handler** using `register_asynchronous_uri_scheme_protocol` for true async dispatch
- **Constant-time invoke key validation** (`validate_invoke_key_ct`) using bitwise masking
- **Input sanitization**: `sanitize_name`, `percent_decode`, path traversal protection
- **JSON error responses** via `make_error_response` in the protocol handler
- **Mutex poison recovery** helpers: `lock_or_recover`, `write_or_recover`, `read_or_recover`
- **`DRAIN_FRAME_OVERHEAD`** constant consolidating the 8-byte per-frame wire overhead
- **Channel name validation** with regex pattern + duplicate detection at builder time
- **Dual event emission**: `conduit:data-available` (global) + `conduit:data-available:{channel}` (per-channel) on push
- **`ConduitError`** class in TypeScript with `status`, `target`, `message` fields
- **`resetConduit()`** for forcing re-bootstrap (development hot-reload)
- **`parseDrainBlob()`** TypeScript helper for parsing drain wire format
- **Bounds checking** on all TypeScript wire read functions
- **Initial drain on subscribe** to catch data pushed before listener registration
- **Compile-fail tests** for derive macro error cases (`trybuild`)

### Changed

- **Breaking:** `.handler()` now accepts `impl ConduitHandler` instead of a closure. Use `handler_raw()` for the old closure signature, or migrate to `#[tauri_conduit::command]` + `handler!()`
- **Breaking:** `RingBuffer::push()` returns `usize` (number of evicted frames) instead of `PushOutcome`. Use `push_checked()` for richer outcome
- **Breaking:** `ChannelBuffer::push()` returns `Result<usize, Error>` instead of `Result<PushOutcome, Error>`. Use `push_checked()` for richer outcome
- **Breaking:** `Error::Serialize` wraps `sonic_rs::Error` directly instead of `String` (restores `source()` chain)
- **Breaking:** Parameter names stay snake_case in JSON (no camelCase conversion). A Rust `user_name` is `user_name` in JS, not `userName`
- `PluginState::push()` returns `Result<(), String>` (backward-compatible with v1)
- `conduit_subscribe` silently filters unknown channels instead of returning an error
- `BootstrapInfo` derives `Clone`, `Serialize`, `Deserialize` with `#[serde(default)]` on `protocol_version`
- TypeScript `invoke()` returns `undefined` (not `null`) for empty responses
- TypeScript `subscribe()` listens on global `conduit:data-available` event with payload filter

### Removed

- `conduit_handlers!` macro (use `handler!()` per-command instead)
- `PushOutcome::is_accepted()` and `PushOutcome::dropped()` helper methods (pattern match directly)
- `max_payload_size` from `PluginBuilder` and `PluginState` (checked after allocation, false security)
- `bootstrapped` AtomicBool guard from `PluginState`
- `_disconnected` flag and `_activeChannels` Set from TypeScript client
- `headers` field from TypeScript `InvokeOptions`
- Backward-compatible dual-path context downcast in derive-generated code

## [1.0.0] - 2026-03-09

### Changed

- **Breaking:** Rename crate `conduit-tauri` to `tauri-plugin-conduit` (Tauri naming convention)
- **Breaking:** Rename TS package `@tauri-conduit/client` to `tauri-plugin-conduit`
- **Breaking:** Rename traits `WireEncode`/`WireDecode` to `Encode`/`Decode`
- **Breaking:** Rename `DispatchTable` to `Router`, `ConduitRingBuffer` to `RingBuffer`, `ConduitError` to `Error`
- **Breaking:** Rename `ConduitState` to `PluginState`, `ConduitPluginBuilder` to `PluginBuilder`
- **Breaking:** Rename methods: `dispatch` to `call`, `wire_encode` to `encode`, `wire_decode` to `decode`, `wire_size` to `encode_size`, `frame_wrap` to `frame_pack`, `frame_unwrap` to `frame_unpack`
- **Breaking:** Rename TS exports: `writeFrameHeader` to `packFrame`, `readFrameHeader` to `unpackFrame`
- **Breaking:** Rename `FrameHeader` field `transport_tier` to `reserved`
- Remove `onData` (redundant alias for `subscribe`)

### Added

- Release workflow with automated npm publish via OIDC provenance
- `workflow_call` trigger on CI for reuse from release workflow

## [0.1.0] - 2026-03-09

### Added

- Binary frame codec with 11-byte header (`conduit-core`)
- `Encode` / `Decode` traits for primitive types, `Vec<u8>`, `String`
- `#[derive(Encode, Decode)]` proc macros (`conduit-derive`)
- Synchronous `Router` for named command handlers
- In-process `RingBuffer` with lossy back-pressure
- Tauri v2 plugin with `conduit://` custom protocol (`tauri-plugin-conduit`)
- TypeScript client with `invoke()`, `subscribe()`, `drain()` (`tauri-plugin-conduit`)
- Per-launch invoke key with constant-time validation (`X-Conduit-Key` header)
- Panic isolation via `catch_unwind` in the protocol handler
- Criterion benchmarks: codec, ring buffer, dispatch, and JSON-vs-binary comparison
