# tauri-conduit -- LLM Development Guide

> Binary IPC for Tauri v2 via `conduit://` custom protocol.

## What this is

`tauri-conduit` replaces Tauri's JSON-over-webview IPC with a binary custom protocol (`conduit://`) that runs entirely in-process. Single transport, no network surface. conduit-core stays sync (no tokio dependency). Async handlers are supported at the plugin level — `#[tauri_conduit::command] async fn` generates a `HandlerResponse::Async` future that the plugin spawns on tokio via `tauri::async_runtime::spawn`, exactly like `#[tauri::command]`.

Streaming uses a hybrid model: Rust pushes binary frames into a ring buffer + emits a `conduit:data-available` Tauri event. JS listens for the event, then drains binary data via `conduit://localhost/drain/<channel>`. Users can also poll with `drain()` directly.

## Architecture

```
crates/
  conduit-core/            Core library
    src/
      lib.rs                 Public API re-exports
      codec.rs               11-byte frame header, Encode/Decode traits
      error.rs               Error types
      router.rs              Router (synchronous command registry)
      ringbuf.rs             In-process ring buffer for streaming (preformatted wire buffer)
      queue.rs               Bounded queue with guaranteed delivery (preformatted wire buffer)
      channel.rs             ChannelBuffer enum (Lossy/Reliable)
      handler.rs             ConduitHandler trait, HandlerContext, HandlerResponse
    benches/
      codec_bench.rs         Frame + wire encoding benchmarks
      ringbuf_bench.rs       Ring buffer throughput + contention benchmarks
      dispatch_bench.rs      Command dispatch benchmarks
      comparison_bench.rs    Tauri vs Level 1 vs Level 2 head-to-head
      handler_bench.rs       Handler registration mode benchmarks
      queue_bench.rs         Queue vs RingBuffer benchmarks
  conduit/                  Facade crate (enables #[tauri_conduit::command] path)
    src/lib.rs               Re-exports command macro, ConduitHandler, core types
  conduit-derive/           Proc macros
    src/lib.rs               #[derive(Encode, Decode)], #[command] (generates ConduitHandler impl), handler!()
  tauri-plugin-conduit/     Tauri v2 plugin
    src/lib.rs               Plugin builder, async protocol handler, subscribe command
packages/
  tauri-plugin-conduit/     TypeScript client (tauri-plugin-conduit)
    src/
      index.ts               Drop-in invoke(), connect(), subscribe(), drain()
      negotiate.ts           Bootstrap (obtains invoke key + channel list)
      error.ts               ConduitError class
      codec/frame.ts         11-byte frame codec (JS mirror)
      codec/wire.ts          Binary decode/encode helpers, parseDrainBlob, WireWriter
      transport/protocol.ts  Custom protocol transport (conduit://)
```

## Key types

| Type | Crate | Purpose |
|---|---|---|
| `Conduit` | TS client | Main client interface: `invoke()`, `invokeBinary()`, `subscribe()`, `drain()` |
| `WireWriter` | TS client | Builder for single-allocation binary encoding (pre-calculates size, writes into one ArrayBuffer) |
| `PluginState<R>` | tauri-plugin-conduit | Managed Tauri state: router, handler map, ring buffers, invoke key, app handle |
| `PluginBuilder` | tauri-plugin-conduit | Builder: `.handler()`, `.handler_raw()`, `.command()`, `.channel()`, `.build()` |
| `ConduitHandler` | conduit-core | Trait for sync/async handlers, implemented by `#[command]`-generated structs |
| `HandlerResponse` | conduit-core | Enum: `Sync(Result<Vec<u8>, Error>)` or `Async(Pin<Box<dyn Future>>)` |
| `Router` | conduit-core | Named synchronous command handlers (payload in, payload out) |
| `RingBuffer` | conduit-core | Thread-safe circular buffer with lossy back-pressure (preformatted wire buffer -- drain_all is single memcpy) |
| `FrameHeader` | conduit-core | 11-byte frame header for all conduit messages |
| `Encode` / `Decode` | conduit-core | Traits for binary serialization of fixed-layout structs. `Decode` has a `MIN_SIZE` constant for upfront bounds checking |
| `Bytes` | conduit-core | Newtype for `Vec<u8>` with efficient bulk encode/decode (no per-element overhead) |
| `PushOutcome` | conduit-core | Enum: `Accepted(usize)` or `TooLarge` (opt-in via `push_checked()`) |
| `Queue` | conduit-core | Thread-safe bounded queue with guaranteed delivery (preformatted wire buffer -- drain_all is single memcpy) |
| `ChannelBuffer` | conduit-core | Enum wrapping `RingBuffer` (Lossy) or `Queue` (Reliable) |
| `Error` | conduit-core | Error types (`UnknownCommand`, `PayloadTooLarge`, `AuthFailed`, `Handler`, `Serialize`, `UnknownChannel`) |

