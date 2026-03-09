# tauri-conduit -- LLM Development Guide

> Binary IPC for Tauri v2 via `conduit://` custom protocol.

## What this is

`tauri-conduit` replaces Tauri's JSON-over-webview IPC with a binary custom protocol (`conduit://`) that runs entirely in-process. Single transport, no network surface. Handlers are synchronous (no tokio, no async in conduit-core).

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
      ringbuf.rs             In-process ring buffer for streaming
    benches/
      codec_bench.rs         Frame + wire encoding benchmarks
      ringbuf_bench.rs       Ring buffer throughput + contention benchmarks
      dispatch_bench.rs      Command dispatch benchmarks
      comparison_bench.rs    Tauri vs Level 1 vs Level 2 head-to-head
  conduit-derive/           Proc macros
    src/lib.rs               #[derive(Encode, Decode)]
  tauri-plugin-conduit/     Tauri v2 plugin
    src/lib.rs               Plugin builder, custom protocol handler, subscribe command
packages/
  tauri-plugin-conduit/     TypeScript client (tauri-plugin-conduit)
    src/
      index.ts               Drop-in invoke(), connect(), subscribe(), drain()
      negotiate.ts           Bootstrap (obtains invoke key + channel list)
      codec/frame.ts         11-byte frame codec (JS mirror)
      codec/wire.ts          Binary decode helpers
      transport/protocol.ts  Custom protocol transport (conduit://)
```

## Key types

| Type | Crate | Purpose |
|---|---|---|
| `Conduit` | TS client | Main client interface: `invoke()`, `invokeBinary()`, `subscribe()`, `drain()` |
| `PluginState<R>` | tauri-plugin-conduit | Managed Tauri state: router, ring buffers, invoke key, app handle |
| `PluginBuilder` | tauri-plugin-conduit | Builder: `.command()`, `.channel()`, `.build()` |
| `Router` | conduit-core | Named synchronous command handlers (payload in, payload out) |
| `RingBuffer` | conduit-core | Thread-safe circular buffer with lossy back-pressure |
| `FrameHeader` | conduit-core | 11-byte frame header for all conduit messages |
| `Encode` / `Decode` | conduit-core | Traits for binary serialization of fixed-layout structs |
| `Error` | conduit-core | Error types (`UnknownCommand`, `PayloadTooLarge`, `AuthFailed`, `Json`) |

## Streaming / push model

1. Register channels on the Rust side: `.channel("telemetry")`
2. Push data: `state.push("telemetry", &bytes)` -- writes to ring buffer, emits `conduit:data-available` event
3. JS subscribes via `conduit_subscribe` Tauri command (validates channel names exist)
4. JS listens for `conduit:data-available` events, then calls `conduit.drain("telemetry")` to fetch binary blob
5. Or: JS calls `drain()` directly for pull-based access (user controls timing)

The ring buffer is lossy -- when the byte budget is exceeded, the oldest frames are dropped. Default capacity is 64 KB.

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

- **Per-launch invoke key** -- 32 random bytes, constant-time validation (`subtle` crate), sent via `X-Conduit-Key` header
- **Panic isolation** -- handler panics caught via `catch_unwind`, returned as 500 errors
- **Capability-based ACL** -- integrates with Tauri's permission system
- **CSP compliance** -- no CSP exceptions required
- **Trust boundary** -- frontend communicates only through router

## Build, test, bench

```sh
cargo test --workspace
cargo clippy --workspace                            # zero warnings
cd crates/conduit-core && cargo bench               # all benchmarks
cd crates/conduit-core && cargo bench -- comparison  # Tauri vs Level 1 vs Level 2
```

## Relationship to tauri-wire

- `tauri-wire` = codec only (9-byte frame, transport-agnostic)
- `tauri-conduit` = unified: codec (11-byte frame) + custom protocol + ring buffer + derive macros + drop-in API

## What NOT to do

- Don't add network transports -- the custom protocol is intentionally in-process only
- Don't skip invoke key authentication
- Don't add async/tokio to conduit-core -- handlers are synchronous by design
- Don't add typed pub-sub or messaging abstractions -- the ring buffer + events is the streaming primitive; users layer their own semantics on top
- Don't reference `TransportTier` or negotiate -- these concepts have been removed
