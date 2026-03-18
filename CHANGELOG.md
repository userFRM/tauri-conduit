# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.1.1] - 2026-03-18

### Fixed

- **npm package contents** — packed artifacts now reliably include `dist/*`, and CI smoke-checks the tarball contents before release.
- **TypeScript test workflow** — `npm test` now uses the same `tsx`-based path exercised in CI, fixing the local `ERR_MODULE_NOT_FOUND` failure mode.
- **Channel subscriptions** — `subscribe()` now validates channels up front, uses per-channel events, and surfaces initial drain errors instead of silently swallowing them.
- **Documentation accuracy** — aligned workspace/package version strings, fixed the plugin README import example, corrected ordered-channel wording, and refined the README positioning without dropping the drop-in replacement claim.

## [2.1.0] - 2026-03-17

### Performance — Rust core (conduit-core)

- **Preformatted wire buffer in RingBuffer & Queue** — `drain_all` now performs a single `memcpy` instead of N x 2 `extend_from_slice` calls. Internal storage changed from `VecDeque<Vec<u8>>` to a contiguous wire buffer that maintains frames in drain-ready format. Measured: **drain_all 2x faster** (4.2µs → 2.1µs at 100 frames), **3.2x faster** at 1000 frames (55µs → 17µs). Push throughput **1.8-2.5x faster** (eliminated per-frame heap allocation).
- **`Bytes` newtype** for efficient bulk `Vec<u8>` encode/decode — avoids per-element encoding overhead for byte arrays.
- **`MIN_SIZE` trait constant on `Decode`** — derive macro generates an upfront bounds check before field-by-field decoding, failing fast on undersized buffers.
- **Overflow guards** — `u32` truncation check on frame length, `frame_count` capped at `u32::MAX`, `checked_add` for 32-bit safety in size calculations.

### Performance — Plugin (tauri-plugin-conduit)

- **Protocol handler allocation elimination** — uses `URI::path()` directly, borrows the invoke key instead of cloning, and `Cow` percent-decoding avoids allocation when the input is already valid UTF-8.
- **Single-spawn async handlers** — replaced double-spawn pattern with `FutureExt::catch_unwind` from `futures-util`, reducing per-invocation overhead.
- **Pre-cached `Arc<AppHandle>`** in `PluginState` — eliminates repeated `Arc::clone` from the app handle on every protocol request.

### Performance — TypeScript client

- **`parseDrainBlob` zero-copy** — returns `Uint8Array` subarray views into the original buffer instead of copying each frame.
- **`WireWriter` builder class** — single-allocation encoding: pre-calculates total size, writes all fields into one `ArrayBuffer`, avoids intermediate allocations.
- **`JSON.stringify` direct to fetch body** — skips redundant `TextEncoder` step; the browser handles string-to-UTF-8 natively.
- **`AbortSignal.timeout` for drain** — replaces manual `AbortController` + `setTimeout` wiring with the built-in API.

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
- **Breaking:** Parameter names follow Tauri-style camelCase conversion in JSON. A Rust `user_name` is passed as `userName` in JS
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
