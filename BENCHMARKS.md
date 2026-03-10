# tauri-conduit Benchmark Report

Generated: 2026-03-10 (v2.0.0)

## Measurement scope

**These numbers measure Rust-side dispatch only.** They exclude the WebView bridge, `fetch()` overhead, and JavaScript parsing. The full end-to-end path for an `invoke()` call is:

```
JS: JSON.stringify(args)           ← not measured
JS: fetch("conduit://...")          ← not measured (WebView intercepts)
WebView: custom protocol dispatch   ← not measured (platform-specific, ~1-5ms)
Rust: handler dispatch              ← MEASURED (this report)
WebView: response delivery          ← not measured
JS: JSON.parse(response)            ← not measured
```

For small payloads, the WebView bridge overhead (~1-5ms) dominates, so the 2.4x Rust improvement translates to a modest end-to-end gain. For large payloads (64KB+), serialization dominates the bridge overhead, and conduit's binary mode delivers substantial end-to-end improvement.

End-to-end benchmarks require a running Tauri app and vary by platform (WKWebView on macOS, WebView2 on Windows, webkit2gtk on Linux). See the [Tradeoffs section](README.md#tradeoffs) in the README for a full discussion.

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
| FrameHeader write_to + read_from | 6.688 ns | 6.654 ns | 6.730 ns |
| frame_pack+unwrap 0B | 17.186 ns | 16.833 ns | 17.634 ns |
| frame_pack+unwrap 64B | 16.681 ns | 16.570 ns | 16.813 ns |
| frame_pack+unwrap 1KB | 60.288 ns | 59.994 ns | 60.607 ns |
| frame_pack+unwrap 64KB | 1.7062 us | 1.6953 us | 1.7177 us |
| Encode+Decode u64 | 7.834 ns | 7.794 ns | 7.873 ns |
| Encode+Decode f64 | 8.681 ns | 8.638 ns | 8.722 ns |
| Encode+Decode bool | 8.252 ns | 8.229 ns | 8.276 ns |
| Encode+Decode Vec\<u8\> 64B | 25.081 ns | 24.930 ns | 25.229 ns |
| Encode+Decode Vec\<u8\> 1KB | 41.249 ns | 40.979 ns | 41.553 ns |
| Encode+Decode String short | 26.743 ns | 26.569 ns | 26.899 ns |
| Encode+Decode String 256ch | 39.798 ns | 39.539 ns | 40.057 ns |

---

## 2. Comparison Benchmarks (`comparison_bench`)

Head-to-head: Tauri invoke (JSON via Value) vs conduit Level 1 (JSON direct) vs conduit Level 2 (binary). Each path is tested in both "raw" mode (handler does serialization manually) and "typed" mode (Router handles serialization via `register_json` / `register_binary`).

### 25B struct roundtrip (MarketTick: i64 + f64 + f64 + u8)

| Path | Mean | Low | High |
|---|---|---|---|
| Tauri invoke (JSON via Value) | 714.27 ns | 708.50 ns | 721.49 ns |
| conduit L1 raw (JSON direct) | 333.68 ns | 331.86 ns | 335.54 ns |
| conduit L1 typed (register_json) | 332.56 ns | 330.95 ns | 334.07 ns |
| conduit L2 raw (binary) | 78.039 ns | 77.601 ns | 78.420 ns |
| conduit L2 typed (register_binary) | 79.678 ns | 79.055 ns | 80.309 ns |

### ~1KB payload roundtrip (MediumPayload: u64 + String + Vec\<f64\> + Vec\<String\> + bool)

| Path | Mean | Low | High |
|---|---|---|---|
| Tauri invoke (JSON via Value) | 8.531 us | 8.491 us | 8.572 us |
| conduit L1 raw (JSON direct) | 7.617 us | 7.570 us | 7.660 us |
| conduit L1 typed (register_json) | 7.622 us | 7.583 us | 7.661 us |
| conduit L2 raw (binary) | 991.36 ns | 982.72 ns | 1.000 us |
| conduit L2 typed (register_binary) | 991.80 ns | 981.83 ns | 1.002 us |

### 64KB payload roundtrip (LargePayload: u64 + Vec\<u8\>[65536] + u32)

| Path | Mean | Low | High |
|---|---|---|---|
| Tauri invoke (JSON via Value) | 2.304 ms | 2.294 ms | 2.314 ms |
| conduit L1 raw (JSON direct) | 859.05 us | 856.06 us | 861.85 us |
| conduit L1 typed (register_json) | 820.78 us | 816.75 us | 824.96 us |
| conduit L2 raw (binary) | 4.592 us | 4.315 us | 4.850 us |
| conduit L2 typed (register_binary) | 4.563 us | 4.276 us | 4.823 us |

---

## 3. Dispatch Benchmarks (`dispatch_bench`)

Raw Router dispatch overhead (no serialization).

| Benchmark | Mean | Low | High |
|---|---|---|---|
| dispatch echo handler | 37.352 ns | 37.085 ns | 37.632 ns |
| dispatch 100 commands (lookup) | 39.417 ns | 39.181 ns | 39.642 ns |
| register + dispatch combined | 129.72 ns | 128.95 ns | 130.49 ns |

---

## 4. Handler Benchmarks (`handler_bench`)

Focused comparison of the three `Router` registration modes. All handlers perform the same logical operation (echo or add), isolating framework overhead.

### Echo (identity roundtrip)

| Registration Mode | Mean | Low | High |
|---|---|---|---|
| register() raw echo | 47.466 ns | 47.205 ns | 47.721 ns |
| register_json() echo | 329.53 ns | 327.76 ns | 331.29 ns |
| register_binary() echo | 79.003 ns | 78.552 ns | 79.445 ns |

### With work (deserialize, add two fields, serialize result)

| Registration Mode | Mean | Low | High |
|---|---|---|---|
| register_json() with work | 223.58 ns | 221.85 ns | 225.54 ns |
| register_binary() with work | 73.309 ns | 72.790 ns | 73.857 ns |

### Lookup in 100-command table

| Registration Mode | Mean | Low | High |
|---|---|---|---|
| register() in 100-cmd table | 52.610 ns | 52.314 ns | 52.907 ns |
| register_json() in 100-cmd table | 334.56 ns | 332.62 ns | 336.45 ns |
| register_binary() in 100-cmd table | 81.068 ns | 80.467 ns | 81.727 ns |

---

## 5. Queue vs RingBuffer Benchmarks (`queue_bench`)

Comparison of the two buffer strategies: `Queue` (guaranteed delivery, rejects when full) and `RingBuffer` (lossy, evicts oldest when full).

### Push throughput (single-threaded, 1000 x 64B frames)

| Buffer | Mean | Low | High |
|---|---|---|---|
| RingBuffer | 27.296 us | 27.173 us | 27.418 us |
| Queue (bounded) | 39.711 us | 39.520 us | 39.897 us |
| Queue (unbounded) | 39.651 us | 39.406 us | 39.892 us |

### Push contention (2 producers, 1 consumer, 64B frames)

| Buffer | Mean | Low | High |
|---|---|---|---|
| RingBuffer | 130.94 ns | 127.88 ns | 133.95 ns |
| Queue (bounded) | 70.507 ns | 69.425 ns | 71.616 ns |

### drain_all latency (100 x 64B frames)

| Buffer | Mean | Low | High |
|---|---|---|---|
| RingBuffer | 4.163 us | 4.146 us | 4.180 us |
| Queue (bounded) | 5.300 us | 5.265 us | 5.335 us |

### Backpressure / overflow behavior (buffer full, 64B frame)

| Operation | Mean | Low | High |
|---|---|---|---|
| Queue reject (full) | 13.565 ns | 13.501 ns | 13.625 ns |
| RingBuffer evict (full) | 24.937 ns | 24.757 ns | 25.133 ns |

### drain_all scaling (64B frames, varying count)

| Buffer | Frames | Mean | Low | High |
|---|---|---|---|---|
| RingBuffer | 10 | 438.22 ns | 435.87 ns | 440.52 ns |
| Queue | 10 | 443.85 ns | 441.74 ns | 445.87 ns |
| RingBuffer | 100 | 4.739 us | 4.716 us | 4.760 us |
| Queue | 100 | 4.717 us | 4.677 us | 4.763 us |
| RingBuffer | 1000 | 55.185 us | 54.718 us | 55.713 us |
| Queue | 1000 | 53.917 us | 53.595 us | 54.240 us |

---

## 6. RingBuffer Benchmarks (`ringbuf_bench`)

Standalone RingBuffer microbenchmarks.

| Benchmark | Mean | Low | High |
|---|---|---|---|
| ringbuf push 64B frame | 44.505 ns | 44.218 ns | 44.773 ns |
| ringbuf push+try_pop roundtrip | 34.918 ns | 34.745 ns | 35.082 ns |
| ringbuf drain_all 100x64B | 4.188 us | 4.150 us | 4.227 us |
| ringbuf push contention 2P/1C | 126.03 ns | 123.72 ns | 128.26 ns |

---

## 7. End-to-End Benchmarks (`bench-app`)

Full JS↔Rust roundtrip through the WebView, measuring the complete invoke path:

```
Tauri:   JS JSON.stringify → postMessage → Tauri IPC bridge → serde_json::Value → T → handler
         → T → serde_json::Value → JSON string → postMessage → JS JSON.parse

Conduit: JS JSON.stringify → fetch(conduit://) → WebView bridge → sonic_rs::from_slice → T → handler
         → T → sonic_rs::to_vec → response → WebView bridge → fetch() response → JS JSON.parse
```

### Environment

| Property | Value |
|---|---|
| OS | macOS (Darwin 25.3.0) |
| Architecture | aarch64 (Apple Silicon) |
| Iterations | 1000 (50 warmup, batched 10x per measurement) |
| Build | `cargo tauri build` (release, optimized) |

### Results

| Payload | Tauri median | Conduit median | Speedup |
|---|---|---|---|
| 25B (MarketTick) | 300 us | 200 us | 1.5x |
| ~1KB (MediumPayload) | 300 us | 300 us | 1.0x |
| ~64KB (LargePayload) | 6.700 ms | 3.100 ms | **2.2x** |

### Analysis

- **Small payloads (25B)**: Conduit is ~1.5x faster. The WebView bridge overhead (~200-300us) dominates at this scale, so the Rust-side 2.1x improvement (714ns → 333ns) is mostly absorbed.
- **Medium payloads (~1KB)**: Roughly equal at the measurement resolution. Both paths are dominated by WebView bridge latency.
- **Large payloads (~64KB)**: Conduit is **2.2x faster**. At this size, serialization dominates bridge overhead — sonic_rs direct deserialization (no intermediate `serde_json::Value`) delivers substantial end-to-end improvement.

### Reproduction

```sh
cd examples/bench-app/src-tauri
cargo tauri build
./target/release/bench-app
```

The app auto-runs on launch — results print to stdout and display in the UI.
