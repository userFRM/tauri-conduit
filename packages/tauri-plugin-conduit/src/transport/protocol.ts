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

/** Default per-request timeout in milliseconds (10 seconds). */
export const DEFAULT_TIMEOUT_MS = 10_000;

export interface ProtocolTransport {
  invoke(cmd: string, payload?: Uint8Array, timeoutMs?: number): Promise<ArrayBuffer>;
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
      payload?: Uint8Array,
      timeoutMs: number = DEFAULT_TIMEOUT_MS,
    ): Promise<ArrayBuffer> {
      const url = `${baseUrl}/invoke/${encodeURIComponent(cmd)}`;
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), timeoutMs);
      try {
        const response = await fetch(url, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/octet-stream',
            'X-Conduit-Key': invokeKey,
          },
          body: (payload ?? new Uint8Array(0)) as BodyInit,
          signal: controller.signal,
        });
        if (!response.ok) {
          throw new Error(`tauri-conduit: command "${cmd}" failed: ${response.status}`);
        }
        return response.arrayBuffer();
      } catch (err) {
        if (err instanceof DOMException && err.name === 'AbortError') {
          throw new Error(`tauri-conduit: command "${cmd}" timed out after ${timeoutMs}ms`);
        }
        throw err;
      } finally {
        clearTimeout(timer);
      }
    },
  };
}
