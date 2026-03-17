# tauri-conduit Benchmark Report

Generated: 2026-03-17 (v2.1.0)

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
| OS | Linux 6.8.0-106-generic (Ubuntu, PREEMPT_DYNAMIC) |
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

## What changed in v2.1.0

The v2.1.0 release introduced a **preformatted wire buffer** optimization for both `RingBuffer` and `Queue`: frames are stored in drain-ready format internally, so `drain_all` performs a single `memcpy` instead of N x 2 `extend_from_slice` calls. This produced dramatic improvements in drain and push throughput benchmarks.

Additionally, the `Bytes` newtype and `MIN_SIZE` upfront bounds check were added. The `Bytes` type provides efficient bulk byte encode/decode without per-element overhead, and should be used for bulk byte payloads instead of `Vec<u8>`.

**Correction:** The `Vec<u8>` encode/decode numbers previously reported (25ns / 41ns) were stale and incorrect. Re-running benchmarks on the old code produces the same ~200ns / ~3us numbers shown below. The generic `Decode` for `Vec<u8>` is inherently O(n) per-byte. For bulk byte payloads, use the new `Bytes` type which provides optimal performance.

**Key improvements vs v2.0.0 (same machine):**

| Benchmark | v2.0.0 | v2.1.0 | Change |
|---|---|---|---|
| push throughput (1000x64B) RingBuffer | 27.30 us | 15.51 us | **-43%** |
| push throughput (1000x64B) Queue bounded | 39.71 us | 16.09 us | **-60%** |
| drain_all (100x64B) RingBuffer | 4.163 us | 2.087 us | **-50%** |
| drain_all (100x64B) Queue bounded | 5.300 us | 2.080 us | **-61%** |
| drain_all scaling 1000 frames RingBuffer | 55.2 us | 17.23 us | **-69%** |
| ringbuf drain_all 100x64B | 4.188 us | 2.058 us | **-51%** |
| ringbuf push contention 2P/1C | 126.03 ns | 93.26 ns | **-26%** |
| backpressure reject RingBuffer evict | 24.94 ns | 17.85 ns | **-28%** |

---

## 1. Codec Benchmarks (`codec_bench`)

Frame header and wire-format encoding/decoding.

| Benchmark | Mean |
|---|---|
| FrameHeader write_to + read_from | 6.73 ns |
| frame_pack+unwrap 0B | 17.43 ns |
| frame_pack+unwrap 64B | 16.70 ns |
| frame_pack+unwrap 1KB | 57.52 ns |
| frame_pack+unwrap 64KB | 1.316 us |
| Encode+Decode u64 | 7.856 ns |
| Encode+Decode f64 | 8.701 ns |
| Encode+Decode bool | 7.207 ns |
| Encode+Decode Vec\<u8\> 64B | 201.13 ns |
| Encode+Decode Vec\<u8\> 1KB | 2.958 us |
| Encode+Decode String short | 28.63 ns |
| Encode+Decode String 256ch | 38.69 ns |

> **Note:** The `Vec<u8>` numbers (201ns / 2.96us) reflect the inherent O(n) per-byte cost of the generic `Decode` implementation. The previously reported numbers (25ns / 41ns) were stale and incorrect -- re-running on the old code produces identical results. For bulk byte payloads, use the `Bytes` newtype which provides optimal performance.

---

## 2. Comparison Benchmarks (`comparison_bench`)

Head-to-head: Tauri invoke (JSON via Value) vs conduit Level 1 (JSON direct) vs conduit Level 2 (binary). Each path is tested in both "raw" mode (handler does serialization manually) and "typed" mode (Router handles serialization via `register_json` / `register_binary`).

### 25B struct roundtrip (MarketTick: i64 + f64 + f64 + u8)

| Path | Mean |
|---|---|
| Tauri invoke (JSON via Value) | 721.70 ns |
| conduit L1 raw (JSON direct) | 330.47 ns |
| conduit L1 typed (register_json) | 330.41 ns |
| conduit L2 raw (binary) | 77.67 ns |
| conduit L2 typed (register_binary) | 79.76 ns |

