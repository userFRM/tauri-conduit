# tauri-plugin-conduit

[![npm](https://img.shields.io/npm/v/tauri-plugin-conduit.svg)](https://www.npmjs.com/package/tauri-plugin-conduit)
[![npm downloads](https://img.shields.io/npm/dm/tauri-plugin-conduit.svg)](https://www.npmjs.com/package/tauri-plugin-conduit)

Optional IPC client for Tauri apps that want a fetch-based transport with binary support and a minimal API change.

See the [main repository](https://github.com/userFRM/tauri-conduit) for full documentation, benchmarks, and architecture.

## Install

```sh
npm install tauri-plugin-conduit
```

## Quick Start

Compatible `invoke()` surface:

```typescript
import { invoke } from 'tauri-plugin-conduit';

const result = await invoke<MyType>('my_command', { key: 'value' });
```

Binary payloads:

```typescript
import { connect } from 'tauri-plugin-conduit';

const conduit = await connect();
const buf = await conduit.invokeBinary('raw_cmd', new Uint8Array([1, 2, 3]));
```

Push streaming:

```typescript
import { subscribe } from 'tauri-plugin-conduit';

const unsub = await subscribe('telemetry', (buf) => {
  // Parse binary frames from buf...
});
```

## API

- `invoke<T>(cmd, args?, options?)` — JSON request/response with a Tauri-compatible `invoke()` shape
- `invokeBinary(cmd, payload?, options?)` — binary request/response (raw bytes)
- `subscribe(channel, callback, onError?)` — event-driven push streaming (no polling)
- `drain(channel)` — pull-based ring buffer access (user controls timing)
- `connect()` — explicit connection lifecycle, returns a `Conduit` instance
- `resetConduit()` — force re-bootstrap (useful during development hot-reload)
- `parseDrainBlob(buf)` — parse drain wire format into zero-copy `Uint8Array` subarray views
- `WireWriter` — builder class for single-allocation binary encoding (pre-calculates total size, writes into one `ArrayBuffer`)
- `ConduitError` — structured error with `status`, `target`, and `message` fields

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
