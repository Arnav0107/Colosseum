# Crash-replay backtest

Does the post-credit requirement cover what a hedged book actually lost? This
page answers that for one book, on two months of data, with one breach found.
Status: [`STATUS.md`](STATUS.md).

**Every number on this page** comes from

```bash
cd services/risk && npm run backtest      # tables; writes docs/backtest/results.json
python -I scripts/backtest/chart.py docs/backtest/results.json docs/backtest/backtest.png
```

or from `scripts/backtest/run.sh`, which runs the tests and then both commands.
The output is byte-identical across runs (checked with `cmp`), and the tests
assert determinism.

## Horizon caveat (read first)

Credit lives about 20 minutes (`credit_ttl_slots` ≤ 3,000 slots), and the
price guard can invalidate it sooner. The **primary horizon here is
H = 1 day**, a deliberately **conservative proxy**: the book is assumed to
sit for a full day on parameters and credit fixed at the open. **This is not a
validation at the credit's own horizon.** A supplementary run at H = 20
minutes uses 20-minute bars (open, high, low, close), **not minute data**. The
price guard (`max_move_bps`) is **not** modelled in either run.

## Setup

| Item | Value |
| --- | --- |
| Book | long SOL, short ETH, $50,000 notional each side at the day's open; IM 20% ($10,000), MM 10% ($5,000) per leg; venue 0 and venue 1 |
| Data | Binance spot 20-minute klines, SOLUSDT/ETHUSDT, 2022-11 and 2026-09 (provenance: [`crates/ch_math/tests/data/README.md`](../crates/ch_math/tests/data/README.md)) |
| Evaluation days | every UTC midnight with a full horizon of bars: 60 days (30 + 30) |
| Credit engine | `testbench/src/portfolio.ts`, the TypeScript port of `ch_math`, bit-identical to Rust through the shared vectors. Same model as `ch_core`, **including the floor `credit ≤ (Σ IM − R_p)⁺` (A2)**; no governance scenarios set. A second column adds the calibrated hedge-break scenario. A pre-A2 reference column (no floor) is kept for before/after |
| Credit params | haircut 20%, per-leg cap 75% of IM and `IM − MM`, per-user cap $100,000 (`services/risk/params/localnet.json`) |
| Initial risk params | localnet defaults: SOL 10%/10%, ETH 8%/8%, ρ = 0.8, δ = 0.1, β = 0.02 |
| Sanity check | at the initial params the engine gives **7,111,111,110** micro-USD, the worked-example value pinned in the vectors and the LiteSVM suite (`npm test`, "engine sanity") |

**Parameters known up to each date, no look-ahead.** Governance runs the
calibration ([`services/risk`](../services/risk/README.md)) once a day, using
only bars that **closed before d − 1 day**. The production timelock is
≥ 216,000 slots (~1 day), so a change proposed from data up to d − 1 day takes
effect at d at the earliest. Each update is clamped to the on-chain bounds and
step limits relative to the previous day's parameters. Until there is enough
data (4 days), the initial params stay in force. This is asserted in code:
`runBacktest` throws if a calibration bar closes after the cutoff, and
`calibrate` re-checks it. Two tests prove it:
- "no look-ahead": corrupting every bar after 2022-11-15 leaves every earlier
  day's parameters and credit unchanged, and does change later days;
- "data cutoff ≥ one timelock before use".

A mutation that lets calibration see the evaluation day fails both tests.

**Metric.**

```
post-credit requirement = Σ IM − credit
realized loss           = worst mark-to-market loss of the book over [d, d + H),
                          from the opening prices at d, combining the SOL low and
                          the ETH high of the same bar (conservative; a closes-only
                          figure is reported alongside)
breach (with CH)        = realized loss > post-credit requirement
breach (baseline)       = realized loss > Σ IM          (no Clearinghouse)
margin of safety        = post-credit requirement − realized loss
```

## Results

### H = 1 day (conservative proxy)

