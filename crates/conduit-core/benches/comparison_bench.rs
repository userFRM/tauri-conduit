//! Head-to-head comparison: Tauri invoke vs conduit Level 1 vs conduit Level 2.
//!
//! - **Tauri invoke (simulated)**: JSON string → serde_json::Value → typed T
//!   → handler → T → serde_json::Value → JSON string. This mirrors Tauri's
//!   internal flow where every invoke goes through an intermediate Value.
//!
//! - **conduit Level 1 (drop-in)**: JSON bytes arrive directly at the handler.
//!   Handler does serde_json::from_slice → process → serde_json::to_vec.
//!   No intermediate Value representation. Same JSON, less overhead.
//!
//! - **conduit Level 2 (binary)**: raw bytes arrive at the handler via
//!   WireDecode → process → WireEncode. No JSON anywhere in the path.

use conduit_core::{DispatchTable, WireDecode, WireEncode};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

// ── Shared struct ────────────────────────────────────────────────

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct MarketTick {
    timestamp: i64,
    price: f64,
    volume: f64,
    side: u8,
}

// Manual WireEncode/WireDecode to avoid proc-macro dep in benchmarks.
impl WireEncode for MarketTick {
    fn wire_encode(&self, buf: &mut Vec<u8>) {
        self.timestamp.wire_encode(buf);
        self.price.wire_encode(buf);
        self.volume.wire_encode(buf);
        self.side.wire_encode(buf);
    }
    fn wire_size(&self) -> usize {
        8 + 8 + 8 + 1 // 25 bytes
    }
}

impl WireDecode for MarketTick {
    fn wire_decode(data: &[u8]) -> Option<(Self, usize)> {
        let mut off = 0;
        let (timestamp, n) = i64::wire_decode(&data[off..])?;
        off += n;
        let (price, n) = f64::wire_decode(&data[off..])?;
        off += n;
        let (volume, n) = f64::wire_decode(&data[off..])?;
        off += n;
        let (side, n) = u8::wire_decode(&data[off..])?;
        off += n;
        Some((
            Self {
                timestamp,
                price,
                volume,
                side,
            },
            off,
        ))
    }
}

fn tick() -> MarketTick {
    MarketTick {
        timestamp: 1700000000000,
        price: 42850.75,
        volume: 3.14159,
        side: 1,
    }
}

// ── Level comparison: roundtrip through dispatch ─────────────────

fn level_comparison_struct(c: &mut Criterion) {
    let mut group = c.benchmark_group("25B struct roundtrip");

    let table = DispatchTable::new();

    // Tauri-style handler: receives JSON string as bytes, goes through Value
    table.register("tauri_echo", |payload: Vec<u8>| {
        // Tauri internally: JSON string → Value → from_value::<T>
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let tick: MarketTick = serde_json::from_value(value).unwrap();
        // Handler processes...
        // Tauri internally: T → to_value → JSON string
        let out_value = serde_json::to_value(&tick).unwrap();
        serde_json::to_vec(&out_value).unwrap()
    });

    // conduit Level 1: receives JSON bytes, parses directly (no Value middleman)
    table.register("conduit_l1_echo", |payload: Vec<u8>| {
        let tick: MarketTick = serde_json::from_slice(&payload).unwrap();
        // Handler processes...
        serde_json::to_vec(&tick).unwrap()
    });

    // conduit Level 2: receives binary, decodes directly
    table.register("conduit_l2_echo", |payload: Vec<u8>| {
        let (tick, _) = MarketTick::wire_decode(&payload).unwrap();
        // Handler processes...
        let mut out = Vec::with_capacity(tick.wire_size());
        tick.wire_encode(&mut out);
        out
    });

    let t = tick();
    let json_payload = serde_json::to_vec(&t).unwrap();
    let mut wire_payload = Vec::with_capacity(25);
    t.wire_encode(&mut wire_payload);

    group.bench_function("Tauri invoke (JSON via Value)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.dispatch("tauri_echo", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit Level 1 (JSON direct)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.dispatch("conduit_l1_echo", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit Level 2 (binary)", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let result = table.dispatch("conduit_l2_echo", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ── Level comparison: 1 KB payload ──────────────────────────────

fn level_comparison_1kb(c: &mut Criterion) {
    let mut group = c.benchmark_group("1KB payload roundtrip");

    let table = DispatchTable::new();

    // Tauri: JSON string → Value → Vec<u8> → Value → JSON string
    table.register("tauri_1kb", |payload: Vec<u8>| {
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let data: Vec<u8> = serde_json::from_value(value).unwrap();
        let out_value = serde_json::to_value(&data).unwrap();
        serde_json::to_vec(&out_value).unwrap()
    });

    // conduit Level 1: JSON bytes → Vec<u8> → JSON bytes
    table.register("conduit_l1_1kb", |payload: Vec<u8>| {
        let data: Vec<u8> = serde_json::from_slice(&payload).unwrap();
        serde_json::to_vec(&data).unwrap()
    });

    // conduit Level 2: raw bytes passthrough
    table.register("conduit_l2_1kb", |payload: Vec<u8>| payload);

    let data_1k = vec![0xABu8; 1024];
    let json_payload = serde_json::to_vec(&data_1k).unwrap();

    group.bench_function("Tauri invoke (JSON via Value)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.dispatch("tauri_1kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit Level 1 (JSON direct)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.dispatch("conduit_l1_1kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit Level 2 (binary)", |b| {
        b.iter_batched(
            || data_1k.clone(),
            |p| {
                let result = table.dispatch("conduit_l2_1kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ── Level comparison: 64 KB payload ─────────────────────────────

fn level_comparison_64kb(c: &mut Criterion) {
    let mut group = c.benchmark_group("64KB payload roundtrip");

    let table = DispatchTable::new();

    table.register("tauri_64kb", |payload: Vec<u8>| {
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let data: Vec<u8> = serde_json::from_value(value).unwrap();
        let out_value = serde_json::to_value(&data).unwrap();
        serde_json::to_vec(&out_value).unwrap()
    });

    table.register("conduit_l1_64kb", |payload: Vec<u8>| {
        let data: Vec<u8> = serde_json::from_slice(&payload).unwrap();
        serde_json::to_vec(&data).unwrap()
    });

    table.register("conduit_l2_64kb", |payload: Vec<u8>| payload);

    let data_64k = vec![0xABu8; 64 * 1024];
    let json_payload = serde_json::to_vec(&data_64k).unwrap();

    group.bench_function("Tauri invoke (JSON via Value)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.dispatch("tauri_64kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit Level 1 (JSON direct)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.dispatch("conduit_l1_64kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit Level 2 (binary)", |b| {
        b.iter_batched(
            || data_64k.clone(),
            |p| {
                let result = table.dispatch("conduit_l2_64kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    level_comparison_struct,
    level_comparison_1kb,
    level_comparison_64kb,
);
criterion_main!(benches);
