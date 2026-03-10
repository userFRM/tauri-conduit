# conduit

Facade crate for [tauri-conduit](https://github.com/userFRM/tauri-conduit) — re-exports `#[command]`, `handler!()`, and core types.

```rust
use conduit::{command, handler};

#[command]
fn greet(name: String) -> String {
    format!("Hello, {name}!")
}
```

See the [main repository](https://github.com/userFRM/tauri-conduit) for full documentation.

## License

MIT OR Apache-2.0
