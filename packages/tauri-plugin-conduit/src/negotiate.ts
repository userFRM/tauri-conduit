/**
 * Transport bootstrap for tauri-conduit.
 *
 * Calls the Rust plugin's `bootstrap` command to obtain connection parameters
 * for the custom protocol transport. The conduit:// protocol is always
 * in-process — there is no network surface.
 */

import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

export interface BootstrapInfo {
  /** Base URL for the custom protocol transport, e.g. "conduit://localhost". */
  protocolBase: string;
  /** Per-session authentication key validated on every request. */
  invokeKey: string;
  /** Available ring buffer channel names registered on the Rust side. */
  channels: string[];
  /** Protocol version from the Rust side. */
  protocolVersion?: number;
  /** Label of the current webview, used for Window/WebviewWindow injection. */
  webviewLabel?: string;
}

/**
 * Bootstrap the conduit transport layer.
 *
 * Makes a single Tauri IPC call to `plugin:conduit|bootstrap` which returns
 * all connection parameters. This is origin-locked by Tauri's IPC security.
 */
export async function bootstrap(): Promise<BootstrapInfo> {
  const info: BootstrapInfo = await tauriInvoke<BootstrapInfo>(
    'plugin:conduit|bootstrap',
  );
  // Capture the current webview label for Window/WebviewWindow injection.
  // This is best-effort — if the API is unavailable, we omit the label.
  try {
    const webview = getCurrentWebviewWindow();
    info.webviewLabel = webview.label;
  } catch {
    // Not in a webview context (e.g., unit tests) — skip
  }
  return info;
}

/**
 * Validate that a channel exists on the Rust side.
 *
 * Calls `plugin:conduit|conduit_subscribe` which returns the subset of
 * requested channels that actually exist. Throws if the channel is unknown.
 */
export async function validateChannel(channel: string): Promise<void> {
  const result = await tauriInvoke<string[]>(
    'plugin:conduit|conduit_subscribe',
    { channels: [channel] },
  );
  if (!result.includes(channel)) {
    throw new Error(`conduit: unknown channel "${channel}"`);
  }
}
