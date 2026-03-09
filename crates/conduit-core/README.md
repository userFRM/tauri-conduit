# conduit-core

[![Crates.io](https://img.shields.io/crates/v/conduit-core.svg)](https://crates.io/crates/conduit-core)
[![docs.rs](https://docs.rs/conduit-core/badge.svg)](https://docs.rs/conduit-core)
[![CI](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml/badge.svg)](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/conduit-core.svg)](https://github.com/userFRM/tauri-conduit#license)

Binary IPC core for Tauri v2: codec, dispatch table, and ring buffer.

Part of the [tauri-conduit](https://github.com/userFRM/tauri-conduit) workspace.

## Features

- **11-byte frame codec** with `WireEncode`/`WireDecode` traits for zero-parse binary serialization
- **Synchronous dispatch table** for named command handlers
- **In-process ring buffer** with lossy back-pressure for streaming

## Usage

```rust
use conduit_core::{DispatchTable, ConduitRingBuffer, frame_wrap, FrameHeader};

let table = DispatchTable::new();
table.register("ping", |_| b"pong".to_vec());

let response = table.dispatch("ping", vec![]).unwrap();
assert_eq!(response, b"pong");
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
