//! Head-to-head comparison: JSON (serde) vs binary (conduit) serialization.
//!
//! This benchmark isolates the codec overhead that conduit eliminates relative
//! to Tauri's built-in JSON-based invoke(). The full IPC round-trip includes
//! the Tauri custom protocol handler (platform-dependent), but the codec
//! comparison shows the serialization advantage on any hardware.

use conduit_core::{WireDecode, WireEncode};
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

// ── Encode comparison ────────────────────────────────────────────

fn encode_comparison(c: &mut Criterion) {
    let t = tick();

    c.bench_function("encode 25B struct — JSON (serde)", |b| {
        b.iter(|| {
            let json = serde_json::to_vec(black_box(&t)).unwrap();
            black_box(json);
        });
    });

    c.bench_function("encode 25B struct — binary (conduit)", |b| {
        let mut buf = Vec::with_capacity(25);
        b.iter(|| {
            buf.clear();
            black_box(&t).wire_encode(&mut buf);
            black_box(&buf);
        });
    });
}

// ── Decode comparison ────────────────────────────────────────────

fn decode_comparison(c: &mut Criterion) {
    let t = tick();
    let json_bytes = serde_json::to_vec(&t).unwrap();
    let mut wire_bytes = Vec::with_capacity(25);
    t.wire_encode(&mut wire_bytes);

    c.bench_function("decode 25B struct — JSON (serde)", |b| {
        b.iter(|| {
            let decoded: MarketTick = serde_json::from_slice(black_box(&json_bytes)).unwrap();
            black_box(decoded);
        });
    });

    c.bench_function("decode 25B struct — binary (conduit)", |b| {
        b.iter(|| {
            let (decoded, _) = MarketTick::wire_decode(black_box(&wire_bytes)).unwrap();
            black_box(decoded);
        });
    });
}

// ── Roundtrip comparison ─────────────────────────────────────────

fn roundtrip_comparison(c: &mut Criterion) {
    let t = tick();

    c.bench_function("roundtrip 25B struct — JSON (serde)", |b| {
        b.iter(|| {
            let json = serde_json::to_vec(black_box(&t)).unwrap();
            let decoded: MarketTick = serde_json::from_slice(black_box(&json)).unwrap();
            black_box(decoded);
        });
    });

    c.bench_function("roundtrip 25B struct — binary (conduit)", |b| {
        let mut buf = Vec::with_capacity(25);
        b.iter(|| {
            buf.clear();
            black_box(&t).wire_encode(&mut buf);
            let (decoded, _) = MarketTick::wire_decode(black_box(&buf)).unwrap();
            black_box(decoded);
        });
    });
}

// ── Dispatch simulation comparison ───────────────────────────────

fn dispatch_roundtrip(c: &mut Criterion) {
    use conduit_core::DispatchTable;

    let table = DispatchTable::new();

    // JSON handler: deserialize → process → serialize
    table.register("echo_json", |payload: Vec<u8>| {
        let tick: MarketTick = serde_json::from_slice(&payload).unwrap();
        serde_json::to_vec(&tick).unwrap()
    });

    // Binary handler: decode → process → encode
    table.register("echo_binary", |payload: Vec<u8>| {
        let (tick, _) = MarketTick::wire_decode(&payload).unwrap();
        let mut out = Vec::with_capacity(tick.wire_size());
        tick.wire_encode(&mut out);
        out
    });

    let t = tick();
    let json_payload = serde_json::to_vec(&t).unwrap();
    let mut wire_payload = Vec::with_capacity(25);
    t.wire_encode(&mut wire_payload);

    c.bench_function("dispatch echo — JSON path", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let result = table.dispatch("echo_json", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    c.bench_function("dispatch echo — binary path", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let result = table.dispatch("echo_binary", p).unwrap();
                black_box(result);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

// ── Large payload comparison ─────────────────────────────────────

fn large_payload(c: &mut Criterion) {
    // 1 KB raw binary payload — no struct, just bytes
    let data_1k = vec![0xABu8; 1024];

    c.bench_function("roundtrip 1KB payload — JSON (serde)", |b| {
        b.iter(|| {
            let json = serde_json::to_vec(black_box(&data_1k)).unwrap();
            let decoded: Vec<u8> = serde_json::from_slice(black_box(&json)).unwrap();
            black_box(decoded);
        });
    });

    c.bench_function("roundtrip 1KB payload — binary (memcpy)", |b| {
        b.iter(|| {
            // Binary path: just copy the bytes (no encoding needed for raw data)
            let encoded = black_box(&data_1k).to_vec();
            let decoded = black_box(encoded);
            black_box(decoded);
        });
    });

    // 64 KB payload
    let data_64k = vec![0xABu8; 64 * 1024];

    c.bench_function("roundtrip 64KB payload — JSON (serde)", |b| {
        b.iter(|| {
            let json = serde_json::to_vec(black_box(&data_64k)).unwrap();
            let decoded: Vec<u8> = serde_json::from_slice(black_box(&json)).unwrap();
            black_box(decoded);
        });
    });

    c.bench_function("roundtrip 64KB payload — binary (memcpy)", |b| {
        b.iter(|| {
            let encoded = black_box(&data_64k).to_vec();
            let decoded = black_box(encoded);
            black_box(decoded);
        });
    });
}

criterion_group!(
    benches,
    encode_comparison,
    decode_comparison,
    roundtrip_comparison,
    dispatch_roundtrip,
    large_payload,
);
criterion_main!(benches);
