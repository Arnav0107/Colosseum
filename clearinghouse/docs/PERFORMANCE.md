# Performance and cost

All numbers are **measured** on the compiled SBF program in LiteSVM 0.18 by
`crates/ch_svm_tests/tests/perf.rs`. Reproduce with:

```bash
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,dev-venue-signer
cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test perf -- --nocapture
```

The test asserts the two budgets that matter:
- every instruction stays within the default 200,000 CU per instruction, so no
  ComputeBudget instruction is needed;
- a refresh at `MAX_LEGS` fits one 1,232-byte transaction.

The figures come from the localnet build (`mock-oracle`); the production
oracle path parses a 134-byte Pyth account instead and was not measured
separately. Measured 2026-10-11 at commit `f9eaf02` plus the perf test.

## Compute units

Measured 2026-10-12 after A1 (allowlists), A2 (floor), A3 (hedge break
on-chain), A4 (PSD check) and A5 (report bounds), by
`cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test perf -- --nocapture`.
Figures from commit `f9eaf02` (before this work) are in parentheses.

**Run-to-run variation.** Instructions that create accounts for a freshly
generated user key (`update_user_consent`, `submit_position_snapshot`
create) vary between runs by up to ~9k CU, because the PDA bump search
depends on the key (two runs of the same code measured 29,519 and 20,961 for
`update_user_consent`). The other figures were stable across runs to within
~1k CU.

| Instruction | CU | Notes |
| --- | ---: | --- |
| `initialize` | 33,889 (29,031) | creates config, risk model, pending-change accounts |
| `register_venue` | 26,712 (22,193) | |
| `set_asset_risk` | 21,135 (16,593) | applies at once on localnet (timelock 0) |
| `update_correlations` (1 entry) | 21,382 (16,842) | |
| `update_correlations` (100 entries) | 44,746 (40,107) | largest batch measured; ~0.23k CU per entry |
| `set_scenario` (32 moves) | 23,616 (19,076) | |
| `update_user_consent` (first call) | 20,961 (29,050) | creates consent + portfolio; varies by key (above) |
| `submit_position_snapshot` (create) | 30,674 (26,287) | A5 bounds checked; no mark price; varies by key |
| `submit_position_snapshot` (update) | 28,324 (23,932) | |
| `refresh_portfolio`, 1 leg | 44,654 (38,345) | |
| `refresh_portfolio`, 2 legs | 57,693 (49,158) | |
| `refresh_portfolio`, 4 legs | 84,160 (74,898) | |
| `refresh_portfolio`, 8 legs (`MAX_LEGS`) | **158,358** (147,668) | 79% of the default limit; 4 venues × 2 assets, all venues allowlisted |
| `refresh_portfolio`, 8 legs in **8 distinct assets** | **188,659** (new) | A4 worst case: full 8×8 PSD factorization; 94% of the default limit |
| `refresh_portfolio`, 8 legs on 8 venues, all allowlisted | 159,690 (new) | A1: every view is the full set, joint run only |
| `refresh_portfolio`, 8 legs on 8 venues, **8 distinct 7-leg views** | **740,307** (new) | A1 worst case: joint + 8 view runs (~72.6k each). **Needs a `SetComputeUnitLimit` instruction** (≤ 1,400,000); the keeper adds one (`services/keeper/src/budget.ts`) |
| `invalidate_portfolio` (expired, 8 legs) | 30,439 (24,932) | no oracle accounts |
| `invalidate_portfolio` (price evidence, 8 oracles) | 47,110 (41,602) | |
| `close_position_snapshot` | 18,323 (17,386) | |

Most instructions rose by 4–5k CU without any change to their own logic.
The likely cause is `GlobalConfig` growing from 1,051 to 1,599 bytes (A1
allowlists, A3/A5 parameters), which every instruction deserializes. This is
an inference; it was not profiled.

**A1 view cost.** A refresh costs one engine run for the joint book plus one
per distinct partial view with two or more legs. Books where every venue
allowlists every other (or nobody allowlists anybody) cost no extra run.
`perf.rs` asserts the 1.4M ceiling for the worst case and the 200k default for
the others above.

**A4 PSD check cost.** The first version (one i128 division per term)
measured 284,137 CU for the 8-distinct-asset refresh, against 143,671 with
the check bypassed (a local, uncommitted measurement build). That put the
check at about 140k CU and over budget. The committed version (Crout form,
entries `ρ_ppm·2^30`, one reciprocal per pivot, shifts elsewhere: 8 i128
divisions at n = 8) measures 182,368, so the check costs about 39k CU at
8×8 (A4 measurements were taken before A1 grew `GlobalConfig`). `perf.rs`
asserts the 8-distinct-asset refresh stays within 200,000 CU.

Refresh cost grows faster than linearly because the parametric risk is an
n×n covariance sum and the scenario set grows with n (two market-wide moves,
two gap moves per asset, two basis moves per shared-asset leg, the hedge
break, plus up to 8 governance scenarios). At 8 legs and 8 assets the
headroom to 200k is about 11.3k CU. Raising `MAX_LEGS` would need a
ComputeBudget instruction and a re-measurement.

## Transaction size at `MAX_LEGS`

`refresh_portfolio` with 8 legs takes 20 accounts (4 named + 8 snapshots + 8
oracles) and one signature (the fee payer; refresh needs no other signer).

| | Bytes |
| --- | ---: |
| Serialized transaction | **645** |
| Limit (`PACKET_DATA_SIZE`) | 1,232 |

