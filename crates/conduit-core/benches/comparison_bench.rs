//! Head-to-head comparison: Tauri invoke vs conduit Level 1 vs conduit Level 2.
//!
//! - **Tauri invoke (simulated)**: JSON string -> serde_json::Value -> typed T
//!   -> handler -> T -> serde_json::Value -> JSON string. This mirrors Tauri's
//!   internal flow where every invoke goes through an intermediate Value.
//!
//! - **conduit Level 1 raw** (register): JSON bytes arrive directly at the
//!   handler. Handler does serde_json::from_slice -> process ->
//!   serde_json::to_vec. No intermediate Value representation.
//!
//! - **conduit Level 1 typed** (register_json): Same as Level 1 raw, but the
//!   deserialization/serialization is handled by the Router, not the handler.
//!   Should be equivalent perf, but validates the typed wrapper path.
//!
//! - **conduit Level 2 raw** (register): raw bytes arrive at the handler via
//!   Decode -> process -> Encode. No JSON anywhere in the path.
//!
//! - **conduit Level 2 typed** (register_binary): Same as Level 2 raw, but
//!   the decode/encode is handled by the Router. Validates the typed wrapper.

use conduit_core::{Decode, Encode, Router};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

// -- Shared struct (25 bytes binary, ~65 bytes JSON) -------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct MarketTick {
    timestamp: i64,
    price: f64,
    volume: f64,
    side: u8,
}

// Manual Encode/Decode to avoid proc-macro dep in benchmarks.
impl Encode for MarketTick {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.timestamp.encode(buf);
        self.price.encode(buf);
        self.volume.encode(buf);
        self.side.encode(buf);
    }
    fn encode_size(&self) -> usize {
        8 + 8 + 8 + 1 // 25 bytes
    }
}

impl Decode for MarketTick {
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

// -- Medium struct (~1KB JSON payload) ---------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct MediumPayload {
    id: u64,
    name: String,
    values: Vec<f64>,
    tags: Vec<String>,
    active: bool,
}

impl Encode for MediumPayload {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.id.encode(buf);
        self.name.encode(buf);
        // Encode values as length-prefixed array of f64
        (self.values.len() as u32).encode(buf);
        for v in &self.values {
            v.encode(buf);
        }
        // Encode tags as length-prefixed array of strings
        (self.tags.len() as u32).encode(buf);
        for t in &self.tags {
            t.encode(buf);
        }
        (self.active as u8).encode(buf);
    }
    fn encode_size(&self) -> usize {
        8                                           // id
        + 4 + self.name.len()                       // name
        + 4 + self.values.len() * 8                 // values
        + 4 + self.tags.iter().map(|t| 4 + t.len()).sum::<usize>() // tags
        + 1 // active
    }
}

impl Decode for MediumPayload {
    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        let mut off = 0;
        let (id, n) = u64::decode(&data[off..])?;
        off += n;
        let (name, n) = String::decode(&data[off..])?;
        off += n;
        let (values_len, n) = u32::decode(&data[off..])?;
        off += n;
        let mut values = Vec::with_capacity(values_len as usize);
        for _ in 0..values_len {
            let (v, n) = f64::decode(&data[off..])?;
            off += n;
            values.push(v);
        }
        let (tags_len, n) = u32::decode(&data[off..])?;
        off += n;
        let mut tags = Vec::with_capacity(tags_len as usize);
        for _ in 0..tags_len {
            let (t, n) = String::decode(&data[off..])?;
            off += n;
            tags.push(t);
        }
        let (active_byte, n) = u8::decode(&data[off..])?;
        off += n;
        Some((
            Self {
                id,
                name,
                values,
                tags,
                active: active_byte != 0,
            },
            off,
        ))
    }
}

fn medium_payload() -> MediumPayload {
    MediumPayload {
        id: 123456789,
        name: "benchmark-medium-payload-test-struct".to_string(),
        values: (0..100).map(|i| i as f64 * 1.5).collect(),
        tags: (0..10).map(|i| format!("tag-{i:04}-value")).collect(),
        active: true,
    }
}

// -- Large struct (~64KB binary payload) -------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
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
        8 + 4 + self.data.len() + 4
    }
}

impl Decode for LargePayload {
    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        let mut off = 0;
        let (header, n) = u64::decode(&data[off..])?;
        off += n;
        let (payload, n) = Vec::<u8>::decode(&data[off..])?;
        off += n;
        let (checksum, n) = u32::decode(&data[off..])?;
        off += n;
        Some((
            Self {
                header,
                data: payload,
                checksum,
            },
            off,
        ))
    }
}

fn large_payload() -> LargePayload {
    LargePayload {
        header: 0xDEAD_BEEF_CAFE_BABE,
        data: vec![0xABu8; 64 * 1024],
        checksum: 0x12345678,
    }
}

// ============================================================================
// 25B struct roundtrip
// ============================================================================

