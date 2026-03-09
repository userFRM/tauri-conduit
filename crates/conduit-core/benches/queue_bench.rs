//! Benchmarks comparing Queue (guaranteed delivery) vs RingBuffer (lossy).
//!
//! Both use the same Mutex<VecDeque> interior but differ in overflow semantics:
//! - RingBuffer: drops oldest frames when full (lossy back-pressure)
//! - Queue: rejects new pushes when full (Error::ChannelFull)
//!
//! This benchmark measures:
//! - Push throughput (single-threaded, 1000 frames)
//! - Push throughput under contention (2 producers, 1 consumer)
//! - drain_all latency (after filling buffer)
//! - Backpressure cost: how fast does Queue reject when full?

use conduit_core::{Queue, RingBuffer};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::sync::Arc;
use std::thread;

// ============================================================================
// Push throughput: single-threaded, 1000 frames of 64 bytes
// ============================================================================

fn push_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("push throughput (1000x64B)");
    let frame = vec![0xABu8; 64];

    group.bench_function("RingBuffer", |b| {
        b.iter_batched(
            || RingBuffer::new(256 * 1024), // plenty of room, no drops
            |rb| {
                for _ in 0..1000 {
                    let _ = rb.push(black_box(&frame));
                }
                black_box(rb.frame_count());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("Queue (bounded)", |b| {
        b.iter_batched(
            || Queue::new(256 * 1024), // plenty of room, no rejects
            |q| {
                for _ in 0..1000 {
                    let _ = q.push(black_box(&frame));
                }
                black_box(q.frame_count());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("Queue (unbounded)", |b| {
        b.iter_batched(
            || Queue::unbounded(),
            |q| {
                for _ in 0..1000 {
                    let _ = q.push(black_box(&frame));
                }
                black_box(q.frame_count());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================================
// Push throughput under contention: 2 producers, 1 consumer
// ============================================================================

fn push_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("push contention (2P/1C)");
    let frame = vec![0xABu8; 64];

    group.bench_function("RingBuffer", |b| {
        b.iter_custom(|iters| {
            let rb = Arc::new(RingBuffer::new(256 * 1024));
            let iters_per_producer = iters / 2;

            let start = std::time::Instant::now();

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

    group.bench_function("Queue (bounded)", |b| {
        b.iter_custom(|iters| {
            let q = Arc::new(Queue::new(256 * 1024));
            let iters_per_producer = iters / 2;

            let start = std::time::Instant::now();

            let producers: Vec<_> = (0..2)
                .map(|_| {
                    let q = Arc::clone(&q);
                    let f = frame.clone();
                    thread::spawn(move || {
                        for _ in 0..iters_per_producer {
                            let _ = q.push(black_box(&f));
                        }
                    })
                })
                .collect();

            let q_c = Arc::clone(&q);
            let consumer = thread::spawn(move || {
                let mut drained = 0u64;
                for _ in 0..iters {
                    if q_c.try_pop().is_some() {
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

    group.finish();
}

// ============================================================================
// drain_all latency after filling buffer with 100 frames
// ============================================================================

fn drain_all_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("drain_all (100x64B)");
    let frame = vec![0xABu8; 64];

    group.bench_function("RingBuffer", |b| {
        let rb = RingBuffer::new(256 * 1024);
        b.iter(|| {
            for _ in 0..100 {
                let _ = rb.push(&frame);
            }
            let blob = rb.drain_all();
            black_box(blob);
        });
    });

    group.bench_function("Queue (bounded)", |b| {
        let q = Queue::new(256 * 1024);
        b.iter(|| {
            for _ in 0..100 {
                let _ = q.push(&frame);
            }
            let blob = q.drain_all();
            black_box(blob);
        });
    });

    group.finish();
}

// ============================================================================
// Backpressure cost: how fast does Queue reject when full?
// ============================================================================

fn backpressure_reject(c: &mut Criterion) {
    let mut group = c.benchmark_group("backpressure reject");
    let frame = vec![0xABu8; 64];

    // Queue with capacity for exactly 1 frame (cost = 4 + 64 = 68 bytes)
    group.bench_function("Queue reject (full)", |b| {
        let q = Queue::new(68);
        q.push(&frame).unwrap(); // fill it
        b.iter(|| {
            let result = q.push(black_box(&frame));
            let _ = black_box(result);
        });
    });

    // RingBuffer with capacity for exactly 1 frame -- overflow eviction
    group.bench_function("RingBuffer evict (full)", |b| {
        let rb = RingBuffer::new(68);
        let _ = rb.push(&frame); // fill it
        b.iter(|| {
            let dropped = rb.push(black_box(&frame));
            black_box(dropped);
        });
    });

    group.finish();
}

// ============================================================================
// drain_all latency with varying frame counts
// ============================================================================

fn drain_all_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("drain_all scaling");
    let frame = vec![0xABu8; 64];

    for count in [10, 100, 1000] {
        group.bench_function(format!("RingBuffer {count} frames"), |b| {
            let rb = RingBuffer::new(count * 128); // enough room
            b.iter(|| {
                for _ in 0..count {
                    let _ = rb.push(&frame);
                }
                let blob = rb.drain_all();
                black_box(blob);
            });
        });

        group.bench_function(format!("Queue {count} frames"), |b| {
            let q = Queue::new(count * 128);
            b.iter(|| {
                for _ in 0..count {
                    let _ = q.push(&frame);
                }
                let blob = q.drain_all();
                black_box(blob);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    push_throughput,
    push_contention,
    drain_all_latency,
    backpressure_reject,
    drain_all_scaling,
);
criterion_main!(benches);
