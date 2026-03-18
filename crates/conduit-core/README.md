# conduit-core

[![Crates.io](https://img.shields.io/crates/v/conduit-core.svg)](https://crates.io/crates/conduit-core)
[![docs.rs](https://docs.rs/conduit-core/badge.svg)](https://docs.rs/conduit-core)
[![CI](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml/badge.svg)](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/conduit-core.svg)](https://github.com/userFRM/tauri-conduit#license)

Binary IPC core for Tauri v2: codec, router, ring buffer, and ordered queue.

Part of the [tauri-conduit](https://github.com/userFRM/tauri-conduit) workspace (v2.1.1).

## Features

- **11-byte frame codec** with `Encode`/`Decode` traits for zero-parse binary serialization
- **`Bytes` newtype** for efficient bulk `Vec<u8>` encode/decode
- **`MIN_SIZE` constant** on `Decode` for upfront bounds checking (derived automatically)
- **Synchronous router** for named command handlers (raw, JSON, and binary)
- **In-process ring buffer** (`RingBuffer`) with lossy back-pressure and preformatted wire buffer (drain_all is single memcpy)
- **Ordered queue** (`Queue`) with guaranteed delivery, backpressure, and preformatted wire buffer
- **Channel abstraction** (`ChannelBuffer`) unifying lossy and ordered channels
- **Overflow guards** -- u32 truncation checks, frame_count cap, checked_add for 32-bit safety

## Key Types

| Type | Purpose |
|---|---|
| `Router` | Named synchronous command registry (`register`, `register_json`, `register_binary`, `call`) |
| `RingBuffer` | Thread-safe lossy circular buffer — oldest frames dropped on overflow |
| `Queue` | Thread-safe ordered buffer — backpressure when full, no data loss |
| `ChannelBuffer` | Enum wrapping `RingBuffer` (Lossy) or `Queue` (Reliable) with a unified push/drain API |
| `ConduitHandler` | Trait for sync/async command handlers, implemented by `#[command]`-generated structs |
| `HandlerResponse` | Enum: `Sync(Result<Vec<u8>, Error>)` or `Async(Pin<Box<dyn Future>>)` |
| `HandlerContext` | Context passed to handlers: app handle + optional webview label |
| `PushOutcome` | Enum: `Accepted(usize)` or `TooLarge` — opt-in via `push_checked()` |
| `FrameHeader` | 11-byte binary frame header for all conduit messages |
| `Encode` / `Decode` | Traits for fixed-layout binary serialization (`Decode` includes `MIN_SIZE` constant) |
| `Bytes` | Newtype for `Vec<u8>` with efficient bulk encode/decode |
| `Error` | Error types (`UnknownCommand`, `DecodeFailed`, `Serialize`, `UnknownChannel`, etc.) |

## Usage

```rust
use conduit_core::{Router, RingBuffer, Queue, ChannelBuffer, frame_pack, FrameHeader};

// Raw handler
let router = Router::new();
router.register("ping", |_| b"pong".to_vec());

let response = router.call("ping", vec![]).unwrap();
assert_eq!(response, b"pong");

// JSON handler
router.register_json("add", |args: (i32, i32)| args.0 + args.1);

// Binary handler (with Encode/Decode types)
// router.register_binary("process", |tick: MarketTick| tick);
```

## Benchmarks

Includes a head-to-head comparison benchmark against JSON (serde):

```sh
cargo bench -- comparison   # JSON vs binary codec
cargo bench                 # all benchmarks
```

See the [workspace README](https://github.com/userFRM/tauri-conduit) for full documentation and benchmark numbers.

## License

Licensed under either of [MIT](https://github.com/userFRM/tauri-conduit/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/userFRM/tauri-conduit/blob/master/LICENSE-APACHE) at your option.
