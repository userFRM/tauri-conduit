/**
 * @tauri-conduit/client
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
 * import { invoke } from '@tauri-conduit/client';
 * const result = await invoke<MyType>('my_command', { key: 'value' });
 * ```
 *
 * @example
 * ```typescript
 * // Push streaming — event-driven, no polling
 * import { subscribe } from '@tauri-conduit/client';
 * const unsub = await subscribe('market-data', (buf) => {
 *   // Parse binary frames from buf...
 * });
 * ```
 *
 * @example
 * ```typescript
 * // Full control with connect()
 * import { connect } from '@tauri-conduit/client';
 * const conduit = await connect();
 * const buf = await conduit.invokeBinary('raw_cmd', new Uint8Array([1, 2, 3]));
 * const unsub = await conduit.subscribe('telemetry', onData);
 * ```
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { bootstrap, type BootstrapInfo } from './negotiate.js';
import { createProtocolTransport, type ProtocolTransport } from './transport/protocol.js';

// ── Re-exports ──────────────────────────────────────────────────

export { FRAME_HEADER_SIZE, PROTOCOL_VERSION, MsgType } from './codec/frame.js';
export type { FrameHeader } from './codec/frame.js';
export { writeFrameHeader, readFrameHeader } from './codec/frame.js';
export { DEFAULT_TIMEOUT_MS } from './transport/protocol.js';
export type { ProtocolTransport } from './transport/protocol.js';
export type { BootstrapInfo } from './negotiate.js';

// ── Public types ────────────────────────────────────────────────

/** Options for invoke calls — mirrors @tauri-apps/api/core InvokeOptions. */
export interface InvokeOptions {
  /** Per-request timeout in milliseconds. Defaults to DEFAULT_TIMEOUT_MS. */
  timeout?: number;
}

/** Function that unsubscribes from a channel when called. */
export type UnsubscribeFn = UnlistenFn;

export interface Conduit {
  /** Send a command with JSON args and await a typed response. */
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
  subscribe(channel: string, callback: (data: ArrayBuffer) => void): Promise<UnsubscribeFn>;
  /** Convenience alias for {@link subscribe}. */
  onData(channel: string, callback: (data: ArrayBuffer) => void): Promise<UnsubscribeFn>;
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

  async function drainChannel(channel: string): Promise<ArrayBuffer> {
    const url =
      `${bootstrapInfo.protocolBase}/drain/${encodeURIComponent(channel)}`;
    const response = await fetch(url, {
      method: 'GET',
      headers: { 'X-Conduit-Key': bootstrapInfo.invokeKey },
    });
    if (!response.ok) {
      throw new Error(`tauri-conduit: drain failed: ${response.status}`);
    }
    return response.arrayBuffer();
  }

  async function subscribeToChannel(
    channel: string,
    callback: (data: ArrayBuffer) => void,
  ): Promise<UnsubscribeFn> {
    const unlisten = await listen<string>(
      'conduit:data-available',
      async (event) => {
        if (event.payload === channel) {
          const buf = await drainChannel(channel);
          if (buf.byteLength > 0) {
            callback(buf);
          }
        }
      },
    );
    unsubscribers.push(unlisten);
    return unlisten;
  }

  return {
    async invoke<T>(
      cmd: string,
      args?: Record<string, unknown>,
      options?: InvokeOptions,
    ): Promise<T> {
      let payload: Uint8Array | undefined;
      if (args !== undefined) {
        const encoder = new TextEncoder();
        payload = encoder.encode(JSON.stringify(args));
      }

      const raw = await protocol.invoke(cmd, payload, options?.timeout);

      const decoder = new TextDecoder();
      const text = decoder.decode(raw);

      if (text.length === 0) return undefined as T;

      return JSON.parse(text) as T;
    },

    async invokeBinary(
      cmd: string,
      payload?: Uint8Array,
      options?: InvokeOptions,
    ): Promise<ArrayBuffer> {
      return protocol.invoke(cmd, payload, options?.timeout);
    },

    drain: drainChannel,

    subscribe: subscribeToChannel,

    onData: subscribeToChannel,

    channels: bootstrapInfo.channels ?? [],

    disconnect() {
      for (const unsub of unsubscribers) {
        unsub();
      }
      unsubscribers.length = 0;
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
 * import { subscribe } from '@tauri-conduit/client';
 *
 * const unsub = await subscribe('market-data', (buf) => {
 *   // Parse binary frames...
 * });
 * ```
 */
export async function subscribe(
  channel: string,
  callback: (data: ArrayBuffer) => void,
): Promise<UnsubscribeFn> {
  const conduit = await getConduit();
  return conduit.subscribe(channel, callback);
}

/**
 * Convenience alias for {@link subscribe}.
 */
export async function onData(
  channel: string,
  callback: (data: ArrayBuffer) => void,
): Promise<UnsubscribeFn> {
  const conduit = await getConduit();
  return conduit.onData(channel, callback);
}
