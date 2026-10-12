#!/usr/bin/env python3
"""Chart for docs/BACKTEST.md: post-credit requirement vs realized loss per day.

Reads the backtest output (docs/backtest/results.json) and writes a PNG.
Needs matplotlib. Run from clearinghouse/:

    python -I scripts/backtest/chart.py docs/backtest/results.json docs/backtest/backtest.png
"""
import json
import sys
from datetime import date

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402


def main(src: str, dst: str) -> None:
    with open(src, encoding="utf-8") as f:
        res = json.load(f)
    rows = res["daily"]["rows"]
    months = sorted({r["date"][:7] for r in rows})
    fig, axes = plt.subplots(1, len(months), figsize=(max(13, 3.6 * len(months)), 4.8), sharey=True)
    if len(months) == 1:
        axes = [axes]
    for ax, m in zip(axes, months):
        rs = [r for r in rows if r["date"].startswith(m)]
        x = [date.fromisoformat(r["date"]) for r in rs]
        ax.plot(x, [r["sumIm"] for r in rs], "--", color="#555555", lw=1.2, label="Σ IM (no Clearinghouse)")
        ax.plot(x, [r["postCredit"] for r in rs], "-", color="#1f6fb2", lw=2, label="post-credit requirement (Σ IM − credit)")
        ax.bar(x, [r["loss"]["pathLoss"] for r in rs], width=0.7, color="#d9a441", alpha=0.75, label="realized worst loss, next 1 day")
        br = [r for r in rs if r["breachWithCh"]]
        if br:
            ax.scatter(
                [date.fromisoformat(r["date"]) for r in br],
                [r["loss"]["pathLoss"] for r in br],
                marker="x",
                s=90,
                color="#c0392b",
                zorder=5,
                label="breach (loss > post-credit requirement)",
            )
            for r in br:
                ax.annotate(r["date"], (date.fromisoformat(r["date"]), r["loss"]["pathLoss"]), textcoords="offset points", xytext=(6, 6), fontsize=8, color="#c0392b")
        ax.set_title(m, fontsize=10)
        ax.tick_params(axis="x", labelrotation=45, labelsize=8)
        ax.grid(axis="y", alpha=0.3)
    axes[0].set_ylabel("USD")
    handles, labels = axes[0].get_legend_handles_labels()
    fig.legend(handles, labels, loc="lower center", ncol=4, fontsize=8, frameon=False)
    fig.suptitle("Crash-replay backtest: long SOL / short ETH, $50k each side, IM 20%. H = 1 day (conservative proxy; credit itself lives ~20 min)", fontsize=11)
    fig.tight_layout(rect=(0, 0.07, 1, 1))
    fig.savefig(dst, dpi=110)
    print(f"wrote {dst}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: chart.py <results.json> <out.png>")
    main(sys.argv[1], sys.argv[2])
