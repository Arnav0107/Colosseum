# Crash-replay backtest

Does the post-credit requirement cover what a hedged book actually lost?
This page answers that for one book (long SOL / short ETH) on five months of
data, four of them crash months. One breach remains at each horizon, on the
same day. Status: [`STATUS.md`](STATUS.md).

**Every number on this page** comes from these commands (run 2026-10-12 at the
commit that last changed this page):

```bash
cd services/risk
npm run calibrate     # params/calibration.json
npm run backtest      # tables; docs/backtest/results.json
npm run frontier      # B1 frontier; docs/backtest/frontier.json
npm run leverage      # B1 leverage sweep; docs/backtest/leverage.json
npm run throttle      # B3 evaluation; docs/backtest/throttle.json
cd ../..
python -I scripts/backtest/chart.py docs/backtest/results.json docs/backtest/backtest.png
python -I scripts/backtest/frontier_chart.py docs/backtest/frontier.json docs/backtest/frontier.png
```

`scripts/backtest/run.sh` runs the tests and all of these. The outputs are
deterministic (CI regenerates them and fails on any diff), and
`services/risk` has 24 tests, including the no-look-ahead guard.

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
| Book | long SOL, short ETH, $50,000 notional each side at the window's open; IM 20% ($10,000), MM 10% ($5,000) per leg (5× leverage); venues 0 and 1, which allowlist each other (A1) |
| Data | Binance spot 20-minute klines, SOLUSDT/ETHUSDT: **2021-05** (May 2021 crash), **2022-05** (Terra-Luna), **2022-06** (3AC/Celsius), **2022-11** (FTX), **2026-09** (calm). Provenance and licence (CC BY-NC-SA 4.0, non-commercial): [`crates/ch_math/tests/data/README.md`](../crates/ch_math/tests/data/README.md) |
| Evaluation days | every UTC midnight with a full horizon of bars: **152 days** (31 + 31 + 30 + 30 + 30); **10,944** 20-minute windows |
| Credit engine | `testbench/src/portfolio.ts`, the TypeScript port of `ch_math`, bit-identical to Rust through the shared vectors. Same model as `ch_core`: stressed model, hedge break at the governed `hedge_break_bps` (A3, 5,000), floor `credit ≤ (Σ IM − R_p)⁺` (A2); no governance scenarios set. Reference columns: without the hedge break (pre-A3) and without the floor (pre-A2) |
| Credit params | haircut 20%, per-leg cap 75% of IM and `IM − MM`, per-user cap $100,000 (`services/risk/params/localnet.json`) |
| Initial risk params | localnet defaults: SOL 10%/10%, ETH 8%/8%, ρ = 0.8, δ = 0.1, β = 0.02 |
| Sanity check | at the initial params the engine gives **7,111,111,110** micro-USD, the worked-example value pinned in the vectors and the LiteSVM suite (`npm test`, "engine sanity") |

**Parameters known up to each date, no look-ahead.** Governance runs the
calibration ([`services/risk`](../services/risk/README.md)) once a day, using
only bars that **closed before d − 1 day**. The production timelock is
≥ 216,000 slots (~1 day), so a change proposed from data up to d − 1 day takes
effect at d at the earliest. Each update is clamped to the on-chain bounds and
step limits relative to the previous day's parameters. Until there is enough
data (4 days), the initial params stay in force. The data has gaps between
months; calibration uses every bar closed before the cutoff, from all
earlier months. This is asserted in code: `runBacktest` throws if a
calibration bar closes after the cutoff, and `calibrate` re-checks it. The
tests "no look-ahead" (corrupting every later bar changes no earlier day) and
"data cutoff ≥ one timelock before use" prove it.

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

| sample | days | breaches with CH | pre-A3 (no hedge break) | baseline (Σ IM only) | worst margin of safety (with CH) | pre-A2 (no floor) |
| --- | ---: | ---: | ---: | ---: | --- | ---: |
| all days | 152 | **1** | 1 | 0 | **−$876.81** (2021-05-19) | 3 |
| 2021-05 | 31 | 1 | 1 | 0 | −$876.81 (2021-05-19) | 1 |
| 2022-05 | 31 | 0 | 0 | 0 | $6,777.07 (2022-05-11) | 1 |
| 2022-06 | 30 | 0 | 0 | 0 | $14,276.49 (2022-06-19) | 0 |
| 2022-11 | 30 | 0 | 0 | 0 | $1,565.58 (2022-11-09) | 1 |
| 2026-09 | 30 | 0 | 0 | 0 | $16,962.83 (2026-09-11) | 0 |

