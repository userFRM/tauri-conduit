/**
 * Transport bootstrap for tauri-conduit.
 *
 * Calls the Rust plugin's `bootstrap` command to obtain connection parameters
 * for the custom protocol transport. The conduit:// protocol is always
 * in-process — there is no network surface.
 */

import { invoke as tauriInvoke } from '@tauri-apps/api/core';

export interface BootstrapInfo {
  /** Base URL for the custom protocol transport, e.g. "conduit://localhost". */
  protocolBase: string;
  /** Per-session authentication key validated on every request. */
  invokeKey: string;
  /** Available ring buffer channel names registered on the Rust side. */
  channels: string[];
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
  return info;
}