| sample | days | breaches with CH | with CH + hedge-break | baseline (Σ IM only) | worst margin of safety (with CH) |
| --- | ---: | ---: | ---: | ---: | --- |
| all days | 60 | **1** | 1 | 0 | **−$4,166.38** (2022-11-09) |
| 2022-11 | 30 | 1 | 1 | 0 | −$4,166.38 (2022-11-09) |
| 2026-09 | 30 | 0 | 0 | 0 | $16,962.83 (2026-09-11) |

![Post-credit requirement vs realized loss, breaches marked](backtest/backtest.png)

### H = 20 minutes (bar-level supplementary, not minute-level)

| sample | windows | breaches with CH | with CH + hedge-break | baseline | worst margin of safety (with CH) |
| --- | ---: | ---: | ---: | ---: | --- |
| all windows | 4,320 | 0 | 0 | 0 | $4,976.76 (2022-11-08 19:20 UTC) |
| 2022-11 | 2,160 | 0 | 0 | 0 | $4,976.76 |
| 2026-09 | 2,160 | 0 | 0 | 0 | $17,142.19 |

### The breach

| date | params in force (data cutoff) | SOL / ETH shock | R_p | credit | post-credit req | realized loss | loss (closes) | SOL | ETH | with CH | baseline |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| 2022-11-09 | calibrated (2022-11-08) | 17.43% / 9.73% | $8,715.00 | $5,731.96 | $14,268.04 | **$18,434.42** | $16,951.07 | −42.25% | −17.38% | **BREACH** | ok |

**Clearinghouse turned a covered day into a breach.** Σ IM ($20,000) would
have covered the loss; Σ IM − credit ($14,268) did not.

**Why.**

1. **The model in force had never seen a crash.** The parameters on
   11-09 were calibrated from 1–7 Nov 2022 only, which was quiet before the FTX
   collapse. The SOL shock was 17.43%. On 11-09 SOL fell 42.25% from the open
   while ETH fell only 17.38%, so the short ETH leg hedged less than half of
   the SOL loss. R_p ($8,715) was less than half the realized loss. That is
   a model miss, not a calculation error.
2. **The hedge-break scenario did not help.** For this book the SOL gap
   scenario dominates it at the calibrated fraction, so the credit with
   hedge-break equals the credit without.
3. **The floor (A2) does not reach this day.** On 11-09, Σ IM − R_p =
   $20,000 − $8,715 = $11,285, well above the $5,731.96 of credit, so the
   floor does not bind. Before A2, credit depended only on the *ratio* of the
   shocks: scaling every shock and gap by 4 (R_p = Σ IM) still gave $7,111.11.
   With A2 it gives 0 (test "floor (A2): scaling every shock…"), and the
   parameters in force at the end of the sample (SOL 42.95%, ETH 21.37%) give
   $0.00, compared with $5,315.92 before A2 (HYPOTHETICAL rows below).
4. **Step limits slowed the reaction.** After the crash, a shock may at most
   double per update. The SOL shock went 17.43% → 34.86% → 48.10% over the
   next two days.

**At the credit's own horizon there was no breach.** At H = 20 minutes the
worst window (2022-11-08 19:20 UTC) kept a margin of $4,976.76. This does
**not** validate the 1-day result away: it only says the conservative proxy is
where the failure shows.

### Historical replay: 2–15 November 2022

| | value |
| --- | --- |
| Window | book opened 2022-11-02 00:00 UTC, held to 2022-11-15 24:00 UTC (14 days; **not** one margin horizon) |
| Params in force at open | initial (no calibration data yet) |
| Σ IM / credit / post-credit requirement | $20,000.00 / $7,111.11 / $12,888.89 |
| Worst loss over the window | $19,557.37 (closes: $18,414.13) |
| With CH / baseline | **BREACH** / ok |

**The quoted figures, checked against the CSVs.** The reported moves for
2–15 Nov 2022 were SOL ≈ −54% and ETH ≈ −20%. The data gives:

