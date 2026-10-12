#!/usr/bin/env python3
"""Frontier chart for docs/BACKTEST.md (B1): margin freed vs haircut, one line
per hedge_break_bps; filled markers breach at H = 1 day, hollow ones do not.

Reads docs/backtest/frontier.json (from `cd services/risk && npm run frontier`).
Run from clearinghouse/:

    python -I scripts/backtest/frontier_chart.py docs/backtest/frontier.json docs/backtest/frontier.png
"""
import json
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402

# Categorical slots 1-3 of the validated reference palette (light surface).
# hedge_break 0..5,000 give identical numbers on this book (the SOL gap
# scenario dominates), so they are one series; the script asserts it.
SERIES = {5000: "#2a78d6", 7000: "#eb6834", 8000: "#1baf7a"}
LABEL = {5000: "0 to 5,000", 7000: "7,000", 8000: "8,000"}
SURFACE = "#fcfcfb"
TEXT = "#0b0b0b"
TEXT_2 = "#52514e"
GRID = "#e4e3df"


def main(src: str, dst: str) -> None:
    with open(src, encoding="utf-8") as f:
        pts = json.load(f)["points"]
    by = {(p["hedgeBreakBps"], p["haircutBps"]): p for p in pts}
    for (hb, h), p in by.items():
        if hb < 5000:
            assert p["freedPctDaily"] == by[(5000, h)]["freedPctDaily"], (hb, h)
            assert p["breachesDaily"] == by[(5000, h)]["breachesDaily"], (hb, h)
    fig, ax = plt.subplots(figsize=(9, 5.2), facecolor=SURFACE)
    ax.set_facecolor(SURFACE)
    for hb, color in SERIES.items():
        rs = sorted((p for p in pts if p["hedgeBreakBps"] == hb), key=lambda p: p["haircutBps"])
        x = [p["haircutBps"] / 100 for p in rs]
        y = [p["freedPctDaily"] for p in rs]
        ax.plot(x, y, color=color, lw=2, zorder=2, label=f"hedge_break {LABEL[hb]} bps")
        for p, xi, yi in zip(rs, x, y):
            filled = p["breachesDaily"] > 0
            ax.plot(xi, yi, "o", ms=8, zorder=3, mec=SURFACE, mew=2 if filled else 0,
                    mfc=color if filled else SURFACE)
            if not filled:
                ax.plot(xi, yi, "o", ms=8, zorder=4, mfc="none", mec=color, mew=2)
        ax.annotate(LABEL[hb], (x[-1], y[-1]), xytext=(8, 0), textcoords="offset points",
                    va="center", fontsize=9, color=TEXT_2)
    fl = sorted((p for p in pts if p["hedgeBreakBps"] == 5000), key=lambda p: p["haircutBps"])
    ax.plot([p["haircutBps"] / 100 for p in fl], [p["freedPctDailyFloor"] for p in fl], "--",
            color=TEXT_2, lw=1.5, zorder=2, label="hedge_break 5,000 + floor (A2 proposal)")
    ax.annotate("floor", (fl[-1]["haircutBps"] / 100, fl[-1]["freedPctDailyFloor"]), xytext=(8, 0),
                textcoords="offset points", va="center", fontsize=9, color=TEXT_2)
    ax.plot([], [], "o", ms=8, mfc=TEXT_2, mec=TEXT_2, ls="none", label="1 breach at H = 1 day (2022-11-09)")
    ax.plot([], [], "o", ms=8, mfc="none", mec=TEXT_2, mew=2, ls="none", label="0 breaches at H = 1 day")
    ax.set_xlabel("haircut (%)", color=TEXT)
    ax.set_ylabel("mean margin freed (% of Σ IM), H = 1 day", color=TEXT)
    ax.set_title("Efficiency vs safety: long SOL / short ETH, 60 days (2022-11, 2026-09)\n"
                 "0 breaches at H = 20 min for every setting", color=TEXT, fontsize=11)
    ax.set_ylim(bottom=0)
    ax.grid(True, color=GRID, lw=0.8, zorder=0)
    for s in ax.spines.values():
        s.set_color(GRID)
    ax.tick_params(colors=TEXT_2)
    ax.legend(frameon=False, fontsize=8.5, labelcolor=TEXT, loc="upper right")
    fig.tight_layout()
    fig.savefig(dst, dpi=130, facecolor=SURFACE)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
