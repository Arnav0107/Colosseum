# Risk model

Status of every component: [`STATUS.md`](STATUS.md). Code:
[`crates/ch_math/src/risk.rs`](../crates/ch_math/src/risk.rs) (model),
[`crates/ch_math/src/portfolio.rs`](../crates/ch_math/src/portfolio.rs) (credit engine),
[`programs/ch_core/src/instructions/portfolio.rs`](../programs/ch_core/src/instructions/portfolio.rs) (on-chain use),
[`testbench/src/portfolio.ts`](../testbench/src/portfolio.ts) (TypeScript port, bit-for-bit, enforced by shared vectors).

## 1. What the number means

Clearinghouse computes **initial-margin relief**: by how much a venue that
has integrated the rules may lower its *initial*-margin requirement for a
user's risk-increasing actions, because the user's other consented positions
offset that risk.

It is **not**:

- collateral, a guarantee, or a promise that anyone pays if the user defaults;
- a transfer of gains from one venue to cover losses on another;
- withdrawable equity;
- a change to any venue's maintenance margin or liquidation threshold.

The four capabilities this distinguishes are: (a) calculated margin relief,
(b) venue-recognized trading-power credit, (c) enforceable collateral or
guarantee coverage, and (d) cross-venue P&L settlement. Which of them exist:
[`STATUS.md`](STATUS.md#capabilities) (single source).

Because each leg's credit is capped at `IM − MM`, a venue's effective
requirement is `max(IM − credit, MM)`. Losing a credit therefore never by
itself makes a position liquidatable, and a credit can never prevent a
liquidation the venue's own maintenance-margin engine would perform.

## 2. Inputs

Per leg (one per user × venue × asset, reported by the venue):
notional `N_i` (micro-USD, > 0), side, initial margin `IM_i`, maintenance
margin `MM_i`, with `0 < MM_i ≤ IM_i ≤ N_i`.

Per asset (set by the risk authority, `RiskModel` account):

| Parameter | Meaning | Bounds |
| --- | --- | --- |
| `shock_bps` | one-horizon adverse move used for standalone risk; `0` = asset unsupported (refresh fails closed) | 100 – 10,000 |
| `gap_bps` | idiosyncratic gap: this asset moves alone | `shock_bps` – 10,000 |
| `basis_bps` | venue basis: one venue's mark moves alone | 10 – 10,000 |

Global: correlation matrix `ρ` (32 × 32, symmetric, `[-1, 1]`, scaled 1e6),
correlation stress `δ` (≥ 0.05), same-asset basis decorrelation `β` (≥ 0.01),
up to 8 governance scenarios (per-asset signed moves, each labelled
`HISTORICAL_REPLAY` or `HYPOTHETICAL`).

## 3. Formulas

All integer arithmetic; **risk rounds up, credit rounds down**.

**Standalone risk** of leg `i`: `R_i = ceil(N_i · shock_bps(asset_i) / 10,000)`.
Signed exposure `x_i = +R_i` (long) or `−R_i` (short). `S = Σ R_i`.

**Parametric risk** with direction-aware correlation stress:

```
V = Σ_i Σ_j x_i · x_j · ρ'_ij / 1e6        (ceil)        R_par = isqrt_ceil(V)

ρ'_ii = 1
ρ'_ij (different assets) = clamp(ρ_ij − δ)  if x_i·x_j < 0   (the legs offset: a hedge)
                         = clamp(ρ_ij + δ)  if x_i·x_j > 0   (same direction)
ρ'_ij (same asset)       = 1 − β            if the legs offset
                         = 1                if they point the same way
```

Each cross term's contribution can only grow under stress (it changes by
`+|x_i x_j| δ` before clamping), so **decorrelation hurts hedges and
correlation spikes hurt same-direction books**. This corrects the idea
document, which said "correlations rise in crashes, so the hedge weakens". For
a long/short pair a correlation *rise* strengthens the hedge; what weakens it
is decorrelation, and that is what `ρ − δ` models. Rising correlations hurt
books whose legs point the same way, and that is what `ρ + δ` and the
market-wide scenario model.

**Scenario risk** `R_scen` = the worst loss over these deterministic scenarios.
Each loss is computed exactly from notionals, `loss = ceil(max(0, −Σ side_i · N_i · move_i) / 10,000)`:

1. *Market-wide*: every asset moves by its own `shock_bps`, all in the same
   direction (down, then up). A book with no offsetting exposure loses the
   full `S` here, so it never earns credit from diversification alone. (The
   stress suite found that without this scenario an all-long SOL/ETH/BTC book
   earned ~$2,591 of credit.)
