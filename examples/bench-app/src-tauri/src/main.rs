// Prevent console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use conduit::{command, handler};
use serde::{Deserialize, Serialize};

// ── Shared payload types ────────────────────────────────────────

/// ~25 bytes as JSON (small payload)
#[derive(Clone, Serialize, Deserialize)]
struct SmallPayload {
    timestamp: i64,
    price: f64,
    volume: f64,
    side: u8,
}

/// ~1KB as JSON (medium payload)
#[derive(Clone, Serialize, Deserialize)]
struct MediumPayload {
    id: u64,
    name: String,
    values: Vec<f64>,
    tags: Vec<String>,
    active: bool,
}

/// ~64KB as JSON (large payload — 64KB data array base64'd)
#[derive(Clone, Serialize, Deserialize)]
struct LargePayload {
    header: u64,
    data: Vec<u8>,
    checksum: u32,
}

// ── Tauri IPC handlers (baseline) ───────────────────────────────
// These go through: JS → JSON.stringify → postMessage → Tauri IPC bridge
// → serde_json::Value → from_value::<T> → handler → to_value → JSON string
// → postMessage → JS JSON.parse

#[tauri::command]
fn tauri_echo_small(payload: SmallPayload) -> SmallPayload {
    payload
}

#[tauri::command]
fn tauri_echo_medium(payload: MediumPayload) -> MediumPayload {
    payload
}

#[tauri::command]
fn tauri_echo_large(payload: LargePayload) -> LargePayload {
    payload
}

// ── Conduit handlers (Level 1 — JSON via custom protocol) ───────
// These go through: JS → JSON.stringify → fetch(conduit://) → WebView bridge
// → sonic_rs::from_slice (no Value) → handler → sonic_rs::to_vec → response
// → WebView bridge → fetch() response → JS JSON.parse

#[command]
fn conduit_echo_small(payload: SmallPayload) -> SmallPayload {
    payload
}

#[command]
fn conduit_echo_medium(payload: MediumPayload) -> MediumPayload {
    payload
}

#[command]
fn conduit_echo_large(payload: LargePayload) -> LargePayload {
    payload
}

// ── App entry point ─────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_conduit::init()
                .handler("conduit_echo_small", handler!(conduit_echo_small))
                .handler("conduit_echo_medium", handler!(conduit_echo_medium))
                .handler("conduit_echo_large", handler!(conduit_echo_large))
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            tauri_echo_small,
            tauri_echo_medium,
            tauri_echo_large,
        ])
        .run(tauri::generate_context!())
        .expect("error running bench-app");
}
