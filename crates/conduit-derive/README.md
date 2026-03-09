# conduit-derive

[![Crates.io](https://img.shields.io/crates/v/conduit-derive.svg)](https://crates.io/crates/conduit-derive)
[![docs.rs](https://docs.rs/conduit-derive/badge.svg)](https://docs.rs/conduit-derive)
[![CI](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml/badge.svg)](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/conduit-derive.svg)](https://github.com/userFRM/tauri-conduit#license)

Derive macros for [conduit-core](https://crates.io/crates/conduit-core): `WireEncode` and `WireDecode`.

Part of the [tauri-conduit](https://github.com/userFRM/tauri-conduit) workspace.

## Usage

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

Supported field types: `u8`-`u64`, `i8`-`i64`, `f32`, `f64`, `bool`, `Vec<u8>`, `String`.

See the [workspace README](https://github.com/userFRM/tauri-conduit) for full documentation.

## License

Licensed under either of [MIT](https://github.com/userFRM/tauri-conduit/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/userFRM/tauri-conduit/blob/master/LICENSE-APACHE) at your option.
