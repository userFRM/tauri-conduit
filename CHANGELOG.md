# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-03-09

### Added

- Binary frame codec with 11-byte header (`conduit-core`)
- `WireEncode` / `WireDecode` traits for primitive types, `Vec<u8>`, `String`
- `#[derive(WireEncode, WireDecode)]` proc macros (`conduit-derive`)
- Synchronous `DispatchTable` for named command handlers
- In-process `ConduitRingBuffer` with lossy back-pressure
- Tauri v2 plugin with `conduit://` custom protocol (`conduit-tauri`)
- TypeScript client with `invoke()`, `subscribe()`, `drain()` (`@tauri-conduit/client`)
- Per-launch invoke key with constant-time validation (`X-Conduit-Key` header)
- Panic isolation via `catch_unwind` in the protocol handler
- Criterion benchmarks: codec, ring buffer, dispatch, and JSON-vs-binary comparison
