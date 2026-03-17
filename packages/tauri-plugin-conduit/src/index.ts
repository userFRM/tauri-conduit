/**
 * tauri-plugin-conduit
 *
 * High-performance binary IPC client for Tauri v2.
 *
 * - **Request/response**: `invoke()` / `invokeBinary()` via custom protocol
 *   (`conduit://`) — in-process, no network surface.
 * - **Push streaming**: `subscribe()` — event-driven notification from Rust,
 *   auto-drain binary data via custom protocol. No polling.
 * - **Manual drain**: `drain()` for pull-based ring buffer access.
 *
 * @example
 * ```typescript
 * // Drop-in replacement for @tauri-apps/api/core invoke()
 * import { invoke } from 'tauri-plugin-conduit';
 * const result = await invoke<MyType>('my_command', { key: 'value' });
 * ```
 *
 * @example
 * ```typescript
 * // Push streaming — event-driven, no polling
 * import { subscribe } from 'tauri-plugin-conduit';
 * const unsub = await subscribe('market-data', (buf) => {
 *   // Parse binary frames from buf...
 * });
 * ```
 *
 * @example
 * ```typescript
 * // Full control with connect()
 * import { connect } from 'tauri-plugin-conduit';
 * const conduit = await connect();
 * const buf = await conduit.invokeBinary('raw_cmd', new Uint8Array([1, 2, 3]));
 * const unsub = await conduit.subscribe('telemetry', onData);
 * ```
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { bootstrap, type BootstrapInfo } from './negotiate.js';
import { createProtocolTransport, type ProtocolTransport } from './transport/protocol.js';
import { ConduitError } from './error.js';

const _encoder = new TextEncoder();
const _decoder = new TextDecoder();

// ── Re-exports ──────────────────────────────────────────────────

export { FRAME_HEADER_SIZE, PROTOCOL_VERSION, MsgType } from './codec/frame.js';
export type { FrameHeader } from './codec/frame.js';
export { packFrame, unpackFrame } from './codec/frame.js';
export { parseDrainBlob } from './codec/wire.js';
export { DEFAULT_TIMEOUT_MS } from './transport/protocol.js';
export type { ProtocolTransport } from './transport/protocol.js';
export type { BootstrapInfo } from './negotiate.js';
export { ConduitError } from './error.js';

// ── Public types ────────────────────────────────────────────────

/** Options for invoke calls — mirrors @tauri-apps/api/core InvokeOptions. */
export interface InvokeOptions {
  /** Per-request timeout in milliseconds. Defaults to DEFAULT_TIMEOUT_MS. */
  timeout?: number;
}

/** Function that unsubscribes from a channel when called. */
export type UnsubscribeFn = UnlistenFn;

export interface Conduit {
  /**
   * Send a command with JSON args and await a typed response.
   *
   * @note Commands returning `()` resolve to `null`. This matches Tauri's
   * `invoke()` behavior. For null-safety, use `invoke<MyType | null>(cmd)`.
   * No runtime type validation is performed on the response — `T` is an
   * unchecked assertion, same as Tauri's built-in invoke.
   *
   * @warning The response is parsed with `JSON.parse` which does not sanitize
   * `__proto__` keys. Do not spread the result into other objects without
   * validation if the command handler is untrusted.
   */
  invoke<T>(cmd: string, args?: Record<string, unknown>, options?: InvokeOptions): Promise<T>;
  /** Send a command with a raw binary payload and get raw bytes back. */
  invokeBinary(cmd: string, payload?: Uint8Array, options?: InvokeOptions): Promise<ArrayBuffer>;
  /**
   * Drain all buffered frames from an in-process ring buffer channel.
   *
   * Calls `conduit://localhost/drain/<channel>` and returns the raw binary
   * blob. The caller is responsible for parsing the wire format:
   *   [u32 LE frame_count] followed by [u32 LE len][bytes] per frame.
   */
  drain(channel: string): Promise<ArrayBuffer>;
  /**
   * Subscribe to push notifications from a ring buffer channel.
   *
   * When the Rust backend pushes data to the named channel, a lightweight
   * Tauri event triggers an automatic binary drain via the custom protocol.
   * The callback receives the raw binary blob on each push.
   *
   * Returns an unsubscribe function to stop listening.
   *
   * @example
   * ```typescript
   * const unsub = await conduit.subscribe('telemetry', (buf) => {
   *   // Parse binary frames from buf...
   * });
   * // Later: unsub();
   * ```
   */
  subscribe(
    channel: string,
    callback: (data: ArrayBuffer) => void,
    onError?: (err: Error) => void,
  ): Promise<UnsubscribeFn>;
  /** Available channel names from bootstrap. */
  readonly channels: string[];
  /** Release resources and unsubscribe all listeners. */
  disconnect(): void;
}

