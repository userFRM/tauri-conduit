# tauri-conduit

[![CI](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml/badge.svg)](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

**A faster drop-in replacement for Tauri's `invoke()`.**

Swap one import and your Tauri v2 app communicates through an in-process custom protocol instead of the default webview bridge. Same API, same behavior -- faster transport.

```diff
- import { invoke } from '@tauri-apps/api/core';
+ import { invoke } from '@tauri-conduit/client';
```

For hot paths where every microsecond matters, switch to `invokeBinary()` to skip JSON entirely and get up to **780x faster** serialization.

---

## "Does a Tauri app really need this?"

If your app sends a button click and gets a string back, probably not -- Tauri's built-in IPC is fine for that.

But Tauri is increasingly used for apps where performance actually matters: trading terminals streaming real-time market data, audio/video tools processing buffers every frame, IoT dashboards ingesting sensor telemetry, game overlays syncing state at 60+ fps. In these cases, the IPC layer between your frontend and backend becomes the bottleneck -- not Rust, not your logic, just the bridge.

conduit exists so you don't have to choose between Tauri's developer experience and the performance your use case demands.

## Two levels of optimization

conduit gives you a progressive optimization path -- start easy, go deeper where it matters.

### Level 1: Drop-in replacement (faster transport)

`invoke()` is API-compatible with Tauri's built-in invoke. It still uses JSON for argument encoding, but routes through conduit's in-process custom protocol instead of the webview bridge. Less overhead on the transport side, zero code changes.

```typescript
import { invoke } from '@tauri-conduit/client';

// Same API you already know from Tauri
const result = await invoke('get_ticks', { symbol: 'AAPL' });
```

### Level 2: Binary mode (eliminates JSON entirely)

`invokeBinary()` sends and receives raw bytes. Pair it with the binary codec on the Rust side and JSON is completely out of the picture. This is where the big performance wins are.

```typescript
import { connect } from '@tauri-conduit/client';

const conduit = await connect();
const buf = await conduit.invokeBinary('raw_data', new Uint8Array([1, 2, 3]));
```

### Performance comparison

These numbers measure the serialization layer -- the part conduit's binary mode eliminates:

| Payload | JSON (serde) | Binary (conduit) | Speedup |
|---|---|---|---|
| Small struct (25 bytes) | 260 ns | 20 ns | **13x** |
| Medium payload (1 KB) | 14.5 us | 19 ns | **760x** |
| Large payload (64 KB) | 856 us | 1.1 us | **780x** |

> Run `cd crates/conduit-core && cargo bench -- comparison` to see numbers on your hardware. The larger the payload, the bigger the gap.

## Getting Started

### 1. Install

```sh
# Rust (in your src-tauri directory)
cargo add conduit-tauri

# TypeScript
npm install @tauri-conduit/client
```

### 2. Register your commands (Rust)

```rust
// src-tauri/src/main.rs
tauri::Builder::default()
    .plugin(
        conduit_tauri::init()
            .command("ping", |_| b"pong".to_vec())
            .command("get_ticks", handle_ticks)
            .build()
    )
    .run(tauri::generate_context!())
    .unwrap();
```

Commands receive raw bytes (`Vec<u8>`) and return raw bytes. For JSON-style usage, deserialize the payload in your handler. For binary mode, use the wire codec directly.

### 3. Call from the frontend

```typescript
import { invoke } from '@tauri-conduit/client';

const result = await invoke('get_ticks', { symbol: 'AAPL' });
```

## Streaming

conduit includes built-in streaming from Rust to JavaScript with no polling required.

**Rust side** -- register a channel and push data to it:

```rust
conduit_tauri::init()
    .channel("telemetry")               // register a streaming channel
    .build()

// Later, from any thread:
let state: tauri::State<'_, ConduitState<R>> = app.state();
state.push("telemetry", &bytes)?;       // auto-notifies the frontend
```

**JS side** -- subscribe for automatic delivery, or pull manually:

```typescript
// Option A: automatic (no polling, event-driven)
const unsub = await subscribe('telemetry', (buf) => {
  // Called each time Rust pushes data
});

// Option B: manual (pull whenever you want)
const buf = await drain('telemetry');
```

Under the hood, Rust writes frames into a ring buffer and emits a lightweight event. The JS client listens for the event and fetches the data through the custom protocol. If the consumer falls behind, the oldest frames are dropped -- latest data always wins, and the producer never blocks.

## How it works

conduit registers a `conduit://` custom protocol with Tauri. When your frontend calls `invoke()`, it uses `fetch("conduit://...")` instead of going through the webview message bridge. The request stays in the same process -- no network, no IPC pipes.

| | Tauri `invoke()` | conduit `invoke()` | conduit `invokeBinary()` |
|---|---|---|---|
| **Transport** | Webview bridge | Custom protocol (in-process) | Custom protocol (in-process) |
| **Serialization** | JSON both sides | JSON both sides | None (raw bytes) |
| **Streaming** | Manual event wiring | Built-in push + drain | Built-in push + drain |
| **Network surface** | None | None | None |

## Typed binary codec (optional)

For binary mode, conduit provides derive macros to define compact wire formats. This is entirely optional -- `invoke()` works without it.

```rust
use conduit_derive::{WireEncode, WireDecode};

#[derive(WireEncode, WireDecode)]
struct MarketTick {
    timestamp: i64,
    price: f64,
    volume: f64,
    side: u8,
}
// 25 bytes on the wire. No schema, no parsing.
```

Supported types: `u8`-`u64`, `i8`-`i64`, `f32`, `f64`, `bool`, `Vec<u8>`, `String`.

## Security

Everything runs in-process -- no ports, no sockets, no network endpoints.

- **Per-launch auth key** -- a random 32-byte key is generated each time your app starts. Every request is validated with constant-time comparison. Leaked keys expire when the app restarts.
- **Tauri permissions** -- integrates with Tauri's built-in capability system for command authorization.
- **CSP safe** -- no Content Security Policy exceptions required.
- **Panic isolation** -- if a handler panics, conduit catches it and returns a clean error. The app keeps running.

## Project layout

```
tauri-conduit/
  crates/
    conduit-core/         Core library (codec, dispatch, ring buffer)
    conduit-derive/       Derive macros (WireEncode, WireDecode)
    conduit-tauri/        Tauri v2 plugin
  packages/
    tauri-conduit/        TypeScript client (@tauri-conduit/client)
```

## Contributing

Contributions welcome. Run the test suite before submitting:

```sh
cargo test --workspace
cargo clippy --workspace
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) at your option.
