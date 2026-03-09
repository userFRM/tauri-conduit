# tauri-conduit

[![CI](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml/badge.svg)](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

Binary IPC for Tauri v2 -- bypasses the webview JSON bridge with an in-process custom protocol.

---

## Why

Tauri's built-in `invoke()` serializes every call as JSON, round-trips it through the webview bridge, and deserializes on the other side. For most apps this is fine. For high-frequency binary data -- financial ticks, audio buffers, sensor telemetry, game state -- the serialization overhead becomes the bottleneck.

conduit replaces that path with a `conduit://` custom protocol handler that runs in-process, in the same address space as the Rust backend. No network surface, no JSON, no webview bridge.

### What conduit eliminates

| | Tauri `invoke()` | conduit `invoke()` |
|---|---|---|
| **JS → Rust** | `JSON.stringify()` → webview bridge | `fetch("conduit://...")` → in-process handler |
| **Rust → JS** | serde_json → webview bridge | raw bytes → `ArrayBuffer` |
| **Serialization** | JSON (text, allocations, parsing) | Fixed-layout binary (LE primitives, zero-parse) |
| **Transport** | Webview message bridge | Custom protocol (same address space) |
| **Streaming** | Poll or manual event wiring | Event-driven push + binary drain |

### Codec comparison (JSON vs binary)

Run `cd crates/conduit-core && cargo bench -- comparison` to see on your hardware. Representative numbers:

| Operation | JSON (serde) | Binary (conduit) | Speedup |
|---|---|---|---|
| Encode 25B struct | 105 ns | 5.8 ns | **18x** |
| Decode 25B struct | 122 ns | 11 ns | **11x** |
| Roundtrip 25B struct | 260 ns | 20 ns | **13x** |
| Dispatch echo (encode→handler→decode) | 309 ns | 75 ns | **4x** |
| Roundtrip 1 KB raw payload | 14.5 us | 19 ns | **760x** |
| Roundtrip 64 KB raw payload | 856 us | 1.1 us | **780x** |

These measure the serialization layer that conduit replaces. The full IPC round-trip also includes the Tauri custom protocol handler, which varies by platform and webview engine but adds constant overhead to both paths.

## Quick Start (Rust)

```rust
// src-tauri/src/main.rs
tauri::Builder::default()
    .plugin(
        conduit_tauri::init()
            .command("ping", |_| b"pong".to_vec())
            .command("get_ticks", handle_ticks)
            .channel("telemetry")   // ring buffer for streaming
            .build()
    )
    .run(tauri::generate_context!())
    .unwrap();
```

Push data to the frontend from any Rust thread:

```rust
let state: tauri::State<'_, ConduitState<R>> = app.state();
state.push("telemetry", &tick_bytes)?;  // writes to ring buffer + emits event
```

## Quick Start (TypeScript)

Drop-in replacement for `@tauri-apps/api/core`:

```typescript
import { invoke } from '@tauri-conduit/client';

const result = await invoke('get_ticks', { symbol: 'AAPL' });
```

Full control with binary payloads:

```typescript
import { connect } from '@tauri-conduit/client';

const conduit = await connect();
const buf = await conduit.invokeBinary('raw_data', new Uint8Array([1, 2, 3]));
```

## Streaming

conduit supports both push (event-driven) and pull (polling) access to streaming data. Use whichever model fits your app:

### Push (no polling)

```typescript
import { subscribe } from '@tauri-conduit/client';

const unsub = await subscribe('telemetry', (buf) => {
  // Called automatically when Rust pushes data.
  // Parse buf: [u32 LE frame_count] [u32 LE len][bytes] per frame
});
```

### Pull (user-controlled)

```typescript
import { drain } from '@tauri-conduit/client';

// Call whenever you want the latest data
const buf = await drain('telemetry');
```

### How it works

1. **Rust side** -- `state.push("channel_name", &bytes)` writes a binary frame into the ring buffer and emits a `conduit:data-available` Tauri event.
2. **JS side** -- `subscribe()` listens for the event and auto-drains via `conduit://`. Or call `drain()` manually whenever you want.

The ring buffer applies lossy back-pressure -- if the consumer falls behind, the oldest frames are dropped. This is intentional for real-time data where the latest value is always more relevant than blocking the producer.

## Binary Codec

Derive macros for binary serialization with no parsing overhead:

```rust
use conduit_derive::{WireEncode, WireDecode};

#[derive(WireEncode, WireDecode)]
struct MarketTick {
    timestamp: i64,    // 8 bytes, LE
    price: f64,        // 8 bytes, LE
    volume: f64,       // 8 bytes, LE
    side: u8,          // 1 byte
}
// Total: 25 bytes on the wire. No JSON, no parsing, no allocation.
```

Supported types: `u8`-`u64`, `i8`-`i64`, `f32`, `f64`, `bool`, `Vec<u8>`, `String`.

## Architecture

| Layer | Crate | Purpose |
|---|---|---|
| Core | `conduit-core` | Codec, dispatch table, ring buffer |
| Derive | `conduit-derive` | `#[derive(WireEncode, WireDecode)]` |
| Plugin | `conduit-tauri` | Tauri v2 plugin with `conduit://` protocol |
| Client | `@tauri-conduit/client` | TypeScript `invoke()` / `subscribe()` / `drain()` |

Everything runs in-process. The `conduit://` URI scheme is handled by Tauri's custom protocol mechanism -- the webview engine intercepts the `fetch()` call and routes it to a Rust closure in the same process. No TCP, no IPC pipes, no shared memory files.

## Security

- **No network surface** -- everything runs in the same address space. No ports, no sockets, no endpoints.
- **Per-launch invoke key** -- a 32-byte random key generated at startup, validated on every custom protocol request using constant-time comparison (`subtle` crate). Sent via `X-Conduit-Key` header, never in URLs.
- **Capability-based ACL** -- integrates with Tauri's permission system for command authorization.
- **CSP compliance** -- the custom protocol handler does not require Content Security Policy exceptions.
- **Panic isolation** -- handler panics are caught and returned as 500 errors, never crash the protocol handler.

## Benchmarks

Four benchmark suites in `crates/conduit-core`:

- **comparison** -- head-to-head JSON (serde) vs binary (conduit) at the codec + dispatch layer
- **codec** -- frame header, frame wrap/unwrap, primitive and collection wire encoding
- **ringbuf** -- push throughput, push+pop roundtrip, drain, multi-producer contention
- **dispatch** -- single handler dispatch, lookup across 100 registered commands

```sh
cd crates/conduit-core && cargo bench
```

## Project Structure

```
tauri-conduit/
  crates/
    conduit-core/           Core library (codec, dispatch, ring buffer)
    conduit-derive/         Proc macros (WireEncode, WireDecode)
    conduit-tauri/          Tauri v2 plugin
  packages/
    tauri-conduit/          TypeScript client (@tauri-conduit/client)
```

## Installation

```sh
cargo add conduit-core                    # core library
cargo add conduit-tauri                   # tauri v2 plugin
npm install @tauri-conduit/client         # typescript client
```

Or as workspace dependencies:

```toml
[workspace.dependencies]
conduit-core  = { path = "crates/conduit-core" }
conduit-tauri = { path = "crates/conduit-tauri" }
```

## Relationship to tauri-wire

`tauri-wire` is a standalone binary codec (9-byte frame header, transport-agnostic). `tauri-conduit` is the full stack: 11-byte frame header, custom protocol transport, ring buffer, derive macros, and a drop-in TypeScript client. Use `tauri-conduit` for new projects; use `tauri-wire` independently if you only need a lightweight binary codec.

## Contributing

Contributions welcome. Run the test suite before submitting:

```sh
cargo test --workspace
cargo clippy --workspace   # zero warnings
```

## License

Licensed under either of

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.
