# Known limitations

What this branch does **not** solve, ordered by risk. Each item says what it
would take to fix. Status overview: [`STATUS.md`](STATUS.md).

## Design and economics

1. **Venues rely on each other's reports.** Credit is computed jointly, so a
   misreporting venue can create credit that other venues apply for the same
   user. Damage is bounded by `IM − MM` per leg and by the per-venue,
   per-asset and global caps, but not prevented. *Fix:* a per-venue allowlist
   of venues whose reports it accepts credit from (or credit split by
   reporting set), plus venue attestation or proof of positions.
2. **No loss coverage.** `ch_fund` is a stub by decision
   ([`FUND_DESIGN.md`](FUND_DESIGN.md)). Credit is trading power only, so
   no loss is socialized. But a venue that applies credit carries the risk
   that the hedge on another venue disappears faster than it can raise the
   margin back. *Fix:* a funded, capped default fund with a defined waterfall,
   or venue-to-venue settlement. Neither exists.
3. **Model parameters are judgement.** Shocks, gaps, basis, δ, β and scenarios
   are set by the risk authority within bounds. Nothing estimates them from
   data on chain. There is no liquidity, slippage or time-to-liquidate model
   and no funding P&L ([`RISK_MODEL.md`](RISK_MODEL.md) §6).
4. **Bounded portfolio size.** `MAX_LEGS = 8` per user and `MAX_ASSETS = 32`.
   A refresh at 8 legs costs 147,668 CU ([`PERFORMANCE.md`](PERFORMANCE.md)).
   More legs need a ComputeBudget instruction and re-measurement.
5. **The refresher chooses the baseline price.** Any genuine update within
   `max_price_age_secs` can be used at refresh. This affects the price guard
   only, not the credit amount.
6. **Credit expires quickly.** `credit_ttl_slots` ≤ 3,000 (~20 min), so credit
   needs a live refresher. Liveness depends on someone (keeper or user)
   paying for refreshes; failure is safe (credit stops being usable).

## Implementation

7. **No external audit.** All review was internal (this branch's review
   F1–F14 and the follow-up findings in `UPGRADE_LOG.md`).
8. **Pyth path verified against recorded bytes only.** The parser is tested
   against a real mainnet `PriceUpdateV2` account (fixture) and on the
   compiled `pyth` build in LiteSVM with injected accounts. It has not run
   against the live receiver program on any cluster (no devnet actions were
   taken).
9. **Portfolio `version` restarts after close and re-create.** Venues must read
   credit live (`usable_credit`) and must not cache `(version, credit)` across
   a close ([`VENUE_INTEGRATION.md`](VENUE_INTEGRATION.md)).
10. **Counters lag snapshot closes.** Closing a snapshot invalidates the
    portfolio but releases its outstanding-credit share only on the next
    refresh, `invalidate_portfolio`, `close_user_accounts` or epoch bump. The
    keeper releases it. Until then the caps are slightly conservative
    (over-counted), never under-counted.
11. **Mock venues are not exchanges.** `mock_perps_a/b` only prove the CPI
    authentication path. There is no reference venue that enforces
    `max(IM − credit, MM)` in a real margin engine.
12. **Keeper adapter not run against a live validator here.** Its decisions are
    tested against a fake chain that models the program's rules. Its
    instructions and account lists are the ones the testbench smoke uses
    (CI job, not yet executed: see #17).
13. **Upgrade authority and key custody are out of scope.** No deployment
    exists. OPERATIONS describes the required setup (multisig upgrade
    authority, separate role keys).

## Tooling and supply chain

14. **Accepted npm advisories.** Six transitive production advisories are
    accepted after a reachability review: `toml` via `@coral-xyz/anchor`
    0.30.1, and `stream-json` and `uuid` via `jayson` 4 via `@solana/web3.js` 1.x.
    The register is `scripts/ci/npm-audit-accepted.json`. CI fails on any new
    advisory and after the review date. *Fix:* move the TypeScript side to
    `@solana/kit` and a newer Anchor client, then delete the register entries.
15. **TypeScript client versions.** The testbench and keeper use
    `@coral-xyz/anchor` 0.30.1 against an Anchor 1.2.0 program. The IDL format
    is compatible (checked by tsc against the generated type), but the client
    is not the version the program was built with.
16. **Testbench bundle.** 1.4 MB (node polyfills; Vite warns about chunk size
    and `eval` in `vm-browserify`). Acceptable for a localnet tool; not
    suppressed.
17. **Local environment.** On the Windows development host,
    `solana-test-validator` cannot run (no symlink privilege). So
    `tests/ch_core.ts` and the testbench smoke have **not been executed**.
    They are wired into CI, which has not yet run on this branch (it runs on
    push). On-chain behaviour was verified locally in LiteSVM against the
    compiled programs.

## Model inputs

18. **Fixed (A4): correlation matrix not checked for PSD on-chain.** Every
    refresh now checks, with integer arithmetic only, that the correlation
    sub-matrix over the assets present is positive definite. If it is not,
    the portfolio gets 0 credit (`R_p = max(S, scenarios)`) and the
    `PSD_FALLBACK` flag. In the 20,000-sample fuzz all 10,771 non-PSD
    sub-matrices fall back and none earns credit (`non_psd.rs`; LiteSVM
    `non_psd_correlation_gives_zero_credit_and_sets_flag`). What is left: a
    matrix can be PSD and still wrong (judgement, #3); singular matrices
    (ρ = ±1 between different assets) are rejected too, which costs credit
    but is safe.
19. **Fixed (A2): credit could push held margin below the worst scenario loss
    when venue IM is below the model's standalone risk.** Credit is now capped
    at `(ΣIM − R_p)⁺` in `ch_math` (and so on-chain) and in the TS port.
    Tests: `prop_floor_post_credit_requirement_never_below_portfolio_risk`,
    `floor_closes_the_gap_when_im_is_below_standalone_risk`, two parity
    vectors, and `services/risk` "floor (A2): scaling every shock…". Cost:
    on the backtest data mean freed margin fell from 27.38% to 5.24% of ΣIM,
    and the one breach remains, because it was a model miss
    ([`BACKTEST.md`](BACKTEST.md)). What is left is that a venue's own IM can
    be below the worst loss; that is the venue's requirement, not credit.
20. **Calibration rests on two months of one-venue spot data, and the
    backtest found a breach.** At H = 1 day (a conservative proxy for the
    ~20-minute credit), 1 of 60 days breached with Clearinghouse and 0 without:
    on 2022-11-09, parameters calibrated from a quiet week met a 42% SOL drop
    ([`BACKTEST.md`](BACKTEST.md)). Step limits (shock ≤ 2× per update) and the
    ~1-day timelock slow the reaction to a crash. Basis parameters cannot be
    estimated from this data. The data licence is not recorded
    ([`crates/ch_math/tests/data/README.md`](../crates/ch_math/tests/data/README.md)).
    *Fix:* longer, multi-venue history (needs approved network access), and
    the floor in #19 (applied; it does not remove this breach).
