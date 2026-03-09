# tauri-plugin-conduit

[![Crates.io](https://img.shields.io/crates/v/tauri-plugin-conduit.svg)](https://crates.io/crates/tauri-plugin-conduit)
[![docs.rs](https://docs.rs/tauri-plugin-conduit/badge.svg)](https://docs.rs/tauri-plugin-conduit)
[![CI](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml/badge.svg)](https://github.com/userFRM/tauri-conduit/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/tauri-plugin-conduit.svg)](https://github.com/userFRM/tauri-conduit#license)

Tauri v2 plugin for [conduit](https://github.com/userFRM/tauri-conduit) -- binary IPC over the `conduit://` custom protocol.

Part of the [tauri-conduit](https://github.com/userFRM/tauri-conduit) workspace.

## Usage

```rust
tauri::Builder::default()
    .plugin(
        tauri_plugin_conduit::init()
            .command("ping", |_| b"pong".to_vec())
            .channel("telemetry")
            .build()
    )
    .run(tauri::generate_context!())
    .unwrap();
```

## Security

- Per-launch 32-byte invoke key with constant-time validation
- No network surface -- everything runs in-process
- Integrates with Tauri's capability-based permission system

See the [workspace README](https://github.com/userFRM/tauri-conduit) for full documentation, streaming examples, and benchmark numbers.

## License

Licensed under either of [MIT](https://github.com/userFRM/tauri-conduit/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/userFRM/tauri-conduit/blob/master/LICENSE-APACHE) at your option.
