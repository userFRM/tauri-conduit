//! Focused benchmark comparing the three command registration modes.
//!
//! Measures dispatch overhead only for:
//! - `register()` (raw Vec<u8> in, Vec<u8> out)
//! - `register_json()` (typed JSON deserialization/serialization)
//! - `register_binary()` (typed binary Encode/Decode)
//!
//! All handlers perform the same logical operation (identity / echo) so the
//! measured difference is purely framework overhead.

use conduit_core::{Decode, Encode, Router};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

// -- Shared struct -----------------------------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Tick {
    timestamp: i64,
    price: f64,
    volume: f64,
    side: u8,
}

impl Encode for Tick {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.timestamp.encode(buf);
        self.price.encode(buf);
        self.volume.encode(buf);
        self.side.encode(buf);
    }
    fn encode_size(&self) -> usize {
        8 + 8 + 8 + 1
    }
}

impl Decode for Tick {
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

fn make_tick() -> Tick {
    Tick {
        timestamp: 1700000000000,
        price: 42850.75,
        volume: 3.14159,
        side: 1,
    }
}

// ============================================================================
// Handler dispatch overhead: raw vs json vs binary
// ============================================================================

fn handler_raw_echo(c: &mut Criterion) {
    let table = Router::new();
    table.register("raw_echo", |payload: Vec<u8>| payload);

    let payload = b"hello bench".to_vec();

    c.bench_function("handler register() raw echo", |b| {
        b.iter_batched(
            || payload.clone(),
            |p| {
                let resp = table.call(black_box("raw_echo"), p).unwrap();
                black_box(resp);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn handler_json_echo(c: &mut Criterion) {
    let table = Router::new();
    table.register_json("json_echo", |tick: Tick| tick);

    let t = make_tick();
    let json_payload = serde_json::to_vec(&t).unwrap();

    c.bench_function("handler register_json() echo", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let resp = table.call(black_box("json_echo"), p).unwrap();
                black_box(resp);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn handler_binary_echo(c: &mut Criterion) {
    let table = Router::new();
    table.register_binary("binary_echo", |tick: Tick| tick);

    let t = make_tick();
    let mut wire_payload = Vec::with_capacity(t.encode_size());
    t.encode(&mut wire_payload);

    c.bench_function("handler register_binary() echo", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let resp = table.call(black_box("binary_echo"), p).unwrap();
                black_box(resp);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

// ============================================================================
// Handler dispatch with work: add two fields
// ============================================================================

fn handler_json_with_work(c: &mut Criterion) {
    let table = Router::new();
    table.register_json("json_add", |tick: Tick| -> f64 { tick.price + tick.volume });

    let t = make_tick();
    let json_payload = serde_json::to_vec(&t).unwrap();

    c.bench_function("handler register_json() with work", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let resp = table.call(black_box("json_add"), p).unwrap();
                black_box(resp);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn handler_binary_with_work(c: &mut Criterion) {
    let table = Router::new();
    table.register_binary("binary_add", |tick: Tick| -> f64 {
        tick.price + tick.volume
    });

    let t = make_tick();
    let mut wire_payload = Vec::with_capacity(t.encode_size());
    t.encode(&mut wire_payload);

    c.bench_function("handler register_binary() with work", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let resp = table.call(black_box("binary_add"), p).unwrap();
                black_box(resp);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

// ============================================================================
// Handler dispatch overhead: lookup in a table of 100 commands
// ============================================================================

fn handler_lookup_100(c: &mut Criterion) {
    let mut group = c.benchmark_group("handler lookup (100 cmds)");

    let table = Router::new();
    // Register 97 filler commands
    for i in 0..97 {
        let name = format!("filler_{i:03}");
        table.register(name, |payload: Vec<u8>| payload);
    }
    // Register the three actual handlers at the end
    table.register("lookup_raw", |payload: Vec<u8>| payload);
    table.register_json("lookup_json", |tick: Tick| tick);
    table.register_binary("lookup_binary", |tick: Tick| tick);

    let t = make_tick();
    let json_payload = serde_json::to_vec(&t).unwrap();
    let mut wire_payload = Vec::with_capacity(t.encode_size());
    t.encode(&mut wire_payload);

    group.bench_function("register() in 100-cmd table", |b| {
        b.iter_batched(
            || b"bench".to_vec(),
            |p| {
                let resp = table.call(black_box("lookup_raw"), p).unwrap();
                black_box(resp);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("register_json() in 100-cmd table", |b| {
        b.iter_batched(
            || json_payload.clone(),
            |p| {
                let resp = table.call(black_box("lookup_json"), p).unwrap();
                black_box(resp);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("register_binary() in 100-cmd table", |b| {
        b.iter_batched(
            || wire_payload.clone(),
            |p| {
                let resp = table.call(black_box("lookup_binary"), p).unwrap();
                black_box(resp);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    handler_raw_echo,
    handler_json_echo,
    handler_binary_echo,
    handler_json_with_work,
    handler_binary_with_work,
    handler_lookup_100,
);
criterion_main!(benches);
