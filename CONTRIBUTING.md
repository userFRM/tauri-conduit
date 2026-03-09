# Contributing to tauri-conduit

Thank you for your interest in contributing!

## Getting Started

```sh
git clone https://github.com/userFRM/tauri-conduit.git
cd tauri-conduit
cargo test --workspace
cargo clippy --workspace
```

## Development

### Prerequisites

- Rust 1.85+ (edition 2024)
- Node.js 20+ (for TypeScript client)

### Project Structure

```
crates/
  conduit-core/     Core library (codec, router, ring buffer)
  conduit-derive/   Proc macros (WireEncode, WireDecode)
  conduit-tauri/    Tauri v2 plugin (requires Tauri app context)
packages/
  tauri-conduit/    TypeScript client (@tauri-conduit/client)
```

### Running Tests

```sh
cargo test --workspace          # All Rust tests
cargo clippy --workspace        # Lint check
cargo bench --workspace         # Benchmarks (requires criterion)
cd packages/tauri-conduit && npx tsc --noEmit  # TypeScript type check
```

### Code Style

- Follow existing patterns in the codebase
- All public items must have doc comments
- No `unsafe` code without justification
- Prefer `std::sync` over `tokio::sync` in conduit-core (handlers are synchronous)
- Use `unwrap_or_else(|e| e.into_inner())` for mutex poisoning recovery

### Architecture Principles

- **No network surface.** Everything runs in-process via Tauri's custom protocol handler.
- **No async in conduit-core.** Handlers are synchronous. Callers that need async should spawn internally.
- **Binary-first.** The wire format is fixed-width binary, not JSON. JSON is only used for Tauri command bootstrap.
- **Security by default.** Per-launch invoke key on every request, constant-time validation.

## Pull Requests

1. Fork the repo and create a feature branch
2. Add tests for new functionality
3. Ensure `cargo test --workspace` and `cargo clippy --workspace` pass
4. Keep PRs focused -- one feature or fix per PR

## License

By contributing, you agree that your contributions will be licensed under the MIT OR Apache-2.0 dual license.
