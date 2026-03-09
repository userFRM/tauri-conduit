# conduit-derive

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
```

Supported field types: `u8`–`u64`, `i8`–`i64`, `f32`, `f64`, `bool`, `Vec<u8>`, `String`.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