// ── buildConduit() ──────────────────────────────────────────────

/**
 * Build a Conduit backed by the custom protocol transport.
 */
function buildConduit(
  protocol: ProtocolTransport,
  bootstrapInfo: BootstrapInfo,
): Conduit {
  const unsubscribers: UnsubscribeFn[] = [];

  // Base headers sent with every conduit request (invoke key + webview label)
  const _baseHeaders: Record<string, string> = {
    'X-Conduit-Key': bootstrapInfo.invokeKey,
  };
  if (bootstrapInfo.webviewLabel) {
    _baseHeaders['X-Conduit-Webview'] = bootstrapInfo.webviewLabel;
  }

  async function drainChannel(channel: string): Promise<ArrayBuffer> {
    const url =
      `${bootstrapInfo.protocolBase}/drain/${encodeURIComponent(channel)}`;
    try {
      const response = await fetch(url, {
        method: 'GET',
        headers: _baseHeaders,
        signal: AbortSignal.timeout(30_000),
      });
      if (!response.ok) {
        const errorBody = await response.text();
        throw new ConduitError(response.status, channel, errorBody);
      }
      return response.arrayBuffer();
    } catch (err) {
      if (err instanceof DOMException && err.name === 'TimeoutError') {
        throw new ConduitError(408, channel, 'drain timed out');
      }
      throw err;
    }
  }

  async function subscribeToChannel(
    channel: string,
    callback: (data: ArrayBuffer) => void,
    onError?: (err: Error) => void,
  ): Promise<UnsubscribeFn> {
    const unlisten = await listen<string>(
      'conduit:data-available',
      async (event) => {
        if (event.payload !== channel) return;
        try {
          const buf = await drainChannel(channel);
          if (buf.byteLength > 0) {
            callback(buf);
          }
        } catch (err) {
          if (onError) {
            onError(err instanceof Error ? err : new Error(String(err)));
          } else {
            console.error(`conduit: drain error on channel "${channel}":`, err);
          }
        }
      },
    );
    const wrappedUnlisten = () => {
      const idx = unsubscribers.indexOf(wrappedUnlisten);
      if (idx !== -1) unsubscribers.splice(idx, 1);
      unlisten();
    };
    unsubscribers.push(wrappedUnlisten);
    // Initial drain to catch data pushed before the listener was registered.
    try {
      const buf = await drainChannel(channel);
      if (buf.byteLength > 0) {
        callback(buf);
      }
    } catch {
      // Ignore — channel may be empty
    }
    return wrappedUnlisten;
  }

  return {
    async invoke<T>(
      cmd: string,
      args?: Record<string, unknown>,
      options?: InvokeOptions,
    ): Promise<T> {
      const payload = JSON.stringify(args ?? {});

      const extra = _baseHeaders['X-Conduit-Webview'] ? { 'X-Conduit-Webview': _baseHeaders['X-Conduit-Webview'] } : undefined;
      const raw = await protocol.invoke(cmd, payload, options?.timeout, extra);

      const text = _decoder.decode(raw);

      if (text.length === 0) return undefined as T;

      return JSON.parse(text) as T;
    },

    async invokeBinary(
      cmd: string,
      payload?: Uint8Array,
      options?: InvokeOptions,
    ): Promise<ArrayBuffer> {
      const extra = _baseHeaders['X-Conduit-Webview'] ? { 'X-Conduit-Webview': _baseHeaders['X-Conduit-Webview'] } : undefined;
      return protocol.invoke(cmd, payload, options?.timeout, extra);
    },

    drain: drainChannel,

    subscribe: subscribeToChannel,

    channels: bootstrapInfo.channels ?? [],

    disconnect() {
      // Snapshot before clearing — wrappedUnlisten mutates `unsubscribers`
      // via splice, so iterating the live array would skip entries.
      const snapshot = [...unsubscribers];
      unsubscribers.length = 0;
      for (const unsub of snapshot) {
        unsub();
      }
    },
  };
}

