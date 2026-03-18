# tauri-conduit

[![CI](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml/badge.svg)](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/tauri-plugin-conduit.svg)](https://crates.io/crates/tauri-plugin-conduit)
[![npm](https://img.shields.io/npm/v/tauri-plugin-conduit.svg)](https://www.npmjs.com/package/tauri-plugin-conduit)
[![npm downloads](https://img.shields.io/npm/dm/tauri-plugin-conduit.svg)](https://www.npmjs.com/package/tauri-plugin-conduit)
[![docs.rs](https://docs.rs/tauri-plugin-conduit/badge.svg)](https://docs.rs/tauri-plugin-conduit)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

**Optional IPC path for Tauri apps that want a fetch-based transport with the same process-local runtime model. One import change, zero config, binary support when you need it.**

```diff
- import { invoke } from '@tauri-apps/api/core';
+ import { invoke } from 'tauri-plugin-conduit';
```

---

## Architecture

<img src="docs/images/architecture.png" alt="tauri-conduit architecture" width="100%">

---

## Performance

Tauri's built-in `invoke()` is a solid default and a good fit for many apps. conduit is aimed at cases where transport overhead, built-in binary handling, or high-rate streaming become meaningful parts of the profile.

All numbers are **Rust dispatch layer only** (excludes WebView bridge, `fetch()`, JS parsing). See [BENCHMARKS.md](BENCHMARKS.md) for full methodology.

<img src="docs/images/payload-scaling.png" alt="Tauri invoke vs conduit — roundtrip latency by payload size" width="100%">

| Payload | Tauri invoke | conduit L1 (JSON) | conduit L2 (binary) |
|---|---|---|---|
| 25B struct | 722 ns | 330 ns (**2.2x**) | 80 ns (**9x**) |
| ~1 KB | 8.1 µs | 7.6 µs (**1.1x**) | 1.0 µs (**8x**) |
| 64 KB | 2.27 ms | 834 µs (**2.7x**) | 202 µs (**11x**) |

### Why L1 only changes the picture slightly at 1 KB

<img src="docs/images/bottleneck-breakdown.png" alt="Where does the time go? — Tauri invoke latency breakdown" width="100%">

At small payloads (25B), WebView transport overhead is 54% of total cost, so the custom protocol path helps more. At 1 KB, JSON serialization is 82% of cost and transport is only 6%, so L1 stays closer to Tauri's built-in IPC. L2 binary skips JSON entirely and therefore changes the tradeoff more substantially.

> Measured with [criterion](https://bheisler.github.io/criterion.rs/) on Intel i7-10700KF @ 3.80 GHz. Run `cd crates/conduit-core && cargo bench -- comparison` to see numbers on your hardware.

### Two levels of optimization

**Level 1 (drop-in)** — `invoke()` is API-compatible with Tauri's built-in invoke. It still uses JSON, but routes through conduit's in-process custom protocol and uses [sonic-rs](https://github.com/cloudwego/sonic-rs) (SIMD-accelerated) to deserialize directly to the target type in one step, skipping serde_json's intermediate `Value` conversion.

**Level 2 (binary)** — `invokeBinary()` avoids JSON entirely. Raw bytes in, raw bytes out. Use `#[derive(Encode, Decode)]` for typed binary structs, or pass raw `Uint8Array` for full control.

## Getting Started

### 1. Install

```sh
# Rust (in your src-tauri directory)
cargo add tauri-plugin-conduit

# TypeScript
npm install tauri-plugin-conduit
```

### 2. Register your commands (Rust)

Use `#[tauri_conduit::command]` for named parameters, `State<T>`/`AppHandle`/`Window`/`Webview` injection, `Result<T, E>` returns, and async:

```rust
use tauri_conduit::command;
use tauri::State;

struct AppState { app_name: String }

#[command]
fn get_ticks(symbol: String, limit: u32) -> Vec<Tick> {
    db::query_ticks(&symbol, limit)
}

#[command]
fn place_order(state: State<'_, AppState>, symbol: String, qty: f64) -> Result<OrderId, String> {
    broker::submit(&state.app_name, &symbol, qty).map_err(|e| e.to_string())
}

#[command]
async fn fetch_data(url: String) -> Result<Vec<u8>, String> {
    reqwest::get(&url).await.map_err(|e| e.to_string())?
        .bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}
```

Register handlers in your Tauri builder using `handler!()` to resolve command functions:

```rust
// src-tauri/src/main.rs
use tauri_conduit::handler;

tauri::Builder::default()
    .plugin(
        tauri_plugin_conduit::init()
            .handler("get_ticks", handler!(get_ticks))
            .handler("place_order", handler!(place_order))
            .handler("fetch_data", handler!(fetch_data))
            .channel("telemetry")
            .channel_ordered("events")
            .build()
    )
    .run(tauri::generate_context!())
    .unwrap();
```

Six handler registration methods are available:
- `handler(name, handler)` -- **recommended.** Use with `#[tauri_conduit::command]` + `handler!()`. Supports named parameters, `State<T>`, `AppHandle`, `Window`/`WebviewWindow`, `Webview` injection, `Result<T, E>`, and async. Full `#[tauri::command]` parity.
- `handler_raw(name, closure)` -- legacy closure-based handler (`Fn(Vec<u8>, &dyn Any) -> Result<Vec<u8>, Error>`). Use for backward compatibility when migrating from v1.
- `command_json(name, handler)` -- JSON in, JSON out. Single argument type (no named parameters, no State, no async).
- `command_json_result(name, handler)` -- same as above, but the handler returns `Result<R, E>`. Errors are propagated to the caller.
- `command_binary(name, handler)` -- binary in, binary out. The handler takes a type implementing `Decode` and returns a type implementing `Encode`. No JSON involved.
- `command(name, handler)` -- raw `Vec<u8>` in, `Vec<u8>` out. Full control, no automatic (de)serialization.

### 3. Call from the frontend

```typescript
import { invoke } from 'tauri-plugin-conduit';

const result = await invoke('get_ticks', { symbol: 'AAPL' });
```

### Parameter naming

Like Tauri's `#[tauri::command]`, tauri-conduit's `#[command]` macro automatically converts Rust snake_case parameter names to camelCase in JSON. A Rust parameter `user_name: String` is passed as `{ userName: "Alice" }` from JavaScript.

## Streaming

conduit includes built-in streaming from Rust to JavaScript via ring buffers and Tauri events.

> **Note:** The default `.channel("name")` creates a **lossy** ring buffer -- oldest frames are silently dropped when the buffer is full. Use `.channel_ordered("name")` for guaranteed-delivery ordered channels. Both channel types default to a 64 KB budget; use `channel_ordered_with_capacity(0)` only if you explicitly want an unbounded ordered queue.

Two channel types are available:

- **`channel(name)`** -- lossy. When the buffer is full, the oldest frames are silently dropped. Use for telemetry, game state, and real-time data where freshness matters more than completeness.
- **`channel_ordered(name)`** -- ordered, no drops. When the buffer is full, `push()` returns an error (backpressure). Use for transaction logs, control messages, and data that must arrive intact and in order.

Both types default to 64 KB capacity. Use `channel_with_capacity()` or `channel_ordered_with_capacity()` to override.

**Rust side** -- register channels and push data:

```rust
tauri_plugin_conduit::init()
    .channel("telemetry")               // lossy streaming channel
    .channel_ordered("events")          // ordered, no-drop channel
    .build()

// Later, from any thread:
let state: tauri::State<'_, tauri_plugin_conduit::PluginState<R>> = app.state();
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

Under the hood, Rust writes frames into a ring buffer and emits a `conduit:data-available` event. The JS client listens for the event and fetches data through the custom protocol. Behavior when the buffer is full depends on the channel type: lossy channels drop the oldest frames; ordered channels return an error to the producer.

```mermaid
flowchart LR
    subgraph Rust
        P["Any thread"] -- "push(channel, bytes)" --> RB["Wire Buffer"]
        RB -- "emits" --> EV["conduit:data-available"]
    end

    subgraph Frontend
        EV -- "event listener" --> S["subscribe() callback"]
        S -- "fetch conduit://drain/" --> RB
        RB -- "binary frames" --> S
    end

    style RB fill:#f59e0b,stroke:#d97706,color:#000
    style EV fill:#3b82f6,stroke:#2563eb,color:#fff
```

## How it works

conduit registers a `conduit://` custom protocol with Tauri. When your frontend calls `invoke()`, it uses `fetch("conduit://...")` instead of going through the webview message bridge. The request stays in the same process -- no network, no IPC pipes.

### Tauri's built-in IPC path

```mermaid
sequenceDiagram
    participant JS as Frontend (JS)
    participant WV as Webview Bridge
    participant RT as Tauri Runtime
    participant H as Handler

    JS->>WV: JSON.stringify(args)
    WV->>RT: postMessage (IPC)
    RT->>RT: JSON bytes → serde_json::Value
    RT->>H: Value → T (second deserialize)
    H->>RT: T → Value (serialize)
    RT->>RT: Value → JSON bytes
    RT->>WV: IPC response
    WV->>JS: JSON.parse(result)
```

### conduit Level 1 (drop-in) -- same JSON, fewer steps

```mermaid
sequenceDiagram
    participant JS as Frontend (JS)
    participant CP as conduit:// Protocol
    participant H as Handler

    JS->>CP: fetch("conduit://…", JSON body)
    Note over CP: JSON bytes → T (single step)
    CP->>H: Typed struct directly
    H->>CP: T → JSON bytes
    CP->>JS: Response body
```

### conduit Level 2 (binary) -- no JSON anywhere

```mermaid
sequenceDiagram
    participant JS as Frontend (JS)
    participant CP as conduit:// Protocol
    participant H as Handler

    JS->>CP: fetch("conduit://…", raw bytes)
    Note over CP: Binary passthrough — no JSON parsing
    CP->>H: Raw bytes (Vec of u8)
    H->>CP: Raw bytes
    CP->>JS: ArrayBuffer
```

**Why Level 1 can still help even though it still uses JSON:** Tauri's built-in invoke deserializes JSON into an intermediate `serde_json::Value`, then converts that value into your typed struct. conduit uses [sonic-rs](https://github.com/cloudwego/sonic-rs) (SIMD-accelerated JSON) to deserialize directly from bytes to the target struct in one step, and routes through an in-process custom protocol instead of the webview message bridge.

| | Tauri `invoke()` | conduit `invoke()` | conduit `invokeBinary()` |
|---|---|---|---|
| **Transport** | Webview bridge | Custom protocol (in-process) | Custom protocol (in-process) |
| **Rust-side JSON** | serde_json: bytes -> Value -> T (double parse) | sonic-rs: bytes -> T (single parse, SIMD) | No JSON |
| **Handler registration** | `#[tauri::command]`: named params, `State<T>`, `Result<T,E>`, async | `#[tauri_conduit::command]` + `handler!()`: named params, `State<T>`, `AppHandle`, `Window`/`Webview`, `Result<T,E>`, sync + async (tokio-spawned) | `command_binary(name, fn)`: Encode/Decode types, sync only |
| **Streaming** | Manual event wiring | Built-in push + drain (lossy and ordered) | Built-in push + drain (lossy and ordered) |
| **Network surface** | None | None | None |

## Typed binary codec (optional)

For binary mode, conduit provides derive macros to define compact binary formats. This is entirely optional -- `invoke()` works without it.

```rust
use conduit_derive::{Encode, Decode};

#[derive(Encode, Decode)]
struct MarketTick {
    timestamp: i64,
    price: f64,
    volume: f64,
    side: u8,
}
// 25 bytes on the wire. No schema, no parsing.
```

Supported types: `u8`-`u64`, `i8`-`i64`, `f32`, `f64`, `bool`, `Vec<u8>`, `String`, `Bytes` (newtype for efficient bulk `Vec<u8>` encode/decode).

## Security

Everything runs in-process -- no ports, no sockets, no network endpoints.

- **Authentication** -- conduit uses a per-launch 32-byte invoke key (constant-time validated) as its access control mechanism. This is simpler than Tauri's per-command ACL: any webview with the invoke key can call any registered command. For multi-window apps requiring granular per-command access control, stick with Tauri's built-in IPC.
- **CSP safe** -- no Content Security Policy exceptions required.
- **Panic isolation** -- if a handler panics, conduit catches it and returns a clean error. The app keeps running.

**Threat model**: The invoke key protects against cross-origin requests (other tabs, browser extensions intercepting network requests). It does **not** protect against malicious JavaScript running in the same WebView context -- any JS with access to the page can obtain the key via `fetch()` interception or DevTools. This matches Tauri's own trust model: the WebView JS context is trusted. Disable DevTools in production builds.

## How it fits alongside Tauri's built-in IPC

If Tauri's built-in IPC already meets your needs, it remains the simplest default. conduit is meant for projects that want a compatible `invoke()` surface, built-in binary request/response support, or higher-throughput streaming primitives.

`#[tauri_conduit::command]` is designed to stay close to `#[tauri::command]`: named parameters (camelCase conversion included), `State<T>`, `AppHandle`, `Window`/`Webview` injection, async, and `Result<T, E>`.

For streaming, conduit provides high-throughput ring buffer channels (`subscribe()`/`drain()`). For per-invocation progress callbacks, use `AppHandle::emit()` directly — handlers have full access to Tauri's event system via `AppHandle` injection.

## Project layout

```
tauri-conduit/
  crates/
    conduit/                   Facade crate (re-exports #[command], Encode, Decode)
    conduit-core/              Core library (codec, router, ring buffer)
    conduit-derive/            Proc macros (Encode, Decode, #[command])
    tauri-plugin-conduit/      Tauri v2 plugin
  packages/
    tauri-plugin-conduit/      TypeScript client (tauri-plugin-conduit)
```

The `tauri-conduit` facade crate (6 lines) exists solely to enable the `#[tauri_conduit::command]` attribute path. It re-exports the proc macro and core types.

### Testing

```sh
cargo test --workspace                                    # core + derive crates
cargo test --manifest-path crates/tauri-plugin-conduit/Cargo.toml  # plugin unit tests
cd packages/tauri-plugin-conduit && npm test                              # TS codec tests
```

> **Note:** There are no end-to-end integration tests that exercise the full Tauri->conduit->WebView roundtrip. The test suite covers unit-level Rust dispatch, codec correctness, and TypeScript wire format -- not the custom protocol transport under a running Tauri app.

### Dependencies

conduit-core depends on `serde` and `sonic-rs` unconditionally. These are required by the Router JSON handler methods, the `ConduitHandler` trait, and the `Error::Serialize` variant. The pure binary codec (`Encode`/`Decode`, `RingBuffer`) does not use JSON at runtime, but these dependencies are not feature-gated because the handler system is a core part of conduit's purpose.

## Contributing

Contributions welcome. Run the test suite before submitting:

```sh
cargo test --workspace
cargo clippy --workspace
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) at your option.
