//! M1: how often does the price guard allow invalidation, and what refresh
//! load does that create? Measured on recorded data (HISTORICAL).
//!
//!     cargo test -p ch_math --test move_guard_load -- --nocapture
//!
//! Data: Binance spot 1-minute klines aggregated to 20-minute windows
//! (`tests/data/klines_20m/`, sources and SHA-256 in SOURCES.txt, produced by
//! `scripts/data/fetch_klines_20m.py`). One window ≈ one credit lifetime
//! (`credit_ttl_slots` 3,000 ≈ 20 min); the window open is the refresh
//! baseline and high/low give the largest move from it. A window "breaches"
//! when `price_move_bps(open, high or low) > max_move_bps`, the same
//! comparison `invalidate_portfolio` makes.
//!
//! Caveats (also in docs/PERFORMANCE.md): exchange spot highs/lows, not the
//! on-chain oracle; refreshes are not aligned to clock windows in practice;
//! a breach only *permits* invalidation (someone must submit the evidence).

use ch_math::price_move_bps;
use std::collections::HashMap;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/klines_20m");
const THRESHOLDS: [u64; 4] = [200, 300, 500, 1_000];
const MONTHS: [&str; 2] = ["2022-11", "2026-09"];
const WINDOWS_PER_DAY: f64 = 72.0;

/// window_start_ms → worst move in bps from the window open.
fn worst_moves(symbol: &str, month: &str) -> HashMap<u64, u64> {
    let path = format!("{DIR}/{symbol}-{month}.csv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut out = HashMap::new();
    for line in text.lines().skip(1).map(str::trim_end) {
        let f: Vec<u64> = line
            .split(',')
            .map(|x| x.parse().expect("integer"))
            .collect();
        let (t, open, high, low) = (f[0], f[1], f[2], f[3]);
        assert!(low <= open && open <= high, "{path}: bad OHLC at {t}");
        out.insert(t, price_move_bps(open, high).max(price_move_bps(open, low)));
    }
    out
}

fn pct(n: usize, d: usize) -> f64 {
    100.0 * n as f64 / d as f64
}

#[test]
fn breach_frequency_and_refresh_load_on_recorded_data() {
    println!(
        "\n| Month | Series | windows | > 2% | > 3% | > 5% | > 10% |\n| --- | --- | ---: | ---: | ---: | ---: | ---: |"
    );
    let mut load = Vec::new();
    for month in MONTHS {
        let sol = worst_moves("SOLUSDT", month);
        let eth = worst_moves("ETHUSDT", month);
        assert_eq!(
            sol.len(),
            2_160,
            "{month}: a 30-day month has 2,160 windows"
        );
        assert_eq!(eth.len(), 2_160);
        let pair: Vec<(u64, u64)> = sol
            .iter()
            .filter_map(|(t, s)| eth.get(t).map(|e| (*s, *e)))
            .collect();
        assert_eq!(pair.len(), 2_160, "{month}: windows must align");

        let mut pair_rates = Vec::new();
        for (name, moves) in [
            ("SOL", sol.values().copied().collect::<Vec<_>>()),
            ("ETH", eth.values().copied().collect()),
            (
                "SOL/ETH pair (either leg)",
                pair.iter().map(|(s, e)| *s.max(e)).collect(),
            ),
        ] {
            let rates: Vec<f64> = THRESHOLDS
                .iter()
                .map(|&t| pct(moves.iter().filter(|&&m| m > t).count(), moves.len()))
                .collect();
            // A tighter guard can only breach more often.
            assert!(rates.windows(2).all(|w| w[0] >= w[1]), "{month} {name}");
            println!(
                "| {month} | {name} | {} | {:.2}% | {:.2}% | {:.2}% | {:.2}% |",
                moves.len(),
                rates[0],
                rates[1],
                rates[2],
                rates[3]
            );
            if name.starts_with("SOL/ETH") {
                pair_rates = rates;
            }
        }
        load.push((month, pair_rates));
    }

    // Refresh load for one active 2-leg portfolio (SOL long / ETH short).
    // Baseline: the keeper refreshes every 2,700 slots ≈ 80 times/day. Each
    // breach adds one invalidate_portfolio and one early refresh.
    println!(
        "\n| Month | max_move_bps | breach windows / day | extra tx / day | total tx / day | base fee / day (lamports) |\n| --- | ---: | ---: | ---: | ---: | ---: |"
    );
    for (month, rates) in &load {
        for (i, &t) in THRESHOLDS.iter().enumerate() {
            if t != 300 && t != 500 {
                continue;
            }
            let breaches = WINDOWS_PER_DAY * rates[i] / 100.0;
            let extra = 2.0 * breaches;
            let total = 80.0 + extra;
            println!(
                "| {month} | {t} | {breaches:.2} | {extra:.2} | {total:.1} | {:.0} |",
                total * 5_000.0
            );
        }
    }
}
