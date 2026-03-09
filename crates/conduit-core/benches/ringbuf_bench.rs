use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::sync::Arc;
use std::thread;

use conduit_core::RingBuffer;

fn push_single(c: &mut Criterion) {
    let frame = vec![0xABu8; 64];

    c.bench_function("ringbuf push 64B frame", |b| {
        b.iter_batched(
            || RingBuffer::new(64 * 1024),
            |rb| {
                let dropped = rb.push(black_box(&frame));
                black_box(dropped)
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn push_pop_roundtrip(c: &mut Criterion) {
    let frame = vec![0xABu8; 64];

    c.bench_function("ringbuf push+try_pop roundtrip", |b| {
        let rb = RingBuffer::new(64 * 1024);
        b.iter(|| {
            let _ = rb.push(black_box(&frame));
            let popped = rb.try_pop();
            black_box(popped);
        });
    });
}

fn drain_all_100_frames(c: &mut Criterion) {
    let frame = vec![0xABu8; 64];

    c.bench_function("ringbuf drain_all 100x64B", |b| {
        let rb = RingBuffer::new(64 * 1024);
        b.iter(|| {
            for _ in 0..100 {
                let _ = rb.push(&frame);
            }
            let blob = rb.drain_all();
            black_box(blob);
        });
    });
}

fn push_contention(c: &mut Criterion) {
    let frame = vec![0xABu8; 64];

    c.bench_function("ringbuf push contention 2P/1C", |b| {
        b.iter_custom(|iters| {
            let rb = Arc::new(RingBuffer::new(64 * 1024));
            let iters_per_producer = iters / 2;

            let start = std::time::Instant::now();

            // Spawn 2 producer threads.
            let producers: Vec<_> = (0..2)
                .map(|_| {
                    let rb = Arc::clone(&rb);
                    let f = frame.clone();
                    thread::spawn(move || {
                        for _ in 0..iters_per_producer {
                            let _ = rb.push(black_box(&f));
                        }
                    })
                })
                .collect();

            // 1 consumer thread draining.
            let rb_c = Arc::clone(&rb);
            let consumer = thread::spawn(move || {
                let mut drained = 0u64;
                for _ in 0..iters {
                    if rb_c.try_pop().is_some() {
                        drained += 1;
                    }
                }
                drained
            });

            for p in producers {
                p.join().unwrap();
            }
            let _ = consumer.join().unwrap();

            start.elapsed()
        });
    });
}

criterion_group!(
    benches,
    push_single,
    push_pop_roundtrip,
    drain_all_100_frames,
    push_contention,
);
criterion_main!(benches);