![Post-credit requirement vs realized loss, breaches marked](backtest/backtest.png)

### H = 20 minutes (bar-level supplementary, not minute-level)

| sample | windows | breaches with CH | pre-A3 | baseline | worst margin of safety (with CH) | pre-A2 |
| --- | ---: | ---: | ---: | ---: | --- | ---: |
| all windows | 10,944 | **1** | 1 | 0 | **−$3,023.00** (2021-05-19 12:40 UTC) | 1 |
| 2021-05 | 2,232 | 1 | 1 | 0 | −$3,023.00 (2021-05-19 12:40 UTC) | 1 |
| 2022-05 | 2,232 | 0 | 0 | 0 | $12,628.62 | 0 |
| 2022-06 | 2,160 | 0 | 0 | 0 | $14,743.25 | 0 |
| 2022-11 | 2,160 | 0 | 0 | 0 | $10,678.51 | 0 |
| 2026-09 | 2,160 | 0 | 0 | 0 | $17,142.19 | 0 |

### The breach: 2021-05-19

| date | params in force (data cutoff) | SOL / ETH shock | R_p | credit | post-credit req | realized loss | loss (closes) | SOL | ETH | with CH | baseline |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| 2021-05-19 | calibrated (2021-05-18) | 27.21% / 19.17% | $13,685.36 | $6,314.64 | $13,685.36 | **$14,562.16** | $5,459.33 | −37.49% | −27.74% | **BREACH** | ok |

**Why.**

1. **The model in force had two weeks of history.** 2021-05 is the first
   month of data, so on 05-19 the parameters came from 1–17 May only. SOL fell
   37.5% from the open and ETH 27.7%, so the short ETH leg covered about 74%
   of the SOL loss. The model said the book could lose $13,685.36 (R_p). The
   worst intraday combination of SOL's low and ETH's high reached $14,562.16,
   6.4% more.
2. **The floor (A2) was binding and still missed.** Credit was exactly
   Σ IM − R_p, so the requirement equalled the model's own risk. The breach is
   the gap between the model and the market, not a calculation error. On
   closes the loss was $5,459.33, well covered: the breach comes from the
   intrabar path.
3. **At the credit's own horizon it also breached.** The 12:40 UTC window
   on 2021-05-19 lost $3,023.00 more than the requirement. This is the first
   breach at the 20-minute horizon in any run of this backtest. The B3 throttle
   removes it (below).
4. **A parameter-only fix**, for scale: a haircut of at least 33.67%
   (3,367 bps, 2,000 in force) would have removed the 1-day breach (binary
   search in `npm run backtest`). Not applied.

### B2: longer history (what changed)

The B2 months were fetched with the network access you approved (provenance
in the data README). Same command, before (2022-11 and 2026-09 only) and after
(all five months):

| | two months (60 days) | five months (152 days) |
| --- | --- | --- |
| breaches with CH, H = 1 day | 1 (2022-11-09, −$4,166.38) | 1 (2021-05-19, −$876.81) |
| 2022-11-09 | **BREACH** (params from a calm week, SOL shock 17.43%) | **covered**, margin $1,565.58 (params had seen May 2021 and May/June 2022) |
| breaches with CH, H = 20 min | 0 | **1** (2021-05-19 12:40, −$3,023.00) |
| mean freed (% of Σ IM, H = 1 day) | 5.24% | 4.51% |

**The 2022-11-09 breach goes away once the calibration has seen earlier
crashes; a new one appears where the calibration has not.** That is the
same failure, moved to the start of the data: risk parameters fitted to
a calm window before a crash. Seeding the calibration with crash history
before any credit is issued is the operational fix (B2 recommendation). It
does not help on the first day of a new kind of crash.

### Historical replay: 2–15 November 2022

| | value |
| --- | --- |
| Window | book opened 2022-11-02 00:00 UTC, held to 2022-11-15 24:00 UTC (14 days; **not** one margin horizon) |
| Params in force at open | calibrated (history includes 2021-05, 2022-05, 2022-06) |
| Σ IM / credit / post-credit requirement | $20,000.00 / **$0.00** / $20,000.00 |
| Worst loss over the window | $19,557.37 (closes: $18,414.13) |
| With CH / baseline | ok / ok |

With two months of data (before B2) the parameters at the open were the
initial ones. Credit was then $7,111.11, and this replay breached.

