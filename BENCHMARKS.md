# tauri-conduit Benchmark Report

Generated: 2026-03-09

## Environment

| Property | Value |
|---|---|
| OS | Linux 6.8.0-101-generic (Ubuntu, PREEMPT_DYNAMIC) |
| Architecture | x86_64 |
| CPU | Intel Core i7-10700KF @ 3.80GHz |
| Cores | 16 |
| RAM | 125 GiB |
| Rust | rustc 1.93.1 (01f6ddf75 2026-02-11) |
| Criterion | 0.5 (plotters backend) |
| Profile | release (optimized) |

## Reproduction

```sh
cd crates/conduit-core && cargo bench 2>&1
```

Individual suites:

```sh
cargo bench -- codec           # codec_bench
cargo bench -- comparison      # comparison_bench
cargo bench -- dispatch        # dispatch_bench
cargo bench -- handler         # handler_bench
cargo bench -- "push throughput\|push contention\|drain_all\|backpressure"  # queue_bench
cargo bench -- ringbuf         # ringbuf_bench
```

---

## 1. Codec Benchmarks (`codec_bench`)

Frame header and wire-format encoding/decoding.

| Benchmark | Mean | Low | High |
|---|---|---|---|
| FrameHeader write_to + read_from | 10.003 ns | 9.955 ns | 10.052 ns |
| frame_pack+unwrap 0B | 16.429 ns | 16.342 ns | 16.512 ns |
| frame_pack+unwrap 64B | 16.599 ns | 16.522 ns | 16.675 ns |
| frame_pack+unwrap 1KB | 59.339 ns | 59.071 ns | 59.603 ns |
| frame_pack+unwrap 64KB | 1.2928 us | 1.2866 us | 1.2987 us |
| Encode+Decode u64 | 7.717 ns | 7.677 ns | 7.756 ns |
| Encode+Decode f64 | 8.483 ns | 8.441 ns | 8.527 ns |
| Encode+Decode bool | 8.045 ns | 8.015 ns | 8.074 ns |
| Encode+Decode Vec\<u8\> 64B | 23.231 ns | 23.112 ns | 23.346 ns |
| Encode+Decode Vec\<u8\> 1KB | 41.439 ns | 41.286 ns | 41.585 ns |
| Encode+Decode String short | 26.363 ns | 26.259 ns | 26.468 ns |
| Encode+Decode String 256ch | 39.789 ns | 39.578 ns | 40.000 ns |

---

## 2. Comparison Benchmarks (`comparison_bench`)

Head-to-head: Tauri invoke (JSON via Value) vs conduit Level 1 (JSON direct) vs conduit Level 2 (binary). Each path is tested in both "raw" mode (handler does serialization manually) and "typed" mode (Router handles serialization via `register_json` / `register_binary`).

### 25B struct roundtrip (MarketTick: i64 + f64 + f64 + u8)

| Path | Mean | Low | High |
|---|---|---|---|
| Tauri invoke (JSON via Value) | 755.58 ns | 751.88 ns | 759.19 ns |
| conduit L1 raw (JSON direct) | 318.81 ns | 317.29 ns | 320.29 ns |
| conduit L1 typed (register_json) | 316.02 ns | 314.78 ns | 317.19 ns |
| conduit L2 raw (binary) | 74.125 ns | 73.741 ns | 74.515 ns |
| conduit L2 typed (register_binary) | 74.246 ns | 73.896 ns | 74.601 ns |

### ~1KB payload roundtrip (MediumPayload: u64 + String + Vec\<f64\> + Vec\<String\> + bool)

| Path | Mean | Low | High |
|---|---|---|---|
| Tauri invoke (JSON via Value) | 8.378 us | 8.338 us | 8.419 us |
| conduit L1 raw (JSON direct) | 4.841 us | 4.816 us | 4.865 us |
| conduit L1 typed (register_json) | 4.793 us | 4.769 us | 4.815 us |
| conduit L2 raw (binary) | 976.28 ns | 970.85 ns | 981.60 ns |
| conduit L2 typed (register_binary) | 989.70 ns | 983.55 ns | 995.60 ns |

### 64KB payload roundtrip (LargePayload: u64 + Vec\<u8\>[65536] + u32)

| Path | Mean | Low | High |
|---|---|---|---|
| Tauri invoke (JSON via Value) | 2.157 ms | 2.148 ms | 2.166 ms |
| conduit L1 raw (JSON direct) | 872.11 us | 868.26 us | 876.03 us |
| conduit L1 typed (register_json) | 871.05 us | 867.99 us | 874.08 us |
| conduit L2 raw (binary) | 4.107 us | 4.014 us | 4.183 us |
| conduit L2 typed (register_binary) | 3.973 us | 3.884 us | 4.047 us |