| measurement | SOL | ETH |
| --- | ---: | ---: |
| daily CSV, close 11-01 → close 11-15 | −55.53% | −20.61% |
| daily CSV, close 11-02 → close 11-15 | −53.51% | −17.46% |
| 20-minute klines, open 11-02 00:00 → close 11-15 23:40 | −55.53% | −20.60% |

The quoted pair matches neither convention exactly: SOL ≈ −54% fits the
11-02 close, and ETH ≈ −20% fits the 11-01 close (equivalently the 11-02
open). Measured over one consistent window (11-02 open → 11-15 close), the
figures are **SOL −55.5%, ETH −20.6%**.

### HYPOTHETICAL: decorrelation shock

**HYPOTHETICAL. These moves were chosen, not observed:** SOL −30% and ETH +15%
within one day, so the two hedge legs both lose.

| parameters | loss | Σ IM | credit | post-credit req | with CH | with CH + hedge-break | baseline |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| initial (localnet) | $22,500.00 | $20,000.00 | $7,111.11 | $12,888.89 | BREACH | BREACH | BREACH |
| calibrated, in force 2026-09-30 | $22,500.00 | $20,000.00 | $0.00 (pre-A2: $5,315.92) | $20,000.00 | BREACH | BREACH | BREACH |

Neither venue margin alone nor margin minus credit covers a decorrelation of
this size. With the initial parameters Clearinghouse makes the shortfall
larger by exactly the credit. With the calibrated parameters it adds nothing
(credit 0 since A2).

### A2 floor: before and after

Same command, before (commit `c3b18d1`) and after the floor. Breaches are
unchanged; credit falls sharply, mostly in 2026-09, when the calibration
window already contains the Nov-2022 crash and the model's risk exceeds this
book's 20% IM.

| | before A2 | after A2 |
| --- | ---: | ---: |
| breaches with CH, H = 1 day | 1 (2022-11-09) | 1 (2022-11-09) |
| breaches with CH, H = 20 min | 0 | 0 |
| mean credit, 2022-11 | $5,687.57 | $2,095.66 (floor binds on 21 of 30 days; 0 on 20) |
| mean credit, 2026-09 | $5,264.98 | **$0.00** (0 on all 30 days) |
| mean margin freed, all 60 days | 27.38% of Σ IM | 5.24% |
| worst margin of safety, 2026-09 | $11,758.01 | $16,962.83 |

(Means from `docs/backtest/results.json`, fields `credit` and `creditNoFloor`.)
So for a 5× leveraged book (IM 20%) credit after A2 only exists while the
model's risk stays below the venues' IM.

## Efficiency-vs-safety frontier (B1)

**Every number in this section** comes from

```bash
cd services/risk && npm run frontier     # table; writes docs/backtest/frontier.json
python -I scripts/backtest/frontier_chart.py docs/backtest/frontier.json docs/backtest/frontier.png
```

(run 2026-10-12; tested by `services/risk/src/frontier.test.ts`, which checks
that the setting in force reproduces the backtest above). The same 60 days and
4,320 windows are used, with the calibrated risk parameters in force each day.
Only `haircut_bps` and `hedge_break_bps` are swept. "Freed" is the mean of
credit / Σ IM. It is the same at H = 20 minutes as at H = 1 day, because
parameters change once a day and credit does not depend on the horizon.

![Margin freed vs haircut per hedge_break; filled markers breach at H = 1 day](backtest/frontier.png)

Columns "freed" and "breaches" are the engine **with** the A2 floor. The two
"pre-A2" columns are the same setting without it.

