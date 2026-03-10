# bench-app

End-to-end benchmark comparing Tauri's built-in `invoke()` with conduit's custom protocol transport.

## What it measures

The full roundtrip for each path:

```
Tauri:   JS JSON.stringify → postMessage → Tauri IPC bridge → serde_json::Value → T → handler
         → T → serde_json::Value → JSON string → postMessage → JS JSON.parse

Conduit: JS JSON.stringify → fetch(conduit://) → WebView bridge → sonic_rs::from_slice → T → handler
         → T → sonic_rs::to_vec → response → WebView bridge → fetch() response → JS JSON.parse
```

Three payload sizes: 25B, ~1KB, ~64KB — matching the Rust-layer criterion benchmarks.

## Running

```sh
cd examples/bench-app/src-tauri
cargo tauri dev
```

Click "Run Benchmark" in the UI. Results show median, P95, P99, min, max, and ops/sec for each path, with the conduit-vs-Tauri speedup ratio.

## Requirements

- Rust 1.85+
- System dependencies for Tauri v2 (webkit2gtk, etc.)
- `cargo-tauri` CLI: `cargo install tauri-cli`