---

## 3. Dispatch Benchmarks (`dispatch_bench`)

Raw Router dispatch overhead (no serialization).

| Benchmark | Mean | Low | High |
|---|---|---|---|
| dispatch echo handler | 35.165 ns | 35.003 ns | 35.321 ns |
| dispatch 100 commands (lookup) | 36.897 ns | 36.690 ns | 37.108 ns |
| register + dispatch combined | 125.13 ns | 124.56 ns | 125.67 ns |

---

## 4. Handler Benchmarks (`handler_bench`)

Focused comparison of the three `Router` registration modes. All handlers perform the same logical operation (echo or add), isolating framework overhead.

### Echo (identity roundtrip)

| Registration Mode | Mean | Low | High |
|---|---|---|---|
| register() raw echo | 44.735 ns | 44.504 ns | 44.954 ns |
| register_json() echo | 299.85 ns | 298.57 ns | 301.09 ns |
| register_binary() echo | 76.378 ns | 76.015 ns | 76.730 ns |

### With work (deserialize, add two fields, serialize result)

| Registration Mode | Mean | Low | High |
|---|---|---|---|
| register_json() with work | 223.34 ns | 222.04 ns | 224.68 ns |
| register_binary() with work | 71.029 ns | 70.638 ns | 71.405 ns |

### Lookup in 100-command table

| Registration Mode | Mean | Low | High |
|---|---|---|---|
| register() in 100-cmd table | 51.316 ns | 51.041 ns | 51.584 ns |
| register_json() in 100-cmd table | 300.72 ns | 299.14 ns | 302.24 ns |
| register_binary() in 100-cmd table | 79.593 ns | 79.150 ns | 80.019 ns |

---

## 5. Queue vs RingBuffer Benchmarks (`queue_bench`)

Comparison of the two buffer strategies: `Queue` (guaranteed delivery, rejects when full) and `RingBuffer` (lossy, evicts oldest when full).

### Push throughput (single-threaded, 1000 x 64B frames)

| Buffer | Mean | Low | High |
|---|---|---|---|
| RingBuffer | 27.728 us | 27.577 us | 27.886 us |
| Queue (bounded) | 37.602 us | 37.448 us | 37.752 us |
| Queue (unbounded) | 37.703 us | 37.500 us | 37.921 us |

### Push contention (2 producers, 1 consumer, 64B frames)

| Buffer | Mean | Low | High |
|---|---|---|---|
| RingBuffer | 124.38 ns | 122.26 ns | 126.36 ns |
| Queue (bounded) | 66.532 ns | 65.932 ns | 67.104 ns |

### drain_all latency (100 x 64B frames)

| Buffer | Mean | Low | High |
|---|---|---|---|
| RingBuffer | 3.841 us | 3.824 us | 3.857 us |
| Queue (bounded) | 3.854 us | 3.839 us | 3.869 us |

### Backpressure / overflow behavior (buffer full, 64B frame)

| Operation | Mean | Low | High |
|---|---|---|---|
| Queue reject (full) | 11.835 ns | 11.792 ns | 11.878 ns |
| RingBuffer evict (full) | 23.427 ns | 23.334 ns | 23.517 ns |

### drain_all scaling (64B frames, varying count)

| Buffer | Frames | Mean | Low | High |
|---|---|---|---|---|
| RingBuffer | 10 | 318.49 ns | 317.16 ns | 319.72 ns |
| Queue | 10 | 320.50 ns | 318.99 ns | 322.03 ns |
| RingBuffer | 100 | 3.828 us | 3.808 us | 3.849 us |
| Queue | 100 | 3.869 us | 3.855 us | 3.884 us |
| RingBuffer | 1000 | 51.118 us | 50.860 us | 51.398 us |
| Queue | 1000 | 52.280 us | 52.036 us | 52.524 us |

---

## 6. RingBuffer Benchmarks (`ringbuf_bench`)

Standalone RingBuffer microbenchmarks.

| Benchmark | Mean | Low | High |
|---|---|---|---|
| ringbuf push 64B frame | 43.065 ns | 42.789 ns | 43.334 ns |
| ringbuf push+try_pop roundtrip | 33.229 ns | 33.077 ns | 33.375 ns |
| ringbuf drain_all 100x64B | 3.871 us | 3.858 us | 3.885 us |
| ringbuf push contention 2P/1C | 134.78 ns | 132.02 ns | 137.55 ns |