2. *Idiosyncratic gap*: each asset present moves alone by `±gap_bps`.
3. *Venue basis*: each leg whose asset is also held on another venue moves alone by `±basis_bps`.
4. *Governance scenarios*: up to 8 vectors.
5. *Hedge break* (`ch_math` only, **not used on-chain yet**; see §3a): every
   leg moves adversely at once by a fraction of its shock.

**Portfolio risk** `R_p = max(R_par, R_scen)`.

**Credit**:

```
raw   = floor( ΣIM · (S − R_p)⁺ / S )                 (hedge benefit applied to IM)
total = min( floor(raw · (10,000 − haircut_bps) / 10,000), max_credit_per_user )
share_i  = floor( total · IM_i / ΣIM )                 (pro rata; remainder dropped)
credit_i = min( share_i, IM_i − MM_i, IM_i · max_credit_bps_of_im / 10,000 )
```

then protocol budgets are applied on-chain, each only scaling credits down
(`floor(c · headroom / sum)` within the group): per venue, per asset, global,
using outstanding-credit counters in `GlobalConfig`.

Properties (tested): a single leg gets exactly 0; one-direction books get 0;
leg order never changes any result; every cap holds; the fraction of IM
still held is at least `min(1, R_p / S)`. `R_p` can exceed `S` (gaps above
shocks, large governance scenarios); credit is then 0. Held margin is at least
the worst scenario loss only when the venues' IM covers the standalone risk
(`ΣIM ≥ S`); see §3b and §6.6.

### Worked example

Long SOL $50,000 on venue A, short ETH $50,000 on venue B; IM 20% ($10,000),
MM 10% ($5,000); SOL shock = gap 10%, ETH 8%; ρ = 0.8, δ = 0.1; haircut 20%.

| Quantity | Value |
| --- | --- |
| `R_SOL`, `R_ETH`, `S` | $5,000, $4,000, $9,000 |
| `ρ'` (offsetting) | 0.7 |
| `R_par` | √(25 + 16 − 2·0.7·20)·10⁶ ≈ $3,605.55 |
| Market-wide (both −shock) | long loses $5,000, short gains $4,000 → $1,000 |
| SOL gap −10% alone | $5,000 ← worst |
| `R_p` | $5,000 |
| `raw` | floor(20,000 · 4,000 / 9,000) = $8,888.888888 |
| total after haircut | **$7,111.111110** |
| per leg | $3,555.555555 each (below the $5,000 `IM − MM` cap) |
| effective IM at each venue | $10,000 − $3,555.56 = $6,444.44 (≥ MM $5,000) |

For comparison the v0 model (margin as risk) freed $10,940.36, pushing each
venue's requirement to $4,529.82, below its own MM.

## 3a. Hedge-break scenario (M3), `ch_math` only

Every leg moves against its holder at the same time (longs down, shorts up)
by `ceil(shock_bps · hedge_break_bps / 10,000)`, and each leg's loss is rounded
up separately. Code: `HedgeBreakModel` and `StressedRiskModel::hedge_break_loss`
in [`crates/ch_math/src/risk.rs`](../crates/ch_math/src/risk.rs); TS port in
`testbench/src/portfolio.ts`; shared vectors include it.

**Why a fraction:** at `hedge_break_bps = 10,000` the loss equals `S` exactly,
so `f = 0` and **no portfolio ever gets credit**. The parameter is therefore a
fraction:

| Constant | Value | Meaning |
| --- | --- | --- |
| `HEDGE_BREAK_MAX_BPS` | 10,000 | full shock on every leg (kills all credit) |
| `HEDGE_BREAK_MIN_BPS` | 2,500 | governance floor once enabled |
| `HEDGE_BREAK_DEFAULT_BPS` | **5,000, provisional** | half the shock on every leg at once; to be calibrated from data |
| `0` | disabled | the current on-chain behaviour |

**On-chain status:** `ch_core` still builds `StressedRiskModel` directly, so
the scenario is **off on-chain**. Every on-chain number in this document and
in SPEC is unchanged. Enabling it needs a stored parameter and a governance
path in `ch_core`, which is behind the approval gate
([`WORKFLOW.md`](WORKFLOW.md) §4).

**Sensitivity**, from `cargo test -p ch_math --test hedge_break -- --nocapture`.
Current params: shocks SOL 10% / ETH 8%, ρ = 0.8, δ = 0.1, β = 0.02, haircut
20%, leg cap 75% of IM and `IM − MM`; legs $50k notional, IM $10k, MM $5k.
"Max extra size" is the extra notional the freed margin carries at the venue's
own 20% IM rate: Σ min(credit_i, IM_i − MM_i) · notional_i / IM_i. It is
first order, because the credit is recomputed once the new size is reported.

Worked example (long SOL venue 0 / short ETH venue 1):