fn level_comparison_struct(c: &mut Criterion) {
    let mut group = c.benchmark_group("25B struct roundtrip");

    let table = Router::new();

    // Tauri-style handler: receives JSON string as bytes, goes through Value
    table.register("tauri_echo", |payload: Vec<u8>| {
        // Tauri internally: JSON string -> Value -> from_value::<T>
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let tick: MarketTick = serde_json::from_value(value).unwrap();
        // Handler processes...
        // Tauri internally: T -> to_value -> JSON string
        let out_value = serde_json::to_value(&tick).unwrap();
        serde_json::to_vec(&out_value).unwrap()
    });

    // conduit Level 1 raw: receives JSON bytes, parses directly (no Value middleman)
    table.register("conduit_l1_raw_echo", |payload: Vec<u8>| {
        let tick: MarketTick = serde_json::from_slice(&payload).unwrap();
        // Handler processes...
        serde_json::to_vec(&tick).unwrap()
    });

    // conduit Level 1 typed: register_json handles serde for the handler
    table.register_json("conduit_l1_typed_echo", |tick: MarketTick| tick);

    // conduit Level 2 raw: receives binary, decodes directly
    table.register("conduit_l2_raw_echo", |payload: Vec<u8>| {
        let (tick, _) = MarketTick::decode(&payload).unwrap();
        // Handler processes...
        let mut out = Vec::with_capacity(tick.encode_size());
        tick.encode(&mut out);
        out
    });

    // conduit Level 2 typed: register_binary handles encode/decode for the handler
    table.register_binary("conduit_l2_typed_echo", |tick: MarketTick| tick);

    let t = tick();
    let json_payload = serde_json::to_vec(&t).unwrap();
    let mut wire_payload = Vec::with_capacity(25);
    t.encode(&mut wire_payload);

    group.bench_function("Tauri invoke (JSON via Value)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("tauri_echo", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L1 raw (JSON direct)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("conduit_l1_raw_echo", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L1 typed (register_json)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("conduit_l1_typed_echo", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L2 raw (binary)", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let result = table.call("conduit_l2_raw_echo", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L2 typed (register_binary)", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let result = table.call("conduit_l2_typed_echo", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================================
// ~1KB payload roundtrip
// ============================================================================

fn level_comparison_1kb(c: &mut Criterion) {
    let mut group = c.benchmark_group("1KB payload roundtrip");

    let table = Router::new();

    // Tauri: JSON string -> Value -> MediumPayload -> Value -> JSON string
    table.register("tauri_1kb", |payload: Vec<u8>| {
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let data: MediumPayload = serde_json::from_value(value).unwrap();
        let out_value = serde_json::to_value(&data).unwrap();
        serde_json::to_vec(&out_value).unwrap()
    });

    // conduit Level 1 raw: JSON bytes -> MediumPayload -> JSON bytes
    table.register("conduit_l1_raw_1kb", |payload: Vec<u8>| {
        let data: MediumPayload = serde_json::from_slice(&payload).unwrap();
        serde_json::to_vec(&data).unwrap()
    });

    // conduit Level 1 typed: register_json
    table.register_json("conduit_l1_typed_1kb", |data: MediumPayload| data);

    // conduit Level 2 raw: binary bytes -> MediumPayload -> binary bytes
    table.register("conduit_l2_raw_1kb", |payload: Vec<u8>| {
        let (data, _) = MediumPayload::decode(&payload).unwrap();
        let mut out = Vec::with_capacity(data.encode_size());
        data.encode(&mut out);
        out
    });

    // conduit Level 2 typed: register_binary
    table.register_binary("conduit_l2_typed_1kb", |data: MediumPayload| data);

    let m = medium_payload();
    let json_payload = serde_json::to_vec(&m).unwrap();
    let mut wire_payload = Vec::with_capacity(m.encode_size());
    m.encode(&mut wire_payload);

    group.bench_function("Tauri invoke (JSON via Value)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("tauri_1kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L1 raw (JSON direct)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("conduit_l1_raw_1kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L1 typed (register_json)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("conduit_l1_typed_1kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L2 raw (binary)", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let result = table.call("conduit_l2_raw_1kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L2 typed (register_binary)", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let result = table.call("conduit_l2_typed_1kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================================
// 64KB payload roundtrip
// ============================================================================

fn level_comparison_64kb(c: &mut Criterion) {
    let mut group = c.benchmark_group("64KB payload roundtrip");

    let table = Router::new();

    // Tauri: JSON string -> Value -> LargePayload -> Value -> JSON string
    table.register("tauri_64kb", |payload: Vec<u8>| {
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let data: LargePayload = serde_json::from_value(value).unwrap();
        let out_value = serde_json::to_value(&data).unwrap();
        serde_json::to_vec(&out_value).unwrap()
    });

    // conduit Level 1 raw: JSON bytes -> LargePayload -> JSON bytes
    table.register("conduit_l1_raw_64kb", |payload: Vec<u8>| {
        let data: LargePayload = serde_json::from_slice(&payload).unwrap();
        serde_json::to_vec(&data).unwrap()
    });

    // conduit Level 1 typed: register_json
    table.register_json("conduit_l1_typed_64kb", |data: LargePayload| data);

    // conduit Level 2 raw: binary bytes -> LargePayload -> binary bytes
    table.register("conduit_l2_raw_64kb", |payload: Vec<u8>| {
        let (data, _) = LargePayload::decode(&payload).unwrap();
        let mut out = Vec::with_capacity(data.encode_size());
        data.encode(&mut out);
        out
    });

    // conduit Level 2 typed: register_binary
    table.register_binary("conduit_l2_typed_64kb", |data: LargePayload| data);

    let lp = large_payload();
    let json_payload = serde_json::to_vec(&lp).unwrap();
    let mut wire_payload = Vec::with_capacity(lp.encode_size());
    lp.encode(&mut wire_payload);

    group.bench_function("Tauri invoke (JSON via Value)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("tauri_64kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L1 raw (JSON direct)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("conduit_l1_raw_64kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L1 typed (register_json)", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.call("conduit_l1_typed_64kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L2 raw (binary)", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let result = table.call("conduit_l2_raw_64kb", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("conduit L2 typed (register_binary)", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let result = table.call("conduit_l2_typed_64kb", p).unwrap();
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