## Streaming / push model

1. Register channels on the Rust side: `.channel("telemetry")` (lossy) or `.channel_ordered("events")` (reliable)
2. Push data: `state.push("telemetry", &bytes)` -- writes to ring buffer, emits both `conduit:data-available` (global, payload=channel name) and `conduit:data-available:{channel}` (per-channel) Tauri events
3. JS subscribes via `conduit_subscribe` Tauri command (silently filters to existing channels)
4. JS listens for global `conduit:data-available` event with payload filter, then calls `conduit.drain("telemetry")` to fetch binary blob via custom protocol
5. Or: JS calls `drain()` directly for pull-based access (user controls timing)

Two channel types: lossy `RingBuffer` (oldest frames dropped on overflow, default 64 KB) and reliable `Queue` (backpressure error when full, guaranteed delivery). Both use a preformatted wire buffer internally -- frames are stored in drain-ready format so `drain_all` is a single `memcpy` instead of per-frame serialization.

## Frame format (11 bytes)

```
[u8  version]           always 1
[u8  reserved]          0 (reserved for future use)
[u8  msg_type]          0x00=Request, 0x01=Response, 0x02=Push, 0x04=Error
[u32 sequence]          LE, monotonic
[u32 payload_len]       LE, byte count
[payload ...]           payload_len bytes
```

## Security model

Everything runs in-process. No network endpoints.

- **Per-launch invoke key** -- 32 random bytes, constant-time validation (`subtle` crate), sent via `X-Conduit-Key` header. Any webview with the invoke key can call any registered command -- there is no per-command ACL. This is simpler than Tauri's capability system.
- **Panic isolation** -- handler panics caught via `catch_unwind`, returned as 500 errors
- **CSP compliance** -- no CSP exceptions required
- **Trust boundary** -- frontend communicates only through router
- **Threat model** -- the invoke key protects against cross-origin requests, not against malicious JS running in the same WebView context. This matches Tauri's own trust model.

## Build, test, bench

```sh
cargo test --workspace
cargo clippy --workspace                            # zero warnings
cd crates/conduit-core && cargo bench               # all benchmarks
cd crates/conduit-core && cargo bench -- comparison  # Tauri vs Level 1 vs Level 2
```

## Dependency notes

conduit-core re-exports `sonic_rs` and `serde` as `#[doc(hidden)]` items for use by generated code. Changing their major versions is a semver-breaking change for conduit-core, even though they are hidden from docs.

tauri-plugin-conduit depends on `futures-util` for `FutureExt::catch_unwind` (single-spawn async handler pattern with panic isolation).

## Relationship to tauri-wire

- `tauri-wire` = codec only (9-byte frame, transport-agnostic)
- `tauri-conduit` = unified: codec (11-byte frame) + custom protocol + ring buffer + derive macros + drop-in API

## What NOT to do

- Don't add network transports -- the custom protocol is intentionally in-process only
- Don't skip invoke key authentication
- Don't add tokio as a dependency to conduit-core -- the `ConduitHandler` trait uses only `std::future::Future`, no runtime. Async dispatch happens at the plugin level via `tauri::async_runtime::spawn`
- Don't add typed pub-sub or messaging abstractions -- the ring buffer + events is the streaming primitive; users layer their own semantics on top
- Don't reference `TransportTier` or negotiate -- these concepts have been removed