| hedge_break_bps | R_p | f | total credit | max extra size |
| ---: | ---: | ---: | ---: | ---: |
| 0 | $5,000.00 | 44.44% | $7,111.11 | $35,555.55 |
| 2500 | $5,000.00 | 44.44% | $7,111.11 | $35,555.55 |
| **5000** (provisional default) | $5,000.00 | 44.44% | $7,111.11 | $35,555.55 |
| 5556 | $5,005.00 | 44.39% | $7,102.22 | $35,511.11 |
| 6000 | $5,400.00 | 40.00% | $6,400.00 | $32,000.00 |
| 7000 | $6,300.00 | 30.00% | $4,800.00 | $24,000.00 |
| 7500 | $6,750.00 | 25.00% | $4,000.00 | $20,000.00 |
| 9000 | $8,100.00 | 10.00% | $1,600.00 | $8,000.00 |
| 10000 | $9,000.00 | 0.00% | $0.00 | $0.00 |

Below about 55.6% the SOL gap scenario ($5,000) still dominates, so this pair is
unaffected. The scenario bites hardest on **same-asset basis trades**, which
the other scenarios barely touch (§6.8):

| hedge_break_bps | R_p | f | total credit | max extra size |
| ---: | ---: | ---: | ---: | ---: |
| 0 | $1,000.00 | 90.00% | $10,000.00 | $50,000.00 |
| 2500 | $2,500.00 | 75.00% | $10,000.00 | $50,000.00 |
| **5000** | $5,000.00 | 50.00% | $8,000.00 | $40,000.00 |
| 7500 | $7,500.00 | 25.00% | $4,000.00 | $20,000.00 |
| 10000 | $10,000.00 | 0.00% | $0.00 | $0.00 |

(Long SOL $50k on venue 0, short SOL $50k on venue 1.)

**Correlation-independent bound:** with the scenario enabled,
`R_p ≥ S · h / 10,000` whatever `ρ` says, so
`credit ≤ ΣIM · (1 − h/10,000) · (1 − haircut)`. This is tested with ρ = −1 in
`hedge_break_bounds_credit_for_any_correlation`. It is the backstop against a
bad correlation matrix (§3b).

## 3b. Correlation matrices are not checked for PSD on-chain (M2)

`ch_core` bounds every `ρ` to `[−1, 1]` and limits each change to ±0.25, but
it does **not** check that the matrix is positive semidefinite. That check
belongs in the off-chain risk service (not built yet).

What happens with a non-PSD matrix, measured by
`cargo test -p ch_math --test non_psd -- --nocapture` (20,000 fixed-seed
samples; one in five adversarial, with strongly negative ρ and same-direction
legs):

| Outcome | Samples |
| --- | ---: |
| matrices that are not PSD (f64 Cholesky, test-only) | 19,987 |
| … of which the engine **accepted** (variance came out positive) | 18,154 |
| … of which got credit | 9,465 |
| `NegativeVariance`, failed closed (no credit) | 1,833 |

So a non-PSD matrix **usually does not fail closed**. It produces a positive
but possibly understated parametric risk.

- **The per-term stress does not make this safe.** `ρ ± δ` makes each cross
  term at least as large as with the base matrix, for that book's direction.
  But if the base quadratic form is itself wrong (non-PSD), "at least the
  base" can still be below the true risk.
- **What keeps it bounded is the scenario floor.** `R_p = max(P, L)`, and
  every scenario loss `L` (market-wide, gaps, basis, governance, hedge break)
  is computed from notionals with no `ρ`. Credit is therefore never more
  than the scenarios allow, whatever the matrix says. With the hedge-break
  scenario enabled this becomes a hard, `ρ`-independent cap,
  `credit ≤ ΣIM · (1 − h) · (1 − haircut)` (§3a). It is not enabled on-chain
  today.
- **Every accepted sample** satisfied:
  - every cap;
  - leg-order invariance;
  - the held fraction ≥ `min(1, R_p/S)`;
  - when `ΣIM ≥ S` and credit > 0, held margin ≥ the worst scenario loss.
- **A `NegativeVariance` failure** reverts the refresh. Credit computed under
  the previous matrix is already void, because every correlation change
  bumps the risk epoch.

**Gap found by the same suite.** "Held margin ≥ worst scenario loss" is
**false** when the venues' IM is below the model's standalone risk
(`ΣIM < S`). Pinned counterexample
(`requirement_can_fall_below_worst_loss_when_im_is_below_standalone_risk`):
- ΣIM $10,000, S $40,000, R_p = worst loss $20,000;
- credit $4,750, so $5,250 is held.

That case is under-margined by the venues even with zero credit; credit makes
it worse. Closing it (for example capping credit at `ΣIM − R_p`) changes
on-chain credit, so it is behind the approval gate.

