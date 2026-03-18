# conduit-derive

[![Crates.io](https://img.shields.io/crates/v/conduit-derive.svg)](https://crates.io/crates/conduit-derive)
[![docs.rs](https://docs.rs/conduit-derive/badge.svg)](https://docs.rs/conduit-derive)
[![CI](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml/badge.svg)](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/conduit-derive.svg)](https://github.com/userFRM/tauri-conduit#license)

Proc macros for [conduit-core](https://crates.io/crates/conduit-core): `#[derive(Encode, Decode)]`, `#[command]`, and `handler!()`.

Part of the [tauri-conduit](https://github.com/userFRM/tauri-conduit) workspace (v2.1.1).

## Usage

### Binary codec

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

Supported field types: `u8`-`u64`, `i8`-`i64`, `f32`, `f64`, `bool`, `Vec<u8>`, `String`, `Bytes`.

The `Decode` derive automatically generates a `MIN_SIZE` constant for upfront bounds checking.

### Command handlers

```rust
use conduit::{command, handler};

#[command]
fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

// Register with handler!() macro:
// .handler("greet", handler!(greet))
```

`#[command]` supports named parameters, `State<T>` injection, `AppHandle`, `Window`/`Webview` injection, `Result<T, E>` returns, and `async` functions.

See the [workspace README](https://github.com/userFRM/tauri-conduit) for full documentation.

## License

Licensed under either of [MIT](https://github.com/userFRM/tauri-conduit/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/userFRM/tauri-conduit/blob/master/LICENSE-APACHE) at your option.
