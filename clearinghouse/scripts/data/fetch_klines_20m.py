#!/usr/bin/env python3
"""Fetch Binance spot 1-minute klines and aggregate them to 20-minute windows.

Source: Binance public market-data archive (data.binance.vision), monthly
1m klines, verified against the published .CHECKSUM (SHA-256) files.

Output: crates/ch_math/tests/data/klines_20m/<SYMBOL>-<YYYY-MM>.csv with
columns window_start_ms,open,high,low,close (prices in micro-USD, integers).

One window = one credit lifetime (credit_ttl_slots = 3,000 slots, ~20 min).
The window's open is the refresh baseline; high/low give the largest move
from that baseline inside the window.

Usage (from clearinghouse/):
    python scripts/data/fetch_klines_20m.py SOLUSDT,ETHUSDT 2022-11,2026-09

Needs only the Python standard library. Writes SHA-256 of every source
archive to crates/ch_math/tests/data/klines_20m/SOURCES.txt.
"""
import csv
import hashlib
import io
import os
import sys
import urllib.request
import zipfile
from decimal import Decimal

BASE = "https://data.binance.vision/data/spot/monthly/klines"
WINDOW_MS = 20 * 60 * 1000
OUT = os.path.join("crates", "ch_math", "tests", "data", "klines_20m")


def get(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=120) as r:
        return r.read()


def micro(price: str) -> int:
    return int((Decimal(price) * 1_000_000).to_integral_value())


def main() -> None:
    symbols = sys.argv[1].split(",")
    months = sys.argv[2].split(",")
    os.makedirs(OUT, exist_ok=True)
    sources = []
    for sym in symbols:
        for month in months:
            name = f"{sym}-1m-{month}.zip"
            url = f"{BASE}/{sym}/1m/{name}"
            data = get(url)
            digest = hashlib.sha256(data).hexdigest()
            published = get(url + ".CHECKSUM").decode().split()[0]
            if digest != published:
                sys.exit(f"checksum mismatch for {name}: {digest} != {published}")
            sources.append(f"{url}  sha256={digest}")
            windows = {}
            with zipfile.ZipFile(io.BytesIO(data)) as z:
                with z.open(z.namelist()[0]) as f:
                    for row in csv.reader(io.TextIOWrapper(f)):
                        t = int(row[0])
                        if t > 10**14:  # some archives use microseconds
                            t //= 1000
                        o, h, l, c = (micro(x) for x in row[1:5])
                        w = t - t % WINDOW_MS
                        if w not in windows:
                            windows[w] = [o, h, l, c, t]
                        else:
                            cur = windows[w]
                            cur[1] = max(cur[1], h)
                            cur[2] = min(cur[2], l)
                            if t >= cur[4]:
                                cur[3], cur[4] = c, t
            path = os.path.join(OUT, f"{sym}-{month}.csv")
            with open(path, "w", newline="\n") as out:
                out.write("window_start_ms,open,high,low,close\n")
                for w in sorted(windows):
                    o, h, l, c, _ = windows[w]
                    out.write(f"{w},{o},{h},{l},{c}\n")
            print(f"{path}: {len(windows)} windows")
    with open(os.path.join(OUT, "SOURCES.txt"), "w", newline="\n") as f:
        f.write("Binance spot 1m klines (data.binance.vision), aggregated to 20-minute\n")
        f.write("windows by scripts/data/fetch_klines_20m.py. Checksums verified.\n\n")
        f.write("\n".join(sources) + "\n")


if __name__ == "__main__":
    main()