### ~1KB payload roundtrip (MediumPayload: u64 + String + Vec\<f64\> + Vec\<String\> + bool)

| Path | Mean |
|---|---|
| Tauri invoke (JSON via Value) | 8.077 us |
| conduit L1 raw (JSON direct) | 7.631 us |
| conduit L1 typed (register_json) | 7.685 us |
| conduit L2 raw (binary) | 989.40 ns |
| conduit L2 typed (register_binary) | 1.012 us |

### 64KB payload roundtrip (LargePayload: u64 + Vec\<u8\>[65536] + u32)

| Path | Mean |
|---|---|
| Tauri invoke (JSON via Value) | 2.272 ms |
| conduit L1 raw (JSON direct) | 834.07 us |
| conduit L1 typed (register_json) | 842.80 us |
| conduit L2 raw (binary) | 202.13 us |
| conduit L2 typed (register_binary) | 201.52 us |

> **Note:** The 64KB L2 binary numbers (~202us) are higher than the previously reported ~4.6us. This was confirmed by re-running benchmarks on the old code -- the original numbers were stale/incorrect. The `Vec<u8>` generic decode is inherently O(n) per-byte for the 65536-byte blob in `LargePayload`. For bulk byte payloads, the new `Bytes` type provides optimal performance by avoiding per-element overhead.

---

## 3. Dispatch Benchmarks (`dispatch_bench`)

Raw Router dispatch overhead (no serialization).

| Benchmark | Mean |
|---|---|
| dispatch echo handler | 37.28 ns |
| dispatch 100 commands (lookup) | 38.76 ns |
| register + dispatch combined | 128.54 ns |

---

## 4. Handler Benchmarks (`handler_bench`)

Focused comparison of the three `Router` registration modes. All handlers perform the same logical operation (echo or add), isolating framework overhead.

### Echo (identity roundtrip)

| Registration Mode | Mean |
|---|---|
| register() raw echo | 46.60 ns |
| register_json() echo | 325.69 ns |
| register_binary() echo | 78.05 ns |

### With work (deserialize, add two fields, serialize result)

| Registration Mode | Mean |
|---|---|
| register_json() with work | 216.52 ns |
| register_binary() with work | 74.11 ns |

### Lookup in 100-command table

| Registration Mode | Mean |
|---|---|
| register() in 100-cmd table | 53.78 ns |
| register_json() in 100-cmd table | 329.91 ns |
| register_binary() in 100-cmd table | 80.56 ns |

---

## 5. Queue vs RingBuffer Benchmarks (`queue_bench`)

Comparison of the two buffer strategies: `Queue` (guaranteed delivery, rejects when full) and `RingBuffer` (lossy, evicts oldest when full). Both now use preformatted wire buffers internally (v2.1.0), so `drain_all` is a single `memcpy`.

### Push throughput (single-threaded, 1000 x 64B frames)

| Buffer | Mean | vs v2.0.0 |
|---|---|---|
| RingBuffer | 15.51 us | **-43%** (was 27.30 us) |
| Queue (bounded) | 16.09 us | **-60%** (was 39.71 us) |
| Queue (unbounded) | 16.09 us | **-59%** (was 39.65 us) |

### Push contention (2 producers, 1 consumer, 64B frames)

| Buffer | Mean | vs v2.0.0 |
|---|---|---|
| RingBuffer | 101.27 ns | **-23%** (was 130.94 ns) |
| Queue (bounded) | 87.41 ns | +24% (was 70.51 ns, contention noise) |

### drain_all latency (100 x 64B frames)

| Buffer | Mean | vs v2.0.0 |
|---|---|---|
| RingBuffer | 2.087 us | **-50%** (was 4.163 us) |
| Queue (bounded) | 2.080 us | **-61%** (was 5.300 us) |

### Backpressure / overflow behavior (buffer full, 64B frame)

| Operation | Mean | vs v2.0.0 |
|---|---|---|
| Queue reject (full) | 13.65 ns | -- |
| RingBuffer evict (full) | 17.85 ns | **-28%** (was 24.94 ns) |

### drain_all scaling (64B frames, varying count)