**The quoted figures, checked against the CSVs.** The reported moves for
2–15 Nov 2022 were SOL ≈ −54% and ETH ≈ −20%. The data gives:

| measurement | SOL | ETH |
| --- | ---: | ---: |
| daily CSV, close 11-01 → close 11-15 | −55.53% | −20.61% |
| daily CSV, close 11-02 → close 11-15 | −53.51% | −17.46% |
| 20-minute klines, open 11-02 00:00 → close 11-15 23:40 | −55.53% | −20.60% |

Measured over one consistent window (11-02 open → 11-15 close), the figures
are **SOL −55.5%, ETH −20.6%**.

### HYPOTHETICAL: decorrelation shock

**HYPOTHETICAL. These moves were chosen, not observed:** SOL −30% and ETH +15%
within one day, so the two hedge legs both lose.

| parameters | loss | Σ IM | credit | post-credit req | with CH | baseline |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| initial (localnet) | $22,500.00 | $20,000.00 | $7,111.11 | $12,888.89 | BREACH | BREACH |
| calibrated, in force 2026-09-30 | $22,500.00 | $20,000.00 | $0.00 | $20,000.00 | BREACH | BREACH |

Neither venue margin alone nor margin minus credit covers a decorrelation of
this size. With the initial parameters Clearinghouse makes the shortfall
larger by exactly the credit; with calibrated parameters it adds nothing.

## Where credit is meaningful: leverage sweep (B1)

`npm run leverage`: the same book at 2×, 3× and 5× leverage (IM = 1/L,
MM = IM/2), engine as on chain, 152 days and 10,944 windows.

| leverage | IM per leg | Σ IM | freed (% of Σ IM, H = 1 day) | days with credit | mean credit | mean extra notional | breaches H = 1 day (baseline) | breaches H = 20 min (baseline) | worst margin, H = 1 day |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2× | 50.00% | $50,000.00 | **33.93** | 152 of 152 | $16,963.35 | $33,926.70 | **0** (0) | **0** (0) | $14,199.61 (2022-11-09) |
| 3× | 33.34% | $33,340.00 | **27.55** | 152 of 152 | $9,184.00 | $27,546.48 | **0** (0) | **0** (0) | $4,680.58 (2022-11-09) |
| 5× | 20.00% | $20,000.00 | 4.51 | 20 of 152 | $901.80 | $4,509.00 | 1 (0) | 1 (0) | −$876.81 (2021-05-19) |

Freed by month: 2× 34.38% / 35.65% / 34.79% / 33.36% / 31.38%; 3× 28.64% /
21.84% / 27.30% / 29.51% / 30.59%; 5× 22.11% / 0.00% / 0.00% / 0.00% / 0.00%
(2021-05, 2022-05, 2022-06, 2022-11, 2026-09).

**Plainly: credit is meaningful for moderately leveraged hedged books (2×–3×
here).** They get about a third of their initial margin back as trading power
every day, crashes included, with no breach. At 5× the venues' own IM (20%)
is below the model's crash-calibrated risk, so the floor removes the credit
on almost every day once the calibration has seen a crash. The one day it
did not, it breached. "Extra notional" is credit ÷ the venue's IM rate: the
additional position the freed margin supports at the same leverage.

## Efficiency-vs-safety frontier (B1)

`npm run frontier`: 10 haircuts × 8 hedge-break values, the calibrated risk
parameters in force each day, 152 days and 10,944 windows. "Freed" is the
mean of credit / Σ IM. It is the same at both horizons because parameters
change once a day.

![Margin freed vs haircut per hedge_break; filled markers breach at H = 1 day](backtest/frontier.png)

| haircut | hedge_break | freed (% of Σ IM) | breaches H = 1 day | breaches H = 20 min | pre-A2: freed | pre-A2: breaches H = 1 day |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 500 | 0 – 5,000 | 5.24 | 1 | 1 | 40.29 | 3 |
| **2,000** (in force) | 0 – 5,000 | **4.51** | 1 | 1 | 33.93 | 3 |
| 3,000 | 0 – 5,000 | 3.97 | 1 | 1 | 29.69 | 2 |
| 5,000 | 0 – 5,000 | 2.83 | 0 | 1 | 21.20 | 1 |
| 6,000 | 0 – 5,000 | 2.27 | **0** | **0** | 16.96 | 1 |
| 7,000 | 0 – 5,000 | 1.70 | 0 | 0 | 12.72 | 1 |
| 500 | 7,000 | 3.56 | 0 | 1 | 28.49 | 2 |
| 2,000 | 7,000 | 3.06 | 0 | 1 | 23.99 | 1 |
| 5,000 | 7,000 | 1.97 | 0 | 0 | 14.99 | 1 |
| 500 | 8,000 | 2.27 | **0** | **0** | 18.99 | 1 |
| 2,000 | 8,000 | 1.93 | 0 | 0 | 15.99 | 1 |
| 7,000 | 8,000 | 0.79 | 0 | 0 | 6.00 | 0 |

