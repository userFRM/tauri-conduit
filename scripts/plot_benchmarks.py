#!/usr/bin/env python3
# Copyright 2026 tauri-conduit Contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Generate benchmark visualizations for tauri-conduit.

Produces two publication-quality charts:
1. Payload scaling: grouped bars — Tauri invoke vs conduit L1 (JSON) vs conduit L2 (binary)
2. Bottleneck breakdown: stacked bars explaining WHERE time is spent in Tauri invoke

Data from: cargo bench (Intel i7-10700KF @ 3.80 GHz, Linux 6.8, Rust 1.85)
"""

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import numpy as np
from pathlib import Path

# ---------------------------------------------------------------------------
# Measured data (2026-03-17, v2.1.0)
# ---------------------------------------------------------------------------

payload_labels = ["25 B struct\n(MarketTick)", "~1 KB payload\n(mixed types)", "64 KB payload\n(Vec<u8>)"]
payload_short  = ["25 B", "~1 KB", "64 KB"]

# Latencies in nanoseconds (mean from criterion)
tauri_ns   = [721.70,   8_077,   2_272_000]
l1_json_ns = [330.47,   7_631,     834_070]
l2_bin_ns  = [ 79.76,   1_012,     201_520]

# Bottleneck decomposition percentages
transport_pct = [54, 6,  63]
json_pct      = [35, 82, 28]
binary_pct    = [11, 13,  9]

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def format_latency(ns: float) -> str:
    """Human-readable latency string."""
    if ns < 1_000:
        return f"{ns:.0f} ns"
    elif ns < 1_000_000:
        return f"{ns / 1_000:.1f} \u00b5s"
    else:
        return f"{ns / 1_000_000:.2f} ms"

def ns_to_us(ns: float) -> float:
    return ns / 1_000

# ---------------------------------------------------------------------------
# Color palette — accessible, professional
# ---------------------------------------------------------------------------

TAURI_GREY   = "#6B7280"   # neutral grey for the baseline
L1_BLUE      = "#3B82F6"   # bright blue for L1 JSON
L2_GREEN     = "#10B981"   # emerald green for L2 binary (the hero)

# Bottleneck breakdown
TRANSPORT_CORAL  = "#F97316"  # orange — WebView transport tax
JSON_SLATE       = "#6366F1"  # indigo — JSON codec overhead
BINARY_EMERALD   = "#10B981"  # green  — binary codec (the floor)

BG_COLOR = "#FAFBFC"
GRID_COLOR = "#E5E7EB"

# ---------------------------------------------------------------------------
# Global style
# ---------------------------------------------------------------------------
plt.rcParams.update({
    "font.family": "sans-serif",
    "font.sans-serif": ["Inter", "Segoe UI", "Helvetica Neue", "Arial", "DejaVu Sans"],
    "font.size": 12,
    "axes.spines.top": False,
    "axes.spines.right": False,
    "axes.facecolor": BG_COLOR,
    "figure.facecolor": "white",
    "axes.edgecolor": "#D1D5DB",
    "axes.linewidth": 0.8,
    "axes.labelcolor": "#374151",
    "xtick.color": "#6B7280",
    "ytick.color": "#6B7280",
})

OUT_DIR = Path(__file__).resolve().parent.parent / "docs" / "images"
OUT_DIR.mkdir(parents=True, exist_ok=True)

# =========================================================================
# Chart 1: Payload Scaling — Tauri invoke vs conduit (grouped bars)
# =========================================================================

fig1, ax1 = plt.subplots(figsize=(14, 7))

x = np.arange(len(payload_labels))
n_bars = 3
bar_width = 0.24
gap = 0.03

# Convert to microseconds for the y-axis
tauri_us = [ns_to_us(n) for n in tauri_ns]
l1_us    = [ns_to_us(n) for n in l1_json_ns]
l2_us    = [ns_to_us(n) for n in l2_bin_ns]

offsets = [-(bar_width + gap), 0, (bar_width + gap)]

bars_tauri = ax1.bar(
    x + offsets[0], tauri_us, bar_width,
    label="Tauri invoke", color=TAURI_GREY,
    edgecolor="white", linewidth=0.8, zorder=3,
)
bars_l1 = ax1.bar(
    x + offsets[1], l1_us, bar_width,
    label="conduit L1 (JSON)", color=L1_BLUE,
    edgecolor="white", linewidth=0.8, zorder=3,
)
bars_l2 = ax1.bar(
    x + offsets[2], l2_us, bar_width,
    label="conduit L2 (binary)", color=L2_GREEN,
    edgecolor="white", linewidth=0.8, zorder=3,
)

ax1.set_yscale("log")
ax1.set_ylabel("Roundtrip Latency (microseconds, log scale)", fontsize=13, fontweight="medium")
ax1.set_xticks(x)
ax1.set_xticklabels(payload_labels, fontsize=12, fontweight="medium")

# Custom y-axis formatting
ax1.yaxis.set_major_formatter(ticker.FuncFormatter(
    lambda val, pos: f"{val:,.0f} \u00b5s" if val >= 1 else f"{val * 1000:.0f} ns"
))
ax1.yaxis.set_minor_formatter(ticker.NullFormatter())

# Grid
ax1.grid(axis="y", alpha=0.4, color=GRID_COLOR, zorder=0)
ax1.set_axisbelow(True)

# Title
ax1.set_title(
    "Tauri invoke vs conduit  \u2014  Roundtrip Latency by Payload Size",
    fontsize=16, fontweight="bold", color="#111827", pad=20,
)

# Legend
legend = ax1.legend(
    fontsize=11, loc="upper left", frameon=True, framealpha=0.95,
    edgecolor="#E5E7EB", fancybox=True,
)
legend.get_frame().set_linewidth(0.8)

# --- Value labels and speedup annotations ---
def add_bar_label(ax, bar, value_ns, color, is_baseline=False):
    """Add latency text above a bar."""
    rect = bar.patches[0] if hasattr(bar, 'patches') else bar
    x_center = rect.get_x() + rect.get_width() / 2
    y_top = rect.get_height()
    label = format_latency(value_ns)
    ax.text(
        x_center, y_top, label,
        ha="center", va="bottom", fontsize=8.5,
        color=color, fontweight="bold",
        transform=ax.transData,
    )

for i in range(len(payload_labels)):
    # Latency labels on each bar
    for bars, ns_list, color in [
        (bars_tauri, tauri_ns, TAURI_GREY),
        (bars_l1, l1_json_ns, L1_BLUE),
        (bars_l2, l2_bin_ns, L2_GREEN),
    ]:
        rect = bars[i]
        x_c = rect.get_x() + rect.get_width() / 2
        y_t = rect.get_height()
        label = format_latency(ns_list[i])
        ax1.text(
            x_c, y_t * 1.08, label,
            ha="center", va="bottom", fontsize=8.5,
            color=color, fontweight="bold",
        )

    # Speedup badges for L2 binary — the hero metric
    speedup_l2 = tauri_ns[i] / l2_bin_ns[i]
    rect = bars_l2[i]
    x_c = rect.get_x() + rect.get_width() / 2
    y_t = rect.get_height()
    badge_text = f"{speedup_l2:.0f}x faster"

    # Draw a badge below the bar value
    ax1.annotate(
        badge_text,
        xy=(x_c, y_t * 0.55),
        fontsize=10, fontweight="bold", color="white",
        ha="center", va="center",
        bbox=dict(
            boxstyle="round,pad=0.3",
            facecolor=L2_GREEN, edgecolor="none", alpha=0.95,
        ),
        zorder=5,
    )

    # Speedup for L1 (smaller, above the bar)
    speedup_l1 = tauri_ns[i] / l1_json_ns[i]
    rect_l1 = bars_l1[i]
    x_c_l1 = rect_l1.get_x() + rect_l1.get_width() / 2
    y_t_l1 = rect_l1.get_height()
    ax1.text(
        x_c_l1, y_t_l1 * 1.45, f"{speedup_l1:.1f}x",
        ha="center", va="bottom", fontsize=9, fontweight="bold",
        color=L1_BLUE, alpha=0.85,
    )

# Tighten
fig1.tight_layout(pad=1.5)
fig1.savefig(OUT_DIR / "payload-scaling.png", dpi=180, bbox_inches="tight",
             facecolor="white", edgecolor="none")
print(f"Saved: {OUT_DIR / 'payload-scaling.png'}")


# =========================================================================
# Chart 2: Bottleneck Breakdown — WHERE the time goes
# =========================================================================

fig2, ax2 = plt.subplots(figsize=(14, 7))

x2 = np.arange(len(payload_short))
bar_w = 0.45

# Absolute nanosecond breakdown
transport_ns = [tauri_ns[i] - l1_json_ns[i] for i in range(3)]
json_codec_ns = [l1_json_ns[i] - l2_bin_ns[i] for i in range(3)]
binary_codec_ns = list(l2_bin_ns)

# Convert to microseconds
transport_us = [ns_to_us(n) for n in transport_ns]
json_us      = [ns_to_us(n) for n in json_codec_ns]
binary_us    = [ns_to_us(n) for n in binary_codec_ns]

b_binary = ax2.bar(
    x2, binary_us, bar_w,
    label="Binary codec (the floor)", color=BINARY_EMERALD,
    edgecolor="white", linewidth=0.8, zorder=3,
)
b_json = ax2.bar(
    x2, json_us, bar_w, bottom=binary_us,
    label="JSON serialization overhead", color=JSON_SLATE,
    edgecolor="white", linewidth=0.8, zorder=3,
)
b_transport = ax2.bar(
    x2, transport_us, bar_w,
    bottom=[j + b for j, b in zip(json_us, binary_us)],
    label="WebView transport tax", color=TRANSPORT_CORAL,
    edgecolor="white", linewidth=0.8, zorder=3, alpha=0.9,
)

ax2.set_yscale("log")
ax2.set_ylabel("Latency contribution (microseconds, log scale)", fontsize=13, fontweight="medium")
ax2.set_xticks(x2)
ax2.set_xticklabels(payload_short, fontsize=14, fontweight="bold")
ax2.set_xlabel("Payload Size", fontsize=13, fontweight="medium")

ax2.yaxis.set_major_formatter(ticker.FuncFormatter(
    lambda val, pos: f"{val:,.0f} \u00b5s" if val >= 1 else f"{val * 1000:.0f} ns"
))
ax2.yaxis.set_minor_formatter(ticker.NullFormatter())

ax2.grid(axis="y", alpha=0.4, color=GRID_COLOR, zorder=0)
ax2.set_axisbelow(True)

ax2.set_title(
    "Where Does the Time Go?  \u2014  Tauri invoke Latency Breakdown",
    fontsize=16, fontweight="bold", color="#111827", pad=20,
)

legend2 = ax2.legend(
    fontsize=11, loc="upper left", frameon=True, framealpha=0.95,
    edgecolor="#E5E7EB", fancybox=True,
)
legend2.get_frame().set_linewidth(0.8)

# --- Percentage labels inside each segment ---
segment_data = [
    (binary_us,    binary_pct,    [0]*3,                                     "white"),
    (json_us,      json_pct,      binary_us,                                 "white"),
    (transport_us, transport_pct, [j + b for j, b in zip(json_us, binary_us)], "white"),
]

for seg_vals, seg_pcts, bottoms, text_color in segment_data:
    for i in range(3):
        if seg_pcts[i] < 8:
            continue  # too small to label inside
        y_mid = bottoms[i] + seg_vals[i] / 2
        ax2.text(
            x2[i], y_mid, f"{seg_pcts[i]}%",
            ha="center", va="center", fontsize=13, fontweight="bold",
            color=text_color, zorder=6,
        )

# --- Total latency labels on top of each stacked bar ---
for i in range(3):
    total_us = transport_us[i] + json_us[i] + binary_us[i]
    ax2.text(
        x2[i], total_us * 1.12, format_latency(tauri_ns[i]),
        ha="center", va="bottom", fontsize=11, fontweight="bold",
        color="#374151",
    )

# --- Callout annotation for the 1KB story ---
i_1kb = 1
total_1kb = transport_us[1] + json_us[1] + binary_us[1]
ax2.annotate(
    "At ~1 KB, JSON serialization is 82% of total cost.\n"
    "Transport overhead is just 6%.\n"
    "That's why conduit L1 (JSON) barely wins here\n"
    "but L2 (binary) still delivers 8x.",
    xy=(x2[i_1kb] + bar_w / 2, total_1kb * 0.7),
    xytext=(x2[i_1kb] + 0.75, total_1kb * 3.5),
    fontsize=10.5, color="#374151", linespacing=1.5,
    arrowprops=dict(
        arrowstyle="-|>", color=TRANSPORT_CORAL,
        lw=1.8, connectionstyle="arc3,rad=-0.15",
    ),
    bbox=dict(
        boxstyle="round,pad=0.6", facecolor="#FFF7ED",
        edgecolor=TRANSPORT_CORAL, alpha=0.95, linewidth=1.2,
    ),
    zorder=7,
)

fig2.tight_layout(pad=1.5)
fig2.savefig(OUT_DIR / "bottleneck-breakdown.png", dpi=180, bbox_inches="tight",
             facecolor="white", edgecolor="none")
print(f"Saved: {OUT_DIR / 'bottleneck-breakdown.png'}")


# =========================================================================
# Clean up: remove drain-improvement.png if it exists
# =========================================================================
drain_path = OUT_DIR / "drain-improvement.png"
if drain_path.exists():
    drain_path.unlink()
    print(f"Deleted: {drain_path}")

# =========================================================================
# Print summary table
# =========================================================================
print("\n## Benchmark Summary\n")
print("| Payload | Tauri invoke | conduit L1 (JSON) | conduit L2 (binary) | L2 speedup |")
print("|---------|-------------|-------------------|--------------------:|:----------:|")
for i, label in enumerate(payload_short):
    speedup = tauri_ns[i] / l2_bin_ns[i]
    print(f"| {label:>6} | {format_latency(tauri_ns[i]):>11} "
          f"| {format_latency(l1_json_ns[i]):>17} "
          f"| {format_latency(l2_bin_ns[i]):>19} "
          f"| **{speedup:.0f}x** |")

print("\n## Bottleneck Decomposition\n")
print("| Payload | Transport | JSON codec | Binary codec |")
print("|---------|:---------:|:----------:|:------------:|")
for i, label in enumerate(payload_short):
    print(f"| {label:>6} | {transport_pct[i]:>3}% | {json_pct[i]:>3}% | {binary_pct[i]:>3}% |")