| haircut | hedge_break | freed (% of Σ IM) | breaches H = 1 day (per day) | breaches H = 20 min (per day) | pre-A2: freed | pre-A2: breaches H = 1 day |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 500 | 0 – 5,000 | 6.18 | 1 (0.0167) | 0 (0) | 32.52 | 1 |
| 1,000 | 0 – 5,000 | 5.87 | 1 (0.0167) | 0 (0) | 30.80 | 1 |
| **2,000** (in force) | 0 – 5,000 | **5.24** | 1 (0.0167) | 0 (0) | 27.38 | 1 |
| 3,000 | 0 – 5,000 | 4.61 | 1 (0.0167) | 0 (0) | 23.96 | 1 |
| 5,000 | 0 – 5,000 | 3.35 | 1 (0.0167) | 0 (0) | 17.11 | 1 |
| 7,000 | 0 – 5,000 | 2.06 | 1 (0.0167) | 0 (0) | 10.27 | 1 |
| 2,000 | 6,000 | 4.84 | 1 (0.0167) | 0 (0) | 26.98 | 1 |
| 2,000 | 7,000 | 3.68 | 1 (0.0167) | 0 (0) | 23.99 | 1 |
| 2,000 | 8,000 | 2.40 | 1 (0.0167) | 0 (0) | 15.99 | 1 |
| 6,000 | 8,000 | 1.20 | 1 (0.0167) | 0 (0) | 7.99 | 1 |
| 7,000 | 8,000 | **0.90** | **0** | 0 (0) | 6.00 | 0 |

All 80 settings are in `docs/backtest/frontier.json`. Every setting with
`hedge_break_bps` from 0 to 5,000 gives identical numbers: for this book the
SOL gap scenario dominates.

**What the frontier says.**

1. **The breach does not depend on these two parameters.** The 2022-11-09
   breach survives every haircut up to 60% and every hedge break up to 8,000.
   Of the 80 settings, only haircut 70% with hedge break 8,000 avoids it, and
   that setting frees 0.90% of Σ IM, compared with 5.24% at the setting in
   force (6.00% vs 27.38% without the floor).
   The breach is a model miss (point 1 of "The breach"), not a tuning problem.
2. **H = 20 minutes: 0 breaches at every setting.**
3. **The floor (A2, now on chain) costs most of the credit and removes no
   breach.** At the setting in force, mean freed margin falls from 27.38% to
   5.24%. The calibrated shocks make the model's standalone risk larger than
   the venues' 20% IM, so `Σ IM − R_p` is small. That is the floor doing its
   job, but on this data its price is about 81% of the credit.
4. **Hedge break up to 5,000 is free on this book.** It only costs on books it
   is designed for (same-asset basis trades, RISK_MODEL §3a).

**Recommended default: haircut 2,000 bps, hedge_break 5,000 bps.** This is
judgement, not an optimum. Within the range that frees useful margin, neither
parameter changes the breach count, so the haircut stays where it is.
`hedge_break` goes to 5,000 because it costs nothing here and caps basis-trade
credit independently of ρ. The breach has to be addressed through the model
inputs (longer crash history, B2; the regime throttle, B3), not through these
two parameters.

## Proposed changes

The breach is reported as found. **No parameter was changed to make it go
away.** Each proposal was evaluated by the same command:

1. **Engine floor `credit ≤ (Σ IM − R_p)⁺`.** **Applied (A2).** It removes
   0 of 1 breaches (on 11-09 the model in force underestimated R_p, point 1);
   it was adopted for its own reason, the scale-invariance gap. Results above
   include it.
2. **Seed calibration with history beyond the two committed months** (for
   example 2020-03 and 2021-05 crashes) before the first credit is issued, so
   day one does not run on a calm week's quantile. That needs new data, which
   needs network access and your approval.
3. **What a parameter-only fix would take, for scale:** removing the 11-09
   breach with the current formula requires a haircut of at least
   **78.15%** (7,815 bps; 2,000 bps in force), computed by binary search on
   the engine in `npm run backtest`. That would remove most credit on every
   day. It is **not recommended**, and it is stated only to show that this
   breach is not a tuning problem.

## Limits of this backtest

- One book, one pair, two months, 60 daily evaluations. Daily observations
  overlap with the calibration windows; nothing here is a statistical test.
- Spot prices from one venue. No perpetual marks, funding, or cross-venue
  basis; basis parameters are not estimated.
- Losses combine the SOL low and the ETH high of the same 20-minute bar.
  That is conservative, because the two need not coincide.
- Liquidation, slippage and time-to-liquidate are not modelled. A "breach"
  means the requirement did not cover the mark-to-market loss, not that a
  venue lost money.