No address lookup table is needed. With Pyth, the 8 oracle accounts are
`PriceUpdateV2` accounts; the size is the same (32 bytes per account key).

## Account sizes and rent

Rent at the default rate (LiteSVM's `minimum_balance_for_rent_exemption`).

| Account | Bytes | Rent-exempt lamports | SOL | Who pays | Recoverable |
| --- | ---: | ---: | ---: | --- | --- |
| GlobalConfig | 1,599 | 8,773,160 | 0.008773 | admin, once | no |
| RiskModel (zero-copy) | 3,760 | 19,751,040 | 0.019751 | admin, once | no |
| PendingChange | 894 | 5,191,760 | 0.005192 | admin, once | no |
| VenueRegistration | 115 | 1,234,440 | 0.001234 | payer chosen by the venue manager | no (venues are never closed, by design: F8) |
| UserConsent | 50 | 904,240 | 0.000904 | user | yes, `close_user_accounts` |
| Portfolio | 867 | 5,054,600 | 0.005055 | user | yes, `close_user_accounts` |
| PosSnapshot | 180 | 1,564,640 | 0.001565 | the snapshot's `payer` (venue or its relayer) | yes, to `rent_payer` on close |
| MockPrice | 34 | 822,960 | 0.000823 | admin | localnet only |

Per user with 8 legs: 0.0060 SOL (consent + portfolio) + 8 × 0.00156 SOL
(snapshots, paid by venues) ≈ 0.0169 SOL locked, all of it refundable.

## Program size

| Build | `.so` bytes |
| --- | ---: |
| `mock-oracle,dev-venue-signer` (localnet/tests) | 435,376 |
| `mock-oracle,venue-cpi` (CPI suite) | 436,440 |
| `pyth,dev-venue-signer` (Pyth suite) | 422,288 |
| `pyth,venue-cpi` (production configuration) | 423,352 |

## Keeper cost (estimate from the measurements, not a measured bill)

A keeper refreshes each active portfolio once per `credit_ttl_slots −
KEEPER_MARGIN_SLOTS` = 3,000 − 300 = 2,700 slots ≈ 18 minutes at 400 ms per slot,
so about **80 refreshes per user per day**.

| Item | Per refresh | Per user per day |
| --- | ---: | ---: |
| Base fee (1 signature × 5,000 lamports) | 5,000 lamports | 400,000 lamports = 0.0004 SOL |
| Compute (8 legs, no ComputeBudget instruction) | 147,668 CU | 11.8M CU |
| Priority fee | 0 by default; at *p* micro-lamports per CU, ≈ 148k × *p* / 10⁶ lamports | ≈ 80 × that |

At 1,000 active users, that is about 0.4 SOL per day in base fees plus any
priority fees. Failed attempts on stale inputs are bounded by the keeper's
per-portfolio backoff (`services/keeper`). A second keeper instance at most
doubles the fees; it cannot double-count credit (see the keeper README).

## Price-guard invalidation load (`max_move_bps` = 300)

The default `max_move_bps` in scripts and fixtures is **300** (3%). The hard
bound stays 1..=2,000. A tighter guard makes credit drop sooner after a
move, at the cost of more invalidations and refreshes. Measured on recorded data by
`cargo test -p ch_math --test move_guard_load -- --nocapture`. The data is
Binance spot 1-minute klines aggregated to 20-minute windows (one credit
lifetime); sources and SHA-256 are in `crates/ch_math/tests/data/klines_20m/SOURCES.txt`,
produced by `scripts/data/fetch_klines_20m.py`.

Share of 20-minute windows in which the price moved more than the guard
from the window open:

| Month | Series | > 2% | > 3% | > 5% | > 10% |
| --- | --- | ---: | ---: | ---: | ---: |
| 2022-11 (FTX collapse) | SOL | 13.33% | 7.36% | 3.19% | 0.46% |
| 2022-11 | ETH | 3.38% | 1.20% | 0.37% | 0.00% |
| 2022-11 | SOL/ETH pair (either leg) | 13.70% | **7.45%** | 3.19% | 0.46% |
| 2026-09 | SOL | 0.42% | 0.19% | 0.00% | 0.00% |
| 2026-09 | ETH | 0.32% | 0.09% | 0.05% | 0.00% |
| 2026-09 | SOL/ETH pair (either leg) | 0.51% | **0.19%** | 0.05% | 0.00% |

Load for one active two-leg portfolio:
- 72 windows per day;
- each breach adds one `invalidate_portfolio` and one early refresh on top of
  the keeper's ~80 TTL refreshes per day;
- 5,000 lamports per transaction.

| Month | `max_move_bps` | breaches / day | extra tx / day | total tx / day | base fees / day |
| --- | ---: | ---: | ---: | ---: | ---: |
| 2022-11 | 300 | 5.37 | 10.73 | 90.7 | 453,667 lamports |
| 2022-11 | 500 | 2.30 | 4.60 | 84.6 | 423,000 lamports |
| 2026-09 | 300 | 0.13 | 0.27 | 80.3 | 401,333 lamports |
| 2026-09 | 500 | 0.03 | 0.07 | 80.1 | 400,333 lamports |

Lowering the guard from 5% to 3% adds about 7% to a portfolio's daily
transactions in a crash month (90.7 vs 84.6), and almost nothing in a calm
month. In a crash month about 7.5% of credit lifetimes end early.

Caveats:
- exchange spot highs and lows, not the on-chain oracle;
- refreshes are not aligned to clock windows in practice;
- a breach only *permits* invalidation, because someone must submit the
  evidence;
- two months and one asset pair only.

