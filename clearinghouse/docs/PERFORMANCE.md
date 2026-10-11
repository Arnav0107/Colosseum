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

| Instruction | CU | Notes |
| --- | ---: | --- |
| `initialize` | 29,031 | creates config, risk model, pending-change accounts |
| `register_venue` | 22,193 | |
| `set_asset_risk` | 16,593 | applies at once on localnet (timelock 0) |
| `update_correlations` (1 entry) | 16,842 | |
| `update_correlations` (100 entries) | 40,107 | largest batch measured; ~0.23k CU per entry |
| `set_scenario` (32 moves) | 19,076 | |
| `update_user_consent` (first call) | 29,050 | creates consent + portfolio |
| `submit_position_snapshot` (create) | 26,287 | registers the leg, invalidates the portfolio |
| `submit_position_snapshot` (update) | 23,932 | |
| `refresh_portfolio`, 1 leg | 38,345 | |
| `refresh_portfolio`, 2 legs | 49,158 | |
| `refresh_portfolio`, 4 legs | 74,898 | |
| `refresh_portfolio`, 8 legs (`MAX_LEGS`) | **147,668** | 74% of the default limit; 4 venues × 2 assets, mixed directions, so same-asset basis scenarios are exercised |
| `invalidate_portfolio` (expired, 8 legs) | 24,932 | no oracle accounts |
| `invalidate_portfolio` (price evidence, 8 oracles) | 41,602 | |
| `close_position_snapshot` | 17,386 | |

Refresh cost grows faster than linearly because the parametric risk is an
n×n covariance sum and the scenario set grows with n (two market-wide moves,
two gap moves per asset, two basis moves per shared-asset leg, plus up to 8
governance scenarios). At 8 legs the headroom to 200k is about 52k CU. Raising
`MAX_LEGS` would need a ComputeBudget instruction and a re-measurement.

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
| GlobalConfig | 1,051 | 5,989,320 | 0.005989 | admin, once | no |
| RiskModel (zero-copy) | 3,760 | 19,751,040 | 0.019751 | admin, once | no |
| PendingChange | 894 | 5,191,760 | 0.005192 | admin, once | no |
| VenueRegistration | 115 | 1,234,440 | 0.001234 | payer chosen by the venue manager | no (venues are never closed, by design: F8) |
| UserConsent | 50 | 904,240 | 0.000904 | user | yes, `close_user_accounts` |
| Portfolio | 738 | 4,399,280 | 0.004399 | user | yes, `close_user_accounts` |
| PosSnapshot | 156 | 1,442,720 | 0.001443 | the snapshot's `payer` (venue or its relayer) | yes, to `rent_payer` on close |
| MockPrice | 34 | 822,960 | 0.000823 | admin | localnet only |

Per user with 8 legs: 0.0053 SOL (consent + portfolio) + 8 × 0.00144 SOL
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
