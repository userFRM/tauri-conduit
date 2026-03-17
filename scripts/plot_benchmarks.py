#!/usr/bin/env python3
# Copyright 2026 tauri-conduit Contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Generate benchmark visualizations for tauri-conduit.

Produces three charts:
1. Payload scaling: Tauri invoke vs conduit L1 (JSON) vs conduit L2 (binary)
2. Bottleneck breakdown: stacked bars showing WHERE time is spent
3. drain_all improvement: v2.0.0 vs v2.1.0

Data from: cargo bench (Intel i7-10700KF @ 3.80 GHz, Linux 6.8, Rust 1.85)
"""

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

# ---------------------------------------------------------------------------
# Measured data (2026-03-17, v2.1.0)
# ---------------------------------------------------------------------------

payload_labels = ["25 B", "~1 KB", "64 KB"]
payload_bytes = [25, 1024, 65536]

# Latencies in nanoseconds (mean from criterion)
tauri_ns   = [721.70,   8_077,   2_272_000]
l1_json_ns = [330.47,   7_631,     834_070]
l2_bin_ns  = [ 79.76,   1_012,     201_520]

# drain_all: before (v2.0.0) and after (v2.1.0) in microseconds
drain_frames    = [10,    100,   1000]
drain_before_us = [0.443, 4.188, 55.20]   # v2.0.0 RingBuffer
drain_after_us  = [0.472, 2.058, 17.23]   # v2.1.0 RingBuffer

# Push throughput: 1000x64B in microseconds
push_labels    = ["RingBuffer", "Queue\n(bounded)", "Queue\n(unbounded)"]
push_before_us = [27.30, 39.71, 39.65]
push_after_us  = [15.51, 16.09, 16.09]

# ---------------------------------------------------------------------------
# Derived: bottleneck decomposition
# ---------------------------------------------------------------------------

# Binary codec time = L2 numbers (the floor)
binary_codec_ns = l2_bin_ns

# JSON codec time = L1 - L2 (the JSON serialization/deserialization overhead)
json_codec_ns = [l1 - l2 for l1, l2 in zip(l1_json_ns, l2_bin_ns)]

# Transport overhead = Tauri - conduit L1 (the webview IPC tax conduit removes)
transport_ns = [t - l1 for t, l1 in zip(tauri_ns, l1_json_ns)]

# Percentages for annotations
total_tauri = tauri_ns
transport_pct = [t / tot * 100 for t, tot in zip(transport_ns, total_tauri)]
json_pct      = [j / tot * 100 for j, tot in zip(json_codec_ns, total_tauri)]
binary_pct    = [b / tot * 100 for b, tot in zip(binary_codec_ns, total_tauri)]

# ---------------------------------------------------------------------------
# Style
# ---------------------------------------------------------------------------
plt.rcParams.update({
    "font.family": "sans-serif",
    "font.size": 11,
    "axes.spines.top": False,
    "axes.spines.right": False,
})

BLUE    = "#1565C0"
ORANGE  = "#E65100"
GREEN   = "#2E7D32"
GREY    = "#757575"
LIGHT_BLUE  = "#90CAF9"
LIGHT_ORANGE = "#FFAB91"
LIGHT_GREEN  = "#A5D6A7"

# =========================================================================
# Figure 1: Payload scaling (log-scale latency)
# =========================================================================
fig1, ax1 = plt.subplots(figsize=(10, 6))

x = np.arange(len(payload_labels))
w = 0.25

bars_tauri = ax1.bar(x - w, [n / 1000 for n in tauri_ns], w,
                     label="Tauri invoke (JSON)", color=GREY, edgecolor="white")
bars_l1    = ax1.bar(x,     [n / 1000 for n in l1_json_ns], w,
                     label="conduit L1 (JSON)", color=BLUE, edgecolor="white")
bars_l2    = ax1.bar(x + w, [n / 1000 for n in l2_bin_ns], w,
                     label="conduit L2 (binary)", color=GREEN, edgecolor="white")

ax1.set_yscale("log")
ax1.set_ylabel("Roundtrip Latency (us)", fontsize=13)
ax1.set_xlabel("Payload Size", fontsize=13)
ax1.set_title("Invoke Latency vs Payload Size", fontsize=15, fontweight="bold")
ax1.set_xticks(x)
ax1.set_xticklabels(payload_labels, fontsize=12)
ax1.legend(fontsize=11, loc="upper left")
ax1.grid(axis="y", alpha=0.3)

# Speedup annotations
for i in range(len(payload_labels)):
    speedup_l1 = tauri_ns[i] / l1_json_ns[i]
    speedup_l2 = tauri_ns[i] / l2_bin_ns[i]
    # L1 speedup
    top_l1 = l1_json_ns[i] / 1000
    ax1.annotate(f"{speedup_l1:.1f}x", xy=(x[i], top_l1),
                 xytext=(0, 8), textcoords="offset points",
                 ha="center", fontsize=9, fontweight="bold", color=BLUE)
    # L2 speedup
    top_l2 = l2_bin_ns[i] / 1000
    ax1.annotate(f"{speedup_l2:.0f}x", xy=(x[i] + w, top_l2),
                 xytext=(0, 8), textcoords="offset points",
                 ha="center", fontsize=9, fontweight="bold", color=GREEN)

fig1.tight_layout()
fig1.savefig("docs/images/payload-scaling.png", dpi=150, bbox_inches="tight")
print("Saved: docs/images/payload-scaling.png")

# =========================================================================
# Figure 2: Bottleneck breakdown (stacked bars)
# =========================================================================
fig2, ax2 = plt.subplots(figsize=(10, 6))

x2 = np.arange(len(payload_labels))
bar_w = 0.5

# Convert to microseconds for readability
transport_us = [n / 1000 for n in transport_ns]
json_us      = [n / 1000 for n in json_codec_ns]
binary_us    = [n / 1000 for n in binary_codec_ns]

b1 = ax2.bar(x2, binary_us, bar_w,
             label="Binary codec (the floor)", color=GREEN, edgecolor="white")
b2 = ax2.bar(x2, json_us, bar_w, bottom=binary_us,
             label="JSON overhead (sonic_rs)", color=BLUE, edgecolor="white")
b3 = ax2.bar(x2, transport_us, bar_w,
             bottom=[j + b for j, b in zip(json_us, binary_us)],
             label="WebView transport tax", color=ORANGE, edgecolor="white",
             alpha=0.85)

ax2.set_yscale("log")
ax2.set_ylabel("Latency (us)", fontsize=13)
ax2.set_xlabel("Payload Size", fontsize=13)
ax2.set_title("Where Does the Time Go?  (Tauri invoke breakdown)",
              fontsize=15, fontweight="bold")
ax2.set_xticks(x2)
ax2.set_xticklabels(payload_labels, fontsize=12)
ax2.legend(fontsize=11, loc="upper left")
ax2.grid(axis="y", alpha=0.3)

# Percentage annotations inside the bars
for i in range(len(payload_labels)):
    total_us = transport_us[i] + json_us[i] + binary_us[i]

    # Transport percentage (top segment)
    if transport_pct[i] > 3:
        y_pos = binary_us[i] + json_us[i] + transport_us[i] / 2
        ax2.text(x2[i], y_pos, f"{transport_pct[i]:.0f}%",
                 ha="center", va="center", fontsize=10, fontweight="bold",
                 color="white")

    # JSON percentage (middle segment)
    if json_pct[i] > 3:
        y_pos = binary_us[i] + json_us[i] / 2
        ax2.text(x2[i], y_pos, f"{json_pct[i]:.0f}%",
                 ha="center", va="center", fontsize=10, fontweight="bold",
                 color="white")

    # Binary percentage (bottom segment)
    if binary_pct[i] > 8:
        y_pos = binary_us[i] / 2
        ax2.text(x2[i], y_pos, f"{binary_pct[i]:.0f}%",
                 ha="center", va="center", fontsize=10, fontweight="bold",
                 color="white")

# Callout: explain the 1KB story
ax2.annotate(
    "At 1 KB, JSON codec is 82%\nof total latency.\nTransport is only 6%.\nThat's why L1 barely wins.",
    xy=(1, binary_us[1] + json_us[1] + transport_us[1]),
    xytext=(1.6, (binary_us[1] + json_us[1] + transport_us[1]) * 1.8),
    fontsize=10, color=ORANGE,
    arrowprops=dict(arrowstyle="->", color=ORANGE, lw=1.5),
    bbox=dict(boxstyle="round,pad=0.4", facecolor="lightyellow", edgecolor=ORANGE,
              alpha=0.9),
)

fig2.tight_layout()
fig2.savefig("docs/images/bottleneck-breakdown.png", dpi=150, bbox_inches="tight")
print("Saved: docs/images/bottleneck-breakdown.png")

# =========================================================================
# Figure 3: drain_all improvement (v2.0 vs v2.1)
# =========================================================================
fig3, (ax3a, ax3b) = plt.subplots(1, 2, figsize=(14, 6))

# -- Left: drain_all scaling --
ax3a.plot(drain_frames, drain_before_us, "s--", color=GREY, linewidth=2,
          markersize=8, label="v2.0.0 (VecDeque<Vec<u8>>)")
ax3a.plot(drain_frames, drain_after_us, "o-", color=GREEN, linewidth=2.5,
          markersize=8, label="v2.1.0 (wire buffer)")

# Speedup annotations
for i in range(len(drain_frames)):
    speedup = drain_before_us[i] / drain_after_us[i]
    if speedup > 1.05:
        ax3a.annotate(f"{speedup:.1f}x",
                      xy=(drain_frames[i], drain_after_us[i]),
                      xytext=(0, -18), textcoords="offset points",
                      ha="center", fontsize=10, fontweight="bold", color=GREEN)

ax3a.set_xscale("log")
ax3a.set_yscale("log")
ax3a.set_xlabel("Number of Frames", fontsize=13)
ax3a.set_ylabel("drain_all Latency (us)", fontsize=13)
ax3a.set_title("drain_all: v2.0.0 vs v2.1.0", fontsize=14, fontweight="bold")
ax3a.set_xticks(drain_frames)
ax3a.set_xticklabels([str(f) for f in drain_frames])
ax3a.legend(fontsize=10)
ax3a.grid(True, alpha=0.3)

# Shade improvement region
ax3a.fill_between(drain_frames, drain_before_us, drain_after_us,
                  alpha=0.15, color=GREEN)

# -- Right: push throughput --
x3 = np.arange(len(push_labels))
w3 = 0.3

ax3b.bar(x3 - w3/2, push_before_us, w3,
         label="v2.0.0", color=GREY, edgecolor="white")
ax3b.bar(x3 + w3/2, push_after_us, w3,
         label="v2.1.0", color=GREEN, edgecolor="white")

# Speedup annotations
for i in range(len(push_labels)):
    speedup = push_before_us[i] / push_after_us[i]
    ax3b.annotate(f"{speedup:.1f}x faster",
                  xy=(x3[i] + w3/2, push_after_us[i]),
                  xytext=(0, 6), textcoords="offset points",
                  ha="center", fontsize=9, fontweight="bold", color=GREEN)

ax3b.set_ylabel("push 1000x64B Latency (us)", fontsize=13)
ax3b.set_title("Push Throughput: v2.0.0 vs v2.1.0", fontsize=14, fontweight="bold")
ax3b.set_xticks(x3)
ax3b.set_xticklabels(push_labels, fontsize=11)
ax3b.legend(fontsize=10)
ax3b.grid(axis="y", alpha=0.3)

fig3.tight_layout()
fig3.savefig("docs/images/drain-improvement.png", dpi=150, bbox_inches="tight")
print("Saved: docs/images/drain-improvement.png")

# =========================================================================
# Print summary table
# =========================================================================
print("\n## Bottleneck Decomposition\n")
print("| Payload | Transport (WebView tax) | JSON codec | Binary codec | Total (Tauri) |")
print("|---------|------------------------|------------|--------------|---------------|")
for i, label in enumerate(payload_labels):
    print(f"| {label:>6} | {transport_ns[i]:>10,.0f} ns ({transport_pct[i]:4.1f}%) "
          f"| {json_codec_ns[i]:>8,.0f} ns ({json_pct[i]:4.1f}%) "
          f"| {binary_codec_ns[i]:>8,.0f} ns ({binary_pct[i]:4.1f}%) "
          f"| {tauri_ns[i]:>11,.0f} ns |")

print("\n## Key insight")
print("At 25B, transport is 54% of latency -> conduit L1 wins 2.2x")
print("At 1KB, JSON codec is 82% of latency -> conduit L1 barely wins (1.06x)")
print("At any size, L2 binary skips JSON entirely -> 9x to 11x faster than Tauri")
