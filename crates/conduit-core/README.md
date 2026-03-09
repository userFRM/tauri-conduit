# conduit-core

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

See the [workspace README](https://github.com/userFRM/tauri-conduit) for full documentation.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
