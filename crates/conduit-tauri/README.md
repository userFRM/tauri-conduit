# conduit-tauri

Tauri v2 plugin for [conduit](https://github.com/userFRM/tauri-conduit) — binary IPC over the `conduit://` custom protocol.

## Usage

```rust
tauri::Builder::default()
    .plugin(
        conduit_tauri::init()
            .command("ping", |_| b"pong".to_vec())
            .channel("telemetry")
            .build()
    )
    .run(tauri::generate_context!())
    .unwrap();
```

## Security

- Per-launch 32-byte invoke key with constant-time validation
- No network surface — everything runs in-process
- Integrates with Tauri's capability-based permission system

See the [workspace README](https://github.com/userFRM/tauri-conduit) for full documentation.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
