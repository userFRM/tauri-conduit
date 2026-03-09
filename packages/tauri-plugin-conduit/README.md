# tauri-plugin-conduit

High-performance binary IPC client for Tauri v2.

## Install

```sh
npm install tauri-plugin-conduit
```

## Quick Start

Drop-in replacement for `@tauri-apps/api/core`:

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

- `invoke<T>(cmd, args?, options?)` — JSON request/response
- `invokeBinary(cmd, payload?, options?)` — binary request/response
- `subscribe(channel, callback)` — event-driven push streaming (no polling)
- `drain(channel)` — pull-based ring buffer access (user controls timing)
- `connect()` — explicit connection lifecycle

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
