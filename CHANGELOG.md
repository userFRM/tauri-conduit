# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-03-09

### Changed

- **Breaking:** Rename crate `conduit-tauri` to `tauri-plugin-conduit` (Tauri naming convention)
- **Breaking:** Rename TS package `@tauri-conduit/client` to `tauri-plugin-conduit`
- **Breaking:** Rename traits `WireEncode`/`WireDecode` to `Encode`/`Decode`
- **Breaking:** Rename `DispatchTable` to `Router`, `ConduitRingBuffer` to `RingBuffer`, `ConduitError` to `Error`
- **Breaking:** Rename `ConduitState` to `PluginState`, `ConduitPluginBuilder` to `PluginBuilder`
- **Breaking:** Rename methods: `dispatch` to `call`, `wire_encode` to `encode`, `wire_decode` to `decode`, `wire_size` to `encode_size`, `frame_wrap` to `frame_pack`, `frame_unwrap` to `frame_unpack`
- **Breaking:** Rename TS exports: `writeFrameHeader` to `packFrame`, `readFrameHeader` to `unpackFrame`
- **Breaking:** Rename `FrameHeader` field `transport_tier` to `reserved`
- Remove `onData` (redundant alias for `subscribe`)

### Added

- Release workflow with automated npm publish via OIDC provenance
- `workflow_call` trigger on CI for reuse from release workflow

## [0.1.0] - 2026-03-09

### Added

- Binary frame codec with 11-byte header (`conduit-core`)
- `Encode` / `Decode` traits for primitive types, `Vec<u8>`, `String`
- `#[derive(Encode, Decode)]` proc macros (`conduit-derive`)
- Synchronous `Router` for named command handlers
- In-process `RingBuffer` with lossy back-pressure
- Tauri v2 plugin with `conduit://` custom protocol (`tauri-plugin-conduit`)
- TypeScript client with `invoke()`, `subscribe()`, `drain()` (`tauri-plugin-conduit`)
- Per-launch invoke key with constant-time validation (`X-Conduit-Key` header)
- Panic isolation via `catch_unwind` in the protocol handler
- Criterion benchmarks: codec, ring buffer, dispatch, and JSON-vs-binary comparison
