// Prevent console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use conduit::{command, handler};
use conduit_core::codec::{Decode, Encode};
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

impl Encode for SmallPayload {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.timestamp.encode(buf);
        self.price.encode(buf);
        self.volume.encode(buf);
        self.side.encode(buf);
    }
    fn encode_size(&self) -> usize {
        self.timestamp.encode_size()
            + self.price.encode_size()
            + self.volume.encode_size()
            + self.side.encode_size()
    }
}

impl Decode for SmallPayload {
    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        let mut off = 0;
        let (timestamp, n) = i64::decode(&data[off..])?;
        off += n;
        let (price, n) = f64::decode(&data[off..])?;
        off += n;
        let (volume, n) = f64::decode(&data[off..])?;
        off += n;
        let (side, n) = u8::decode(&data[off..])?;
        off += n;
        Some((Self { timestamp, price, volume, side }, off))
    }
}

/// ~1KB as JSON (medium payload)
/// Vec<f64> and Vec<String> don't implement Encode/Decode,
/// so this payload can only use Level 1 (JSON).
#[derive(Clone, Serialize, Deserialize)]
struct MediumPayload {
    id: u64,
    name: String,
    values: Vec<f64>,
    tags: Vec<String>,
    active: bool,
}

/// ~64KB as JSON (large payload — 64KB data array)
#[derive(Clone, Serialize, Deserialize)]
struct LargePayload {
    header: u64,
    data: Vec<u8>,
    checksum: u32,
}

impl Encode for LargePayload {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.header.encode(buf);
        self.data.encode(buf);
        self.checksum.encode(buf);
    }
    fn encode_size(&self) -> usize {
        self.header.encode_size() + self.data.encode_size() + self.checksum.encode_size()
    }
}

impl Decode for LargePayload {
    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        let mut off = 0;
        let (header, n) = u64::decode(&data[off..])?;
        off += n;
        let (d, n) = Vec::<u8>::decode(&data[off..])?;
        off += n;
        let (checksum, n) = u32::decode(&data[off..])?;
        off += n;
        Some((Self { header, data: d, checksum }, off))
    }
}

// ── Tauri IPC handlers (baseline) ───────────────────────────────

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

// Level 2 handlers are registered via command_binary() below.

// ── Tauri command to print results to stdout ────────────────────

#[tauri::command]
fn print_results(output: String) {
    println!("{output}");
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// ── App entry point ─────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_conduit::init()
                // Level 1: JSON via custom protocol
                .handler("conduit_echo_small", handler!(conduit_echo_small))
                .handler("conduit_echo_medium", handler!(conduit_echo_medium))
                .handler("conduit_echo_large", handler!(conduit_echo_large))
                // Level 2: binary via custom protocol (no JSON)
                .command_binary("binary_echo_small", |p: SmallPayload| p)
                .command_binary("binary_echo_large", |p: LargePayload| p)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            tauri_echo_small,
            tauri_echo_medium,
            tauri_echo_large,
            print_results,
            exit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error running bench-app");
}