// ── connect() ───────────────────────────────────────────────────

/**
 * Connect to the conduit backend.
 *
 * Calls `plugin:conduit|bootstrap` to obtain per-session credentials,
 * then establishes the custom protocol transport for request/response.
 *
 * If bootstrap fails, an error is thrown — there is no fallback transport.
 */
export async function connect(): Promise<Conduit> {
  const bootstrapInfo = await bootstrap();

  const protocol = createProtocolTransport(
    bootstrapInfo.protocolBase,
    bootstrapInfo.invokeKey,
  );

  return buildConduit(protocol, bootstrapInfo);
}

// ── Lazy singleton for drop-in invoke() ─────────────────────────

let _conduit: Promise<Conduit> | null = null;

function getConduit(): Promise<Conduit> {
  if (!_conduit) {
    _conduit = connect().catch((err) => {
      // Reset on failure so the next call retries bootstrap.
      _conduit = null;
      throw err;
    });
  }
  return _conduit;
}

/**
 * Drop-in replacement for `@tauri-apps/api/core`'s `invoke()`.
 *
 * On the first call, bootstraps the transport and caches the connection.
 * All subsequent calls reuse the same transport.
 *
 * Commands are routed through the custom protocol transport (conduit://)
 * which is in-process and Tauri-approved.
 *
 * @note Commands returning `()` resolve to `null`. This matches Tauri's
 * `invoke()` behavior. For null-safety, use `invoke<MyType | null>(cmd)`.
 * No runtime type validation is performed on the response — `T` is an
 * unchecked assertion, same as Tauri's built-in invoke.
 *
 * @note Error handling differs from Tauri's built-in invoke: conduit throws
 * `ConduitError` (with `status`, `target`, `message` fields) instead of
 * Tauri's raw error values. Catch blocks need updating:
 * `catch(e) { if (e instanceof ConduitError) { e.status, e.message } }`
 *
 * @warning The response is parsed with `JSON.parse` which does not sanitize
 * `__proto__` keys. Do not spread the result into other objects without
 * validation if the command handler is untrusted.
 */
export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
  options?: InvokeOptions,
): Promise<T> {
  const conduit = await getConduit();
  return conduit.invoke<T>(cmd, args, options);
}

/**
 * Send a binary command through the conduit transport.
 *
 * Uses the custom protocol transport for in-process binary IPC.
 */
export async function invokeBinary(
  cmd: string,
  payload?: Uint8Array,
  options?: InvokeOptions,
): Promise<ArrayBuffer> {
  const conduit = await getConduit();
  return conduit.invokeBinary(cmd, payload, options);
}

/**
 * Drain all buffered frames from a named ring buffer channel.
 *
 * Returns the raw binary blob from the server. The wire format is:
 *   [u32 LE frame_count] followed by [u32 LE len][bytes] per frame.
 */
export async function drain(channel: string): Promise<ArrayBuffer> {
  const conduit = await getConduit();
  return conduit.drain(channel);
}

/**
 * Subscribe to push notifications from a ring buffer channel.
 *
 * Uses Tauri's event system for lightweight notification + custom protocol
 * for binary data retrieval. This is the recommended way to receive
 * streaming data from the Rust backend.
 *
 * @example
 * ```typescript
 * import { subscribe } from 'tauri-plugin-conduit';
 *
 * const unsub = await subscribe('market-data', (buf) => {
 *   // Parse binary frames...
 * });
 * ```
 */
export async function subscribe(
  channel: string,
  callback: (data: ArrayBuffer) => void,
  onError?: (err: Error) => void,
): Promise<UnsubscribeFn> {
  const conduit = await getConduit();
  return conduit.subscribe(channel, callback, onError);
}

/**
 * Reset the global Conduit singleton, forcing re-bootstrap on next use.
 *
 * This is primarily useful during development when page reloads lose JS state
 * but the Rust backend continues running. The next `invoke()` / `subscribe()`
 * call will automatically re-bootstrap the transport.
 */
export async function resetConduit(): Promise<void> {
  const pending = _conduit;
  _conduit = null;
  if (pending) {
    try {
      const c = await pending;
      c.disconnect();
    } catch {
      // Bootstrap may have failed — nothing to disconnect
    }
  }
}