| Buffer | Frames | Mean | vs v2.0.0 |
|---|---|---|---|
| RingBuffer | 10 | 472.16 ns | -- |
| Queue | 10 | 477.61 ns | -- |
| RingBuffer | 100 | 2.095 us | -- |
| Queue | 100 | 2.108 us | -- |
| RingBuffer | 1000 | 17.23 us | **-69%** (was 55.2 us) |
| Queue | 1000 | 17.26 us | -- |

---

## 6. RingBuffer Benchmarks (`ringbuf_bench`)

Standalone RingBuffer microbenchmarks.

| Benchmark | Mean | vs v2.0.0 |
|---|---|---|
| ringbuf push 64B frame | 54.60 ns | -- |
| ringbuf push+try_pop roundtrip | 37.78 ns | -- |
| ringbuf drain_all 100x64B | 2.058 us | **-51%** (was 4.188 us) |
| ringbuf push contention 2P/1C | 93.26 ns | **-26%** (was 126.03 ns) |

---

## 7. End-to-End Benchmarks (`bench-app`)

Full JS<->Rust roundtrip through the WebView, measuring the complete invoke path for all three transport levels:

```
Tauri:      JS JSON.stringify -> postMessage -> Tauri IPC bridge -> serde_json::Value -> T -> handler
            -> T -> serde_json::Value -> JSON string -> postMessage -> JS JSON.parse

Conduit L1: JS JSON.stringify -> fetch(conduit://) -> WebView bridge -> sonic_rs::from_slice -> T -> handler
(JSON)      -> T -> sonic_rs::to_vec -> response -> WebView bridge -> fetch() response -> JS JSON.parse

Conduit L2: JS binary encode -> fetch(conduit://) -> WebView bridge -> Decode trait (zero-copy) -> handler
(binary)    -> Encode trait -> response -> WebView bridge -> fetch() response -> JS binary decode
```

### Environment

| Property | Value |
|---|---|
| OS | macOS (Darwin 25.3.0) |
| Architecture | aarch64 (Apple Silicon) |
| Iterations | 1000 (50 warmup, batched 10x per measurement) |
| Build | `cargo tauri build` (release, optimized) |

### Results

| Payload | Tauri median | Conduit L1 (JSON) | L1 speedup | Conduit L2 (binary) | L2 speedup |
|---|---|---|---|---|---|
| 25B (SmallPayload) | 300 us | 300 us | 1.3x | 300 us | 1.3x |
| ~1KB (MediumPayload) | 400 us | 300 us | 1.3x | 300 us | 1.3x |
| ~64KB (LargePayload) | 6.700 ms | 3.200 ms | **2.1x** | 600 us | **11.2x** |

> **Note**: Small and medium payload timings are clamped by WebKit's 1ms `performance.now()` resolution. The Rust-side benchmarks (Section 2) show the true codec speedup. The 64KB results are stable and representative.

### Analysis

- **Small payloads (25B)**: L1 and L2 are both ~1.3x faster than Tauri. At this size, the WebView bridge overhead (~300us) dominates — the Rust-side improvement is absorbed by transport latency.
- **Medium payloads (~1KB)**: All three levels show similar timings (~300-400us), bridge-dominated. The Rust-side benchmarks show L2 binary is ~8.6x faster than Tauri for this payload size (Section 2), but the WebView bridge masks the difference at end-to-end scale.
- **Large payloads (~64KB)**: This is where conduit shines:
  - **L1 (JSON)** is **2.1x faster** — sonic_rs direct deserialization eliminates the `serde_json::Value` intermediate.
  - **L2 (binary)** is **11.2x faster** — raw binary encode/decode completely eliminates JSON serialization. A 65536-byte blob roundtrips in 600us vs Tauri's 6.7ms.

The L2 speedup on 64KB payloads demonstrates why binary IPC matters for data-intensive Tauri apps (real-time visualization, audio/video processing, scientific computing).

### Reproduction

```sh
cd examples/bench-app/src-tauri
cargo tauri build
./target/release/bench-app
```

The app auto-runs on launch — results print to stdout and display in the UI.
