# Risk service (offline calibration and backtest)

Turns committed price data into **proposed** governance payloads, and replays
those parameters against history. It holds no key and makes no network call. It
does not submit anything: the risk authority reviews the output and sends the
instructions itself (bounded and timelocked on chain, [`SPEC.md`](../../SPEC.md) §4).
Status: [`../../docs/STATUS.md`](../../docs/STATUS.md).

```bash
npm ci
npm test            # tsc + 16 tests (determinism, PSD repair, clamping, bounds vs Rust, no look-ahead)
npm run calibrate   # writes params/calibration.json, prints the report and diff
npm run backtest    # writes ../../docs/backtest/results.json, prints the tables in docs/BACKTEST.md
```

`--data DIR`, `--current FILE` and `--out FILE` override the defaults. The
default current parameters are `params/localnet.json` (the localnet/test
defaults). For a real deployment, export the on-chain values into the same
shape.

## What it computes

Input: `crates/ch_math/tests/data/klines_20m/*.csv`. Provenance and unknowns,
including the licence, are in that directory's `README.md`.

| Output | Definition |
| --- | --- |
| `shock_bps` | 99% quantile, over every 1-day window (72 × 20-minute bars), of the largest adverse excursion from the window's open, either direction, from bar highs/lows |
| `gap_bps` | 99% quantile of the idiosyncratic 1-day move `|R_a − β·R_b|` (β from 20-minute log returns); the on-chain bound forces `gap ≥ shock` |
| `basis_bps`, `basis_decorr_ppm` | **not estimated**: one-venue spot data has no venue basis. Current values are kept, and the output says so |
| ρ | median of rolling 3-day (216-bar) Pearson correlations of 20-minute log returns |
| `corr_stress_ppm` (δ) | median − 5% quantile of those rolling correlations |
| `hedge_break_bps` (M3) | 99% quantile of the largest simultaneous adverse move of both legs, divided by the sum of their proposed shocks. **Off-chain only**: `ch_core` does not read it yet |

Then:

1. **PSD check** of the correlation matrix. If it fails, the matrix is
   repaired with Higham's nearest correlation matrix (alternating projections),
   and the output reports the eigenvalues before and after and the distance
   moved. A repair is never silent.
2. **Clamp** to every on-chain bound and step limit: shock and gap floors and
   ceilings, `gap ≥ shock`, shock within ½×–2× of the current value, |Δρ| ≤ 0.25,
   δ/β floors, hedge-break range. Each clamp is listed with its raw value and
   rule. The constants are mirrored from Rust, and a test fails if they drift.
3. Round, then **re-check PSD after clamping**. If it fails, no payload is
   produced (fail closed).

Output (`params/calibration.json`):

- `payloads`: the exact instruction arguments
  (`update_correlations.entries[{a,b,rho}]`,
  `set_asset_risk[{asset_id, params:{shock_bps,gap_bps,basis_bps}}]`,
  `set_stress_params`);
- `diff`: current vs proposed, and whether each change applies at once
  (credit-reducing) or waits for the timelock, using the same rules as
  `governance.rs`;
- `inputs` with SHA-256, `sample`, `estimates` (raw, before clamping), `psd`,
  `clamps`.

Example run on the committed data (`npm run calibrate`):

```
PSD: ok (min eigenvalue 0.199031); no repair
Clamps applied:
  SOL.shock_bps: 4291 -> 2000 (step limit: within [old/2, 2·old] of 1000)
  SOL.gap_bps: 1722 -> 2000 (bounds [shock_bps, 10000])
  ETH.shock_bps: 2136 -> 1600 (step limit: within [old/2, 2·old] of 800)
  ETH.gap_bps: 811 -> 1600 (bounds [shock_bps, 10000])
```

The data asks for a 42.91% SOL shock, but one update may only double it.
Reaching the estimate takes three timelocked updates.

## Backtest

See [`docs/BACKTEST.md`](../../docs/BACKTEST.md). It found **one breach** at
H = 1 day (2022-11-09). That page reports and explains it; nothing was tuned
away.

## Code

| File | Role |
| --- | --- |
| `src/data.ts` | CSV loaders (validated), alignment, gap-free segments |
| `src/stats.ts` | quantile (type 7), Pearson, OLS β |
| `src/psd.ts` | Jacobi eigenvalues, PSD check, Higham repair with report |
| `src/bounds.ts` | on-chain bounds, clamps, credit-reducing classification |
| `src/calibrate.ts` | estimates → PSD → clamp → payloads + diff |
| `src/backtest.ts` | look-ahead-guarded daily replay using `testbench/src/portfolio.ts` |
| `src/cli.ts` | `calibrate` / `backtest` commands |
