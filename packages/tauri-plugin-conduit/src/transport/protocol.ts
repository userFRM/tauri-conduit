/**
 * Custom Protocol transport for tauri-conduit.
 *
 * Uses fetch() with the custom `conduit://` scheme. The Tauri webview engine
 * intercepts this in-process -- no network involved. This is the primary,
 * Tauri-approved transport for command invocation.
 *
 * The invoke key is a per-session authentication token provided by the
 * bootstrap call and validated server-side on every request.
 */

import { ConduitError } from '../error.js';

/** Default per-request timeout in milliseconds (10 seconds). */
export const DEFAULT_TIMEOUT_MS = 10_000;

const EMPTY_BODY = new Uint8Array(0);

export interface ProtocolTransport {
  invoke(
    cmd: string,
    payload?: Uint8Array | string,
    timeoutMs?: number,
    extraHeaders?: Record<string, string>,
  ): Promise<ArrayBuffer>;
}

/**
 * Create a custom protocol transport.
 *
 * @param baseUrl   The protocol base URL, e.g. "conduit://localhost"
 * @param invokeKey Per-session authentication key from bootstrap
 */
export function createProtocolTransport(
  baseUrl: string,
  invokeKey: string,
): ProtocolTransport {
  return {
    async invoke(
      cmd: string,
      payload?: Uint8Array | string,
      timeoutMs: number = DEFAULT_TIMEOUT_MS,
      extraHeaders?: Record<string, string>,
    ): Promise<ArrayBuffer> {
      const url = `${baseUrl}/invoke/${encodeURIComponent(cmd)}`;
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), timeoutMs);
      try {
        const headers: Record<string, string> = {
          'Content-Type': 'application/octet-stream',
          'X-Conduit-Key': invokeKey,
          ...(extraHeaders ?? {}),
        };
        const response = await fetch(url, {
          method: 'POST',
          headers,
          body: (payload ?? EMPTY_BODY) as BodyInit,
          signal: controller.signal,
        });
        if (!response.ok) {
          const errorBody = await response.text();
          let message: string;
          try {
            const parsed = JSON.parse(errorBody);
            message = parsed.error ?? errorBody;
          } catch {
            message = errorBody;
          }
          throw new ConduitError(response.status, cmd, message);
        }
        return response.arrayBuffer();
      } catch (err) {
        if (err instanceof DOMException && err.name === 'AbortError') {
          throw new ConduitError(408, cmd, `timed out after ${timeoutMs}ms`);
        }
        throw err;
      } finally {
        clearTimeout(timer);
      }
    },
  };
}