## 4. Scenarios: historical replay vs hypothetical

Every scenario is labelled. **Historical replays** are measured from a
recorded price series; the series and its source live in the repository.

**November 2022 (FTX collapse)**: Binance spot daily klines, retrieved
2026-10-11, stored in
[`crates/ch_math/tests/data/binance_nov2022_daily.csv`](../crates/ch_math/tests/data/binance_nov2022_daily.csv).
The scenario constants are *derived* from that file in a test, not typed in:

| Window | SOL | ETH | Kind |
| --- | --- | --- | --- |
| close 2 Nov → close 15 Nov 2022 | −53.51% (−5,351 bps) | −17.46% (−1,747 bps) | multi-day cumulative |
| close 8 Nov → close 9 Nov 2022 (worst day) | −42.25% (−4,225 bps) | −17.38% (−1,739 bps) | single day |

Basis points are rounded *away from zero* so a replayed loss is never understated.

Press figures (Reuters via Inquirer, 15 Nov 2022: SOL −53.8%, ETH ≈ −20%;
Decrypt week-over-week SOL −41.2%, ETH −7.5%) are close to, but not the same
as, the primary series, and are not used. The cumulative window is **not a
single margin horizon**: it is what a hedge carried through the collapse
experienced if nobody re-margined. Credits live at most 3,000 slots
(~20 minutes) and are invalidated permissionlessly on a 3% price move (the
default `max_move_bps`, see [`PERFORMANCE.md`](PERFORMANCE.md) for its load) from
their baseline, so the realistic exposure window is far shorter.

Result for the worked-example hedge: realized loss over the cumulative window
was $18,020 on $20,000 of total IM. v0 held only $9,060. With either replay
in the scenario set, `R_p ≥ S` and the hedge earns **0** credit. That is
the honest conclusion: a SOL/ETH pair was not a hedge in November 2022.

Hypothetical stresses in the test suite: favourable/adverse correlation
shifts, sudden decorrelation (to 0 at full decorrelation), a volatility spike
on one leg, growing price gaps, the hedge leg liquidated first, same-direction
legs across 8 venues, same-asset cross-venue legs, extreme values ($1B legs)
and overflow (fails closed). See
[`crates/ch_math/tests/risk_stress.rs`](../crates/ch_math/tests/risk_stress.rs).

## 5. Governance of the model

- Every model change (`set_asset_risk`, `set_stress_params`, `set_scenario`,
  `update_correlations`) bumps the model version and the global `risk_epoch`,
  so every credit computed under the old model becomes unusable at once.
- Bounds above are enforced on-chain, together with per-change step limits
  (|Δρ| ≤ 0.25; a shock may at most halve or double). A change that can only
  reduce credit applies at once: a disabled asset, the same shock with a larger
  gap or basis, more δ or β, or a scenario added to an empty slot. Every other
  change waits `timelock_slots` (≥ 216,000 slots in production) and can be
  cancelled by the guardian. Correlation changes always wait. See
  [`SPEC.md`](../SPEC.md) §4 and [`THREAT_MODEL.md`](THREAT_MODEL.md).
- Assets default to unsupported; a leg in an unsupported asset makes refresh fail.

## 6. Assumptions and limitations

1. **Venues report honestly.** Notional, side, IM and MM come from the venue.
   A venue that lies can only hurt itself and venues relying on a
   portfolio it is part of; see THREAT_MODEL.
2. **Parameters are judgement.** Shocks, gaps, basis, `δ`, `β` and scenarios
   are set by the risk authority. Nothing estimates them on-chain.
3. **No liquidity, slippage or time-to-liquidate model.** A venue's own IM/MM
   should cover those; this model only decides how much IM relief a hedge
   justifies.
4. **Funding, borrow and fee P&L are not modelled.** Basis between venues is
   represented only by the basis gap and `β`.
5. **Finite scenario set.** A crash shape not covered by the parametric term,
   the market-wide move, single-asset gaps, venue basis gaps or the 8
   governance vectors is not covered.
6. **Credit scales with IM, risk with notional.** If a venue's IM is below the
   model's standalone risk, the held fraction `≥ min(1, R_p / S)` is a
   fraction of a small IM, and the held amount can fall below the worst
   scenario loss (§3b counterexample). The model does not top up a venue's
   own requirement.
7. **Oracle.** Baselines and freshness checks rely on the configured oracle:
   the mock on localnet; in production, Pyth `PriceUpdateV2`, Full verification
   only, with a governed feed id per asset. The price feeds the guard only. Credit
   is computed from notionals and shocks.
8. **Same-asset legs are capped by `IM − MM`, not by `β` alone**: a
   cross-venue basis trade can receive large relief if both venues' IM is far
   above MM.