All 80 settings are in `docs/backtest/frontier.json`. Hedge break 0–5,000
gives identical numbers on this book (the SOL gap scenario dominates).
Without the throttle, the most efficient setting with **no breach at either
horizon** frees **2.27%** (haircut 60%, or haircut 5% with hedge break 8,000).
The floor (A2) cuts mean freed margin from 33.93% to 4.51% at the setting in
force and removes 2 of the 3 one-day breaches the pre-A2 engine had.

## Regime throttle (B3) evaluation

`npm run throttle`. Each leg's move from its 1-hour EMA, `m = |price − ema| / ema`,
scales credit linearly from 1 (m ≤ start) to 0 (m ≥ full); the portfolio
takes the minimum over legs. Evaluated where the credit is computed (the
window's open). **Stand-ins:** the EMA is an exponential average of 20-minute
closes with a 1-hour time constant, not Pyth's `ema_price`; the confidence
part of the throttle is **not modelled** (no confidence data). Haircut 20%,
hedge break 5,000.

| start | full | freed (H = 1 day) | breaches H = 1 day | freed (H = 20 min) | breaches H = 20 min | days throttled | windows throttled |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| off | – | 4.51 | 1 | 4.51 | 1 | 0 | 0 |
| 100 | 300 | 3.86 | 1 | 3.82 | **0** | 35 | 2,696 |
| 50 | 300 | 3.56 | 1 | 3.47 | 0 | 66 | 5,275 |
| 50 | 200 | 3.21 | 1 | 3.01 | 0 | 66 | 5,275 |
| **50** | **150** | **2.87** | **0** | **2.63** | **0** | 66 | 5,275 |
| 25 | 125 | 2.18 | 0 | 1.98 | 0 | 102 | 7,858 |
| 10 | 110 | 1.76 | 0 | 1.56 | 0 | 136 | 10,039 |

**Result (decision rule: keep B3 only if it removes the breach or improves
the frontier):**
- The design default (100/300) removes the **20-minute** breach but not the
  1-day one.
- 50/150 removes **both** and frees **2.87%** of Σ IM, more than the 2.27% of
  the best breach-free haircut/hedge-break setting. **B3 improves the frontier,
  so it is kept**, with 50/150 as the default.
- **Honest caveats:** the thresholds were chosen after seeing these breaches,
  on one book and five months, so this is not out-of-sample evidence. The EMA
  is a stand-in for Pyth's. Confidence widening is not tested. The throttle
  costs credit on 66 of 152 days.

On the two-month data (before B2) the same sweep removed the 2022-11-09
breach only at 25/125 (3.92% freed vs 0.90% for the best breach-free
haircut setting there).

## Recommended defaults and why

**Haircut 2,000 bps, hedge_break 5,000 bps, throttle 50/150 bps.** In this
sample neither the haircut nor the hedge break changes the breach count
except at settings that remove most of the credit. The throttle reacts to
the regime that caused the breaches, so it removes them at a smaller cost.
All three are judgement on limited data, and the hedge-break value is
the more conservative of 5,000 and the calibrated 3,046.

## Limits of this backtest

- One book, one pair, five months, 152 daily evaluations. Daily observations
  overlap with the calibration windows; nothing here is a statistical test.
- Spot prices from one venue. No perpetual marks, funding, or cross-venue
  basis; basis parameters are not estimated.
- Losses combine the SOL low and the ETH high of the same 20-minute bar.
  That is conservative, because the two need not coincide (on 2021-05-19 the
  closes-only loss was $5,459.33 against $14,562.16).
- Liquidation, slippage and time-to-liquidate are not modelled. A "breach"
  means the requirement did not cover the mark-to-market loss, not that a
  venue lost money.
- The data licence is non-commercial (CC BY-NC-SA 4.0). Production
  parameters need commercially licensed data.
