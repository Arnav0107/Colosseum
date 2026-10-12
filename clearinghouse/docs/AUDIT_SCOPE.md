# Audit scope

For an external auditor. This is what to review, what each component is
trusted with, which invariants must hold, and how to build and test
everything from a clean checkout. Status of each component:
[`STATUS.md`](STATUS.md). Known gaps: [`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md).
Threats and mitigations, each mapped to a test: [`THREAT_MODEL.md`](THREAT_MODEL.md).

## 1. In scope

| Component | Path | Lines (approx.) | Why |
| --- | --- | ---: | --- |
| `ch_core` program | `programs/ch_core/src` | ~3,600 | accounts, governance, venue auth, oracle, credit, allowlists, report bounds |
| Credit engine | `crates/ch_math/src` (`portfolio.rs`, `risk.rs`, `psd.rs`, `view.rs`, `lib.rs`) | ~1,500 | every credit number on-chain is computed here |
| Venue rules | `crates/ch_client/src/lib.rs` | ~290 | what an integrating venue must apply (`usable_credit`, `max(IM − credit, MM)`) |

Build features: production is `--features pyth,venue-cpi`. The program does
not compile without exactly one oracle backend and one venue-auth mode.

## 2. Out of scope (but read for context)

| Component | Why out of scope |
| --- | --- |
| `programs/mock_perps_a`, `mock_perps_b` | test venues (no access control on `set_acceptance`, by design) |
| `programs/mock_perps_engine` (if present) | reference venue for tests/demo, not production |
| `programs/ch_fund` | stub: holds no funds, not called |
| `services/keeper`, `services/risk`, `testbench` | off-chain; hold no protocol role; cannot create credit |
| `mock-oracle`, `dev-venue-signer` features | localnet only; compile-time exclusive with production features |

## 3. Trust boundaries

| Actor | Trusted for | Bounded by (enforced on-chain) |
| --- | --- | --- |
| Governance keys (admin, risk_authority, venue_manager, guardian) | parameters within bounds; venue registration | hard bounds, per-change step limits, timelock for anything that can raise credit; guardian can only remove credit; production requires four distinct keys (A7) |
| Venue programs | their own users' position reports | PDA-only authority (`venue-cpi`); consent; A5 report bounds; credit applied by venue *v* depends only on venues *v* allowlists (A1) |
| Pyth | prices for governed feed ids | Full verification only; feed id per asset; freshness and confidence checks; evidence must postdate the baseline |
| Anyone | calling refresh / invalidate | refresh is deterministic; invalidation needs a reason |
| Program upgrade authority | everything | **not enforceable on-chain**; operational runbook in OPERATIONS §2b |

## 4. Invariants (each must hold after every instruction)

1. Credit kind is `TradingPowerOnly`; credit is never withdrawable.
2. Per leg: `credit ≤ IM − MM` and `credit ≤ IM · max_credit_bps_of_im`.
3. Per portfolio: `Σ credit ≤ max_credit_per_user` and `Σ credit ≤ (ΣIM − R_p)⁺` (A2).
4. Outstanding counters (global, per venue, per asset) equal the sum of
   counted credits of the current epoch, and never exceed their caps.
5. `usable_credit = 0` unless the portfolio is Active, in the current epoch,
   unexpired, and every venue in the leg's view is still allowlisted (A1).
6. A venue's credit never exceeds what the legs of venues it accepts justify,
   and never exceeds the joint computation over all legs (A1, F3).
7. If the correlation sub-matrix over the assets present is not positive
   definite, credit is 0 (A4).
8. Any input change (snapshot write/close, consent change) invalidates in the
   same instruction; any parameter/model/venue-lifecycle change bumps the epoch.
9. No program holds user funds.
10. Losing credit never changes a maintenance margin or liquidation threshold.

Where they are tested: invariants 1–5 after every step of the seeded
stateful test (`crates/ch_svm_tests/tests/stateful.rs`, A8); 6 in
`crates/ch_math/tests/view_props.rs` and `tests/allowlist.rs`; 7 in
`crates/ch_math/tests/non_psd.rs`; 8 in `tests/portfolio.rs`; 10 in
`crates/ch_client` tests.

## 5. Build and test (from `clearinghouse/`)

Pinned toolchain (CI): Rust 1.99.0, Agave 4.3.0, Anchor CLI 1.2.0, Node 20.19.0.

```bash
# everything CI runs, with one PASS/FAIL line per step
scripts/ci/verify-local.sh

# individually
cargo test --workspace --features ch_core/mock-oracle,ch_core/dev-venue-signer
cargo test -p ch_core --features pyth,venue-cpi
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features pyth,venue-cpi --sbf-out-dir target/deploy-prod
cargo test --manifest-path crates/ch_svm_tests/Cargo.toml        # LiteSVM, compiled programs
cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test stateful   # ~5 minutes
python -I scripts/ci/mutate.py                                     # mutation checks (~1 hour)
cargo test -p ch_math --test non_psd -- --nocapture                # 20,000-matrix fuzz
anchor test --validator legacy --skip-build --skip-deploy          # needs a validator
```

## 6. Supply chain

`cargo-audit audit --no-fetch` on both lockfiles (cargo-audit 0.22.2,
advisory DB dated 2026-10-09, **not refreshed**: network access was approved
for market data only): 0 vulnerabilities. Warnings: 1 unmaintained
(`bincode`, RUSTSEC-2025-0141) in the program lockfile and 5 in the LiteSVM
test lockfile. The npm production audit gate (`scripts/ci/check-npm-audit.cjs`)
needs the npm registry and was **not run** here; CI runs it. Re-run both with
network before an audit starts.

## 7. Known limitations an auditor should not re-report

[`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) lists them with their
status. The most important:
- positions are venue-attested: A1 limits who relies on whom and A5 bounds
  report size and growth, but nothing proves a position exists;
- parameters are judgement, and the backtest found one breach at the 1-day
  proxy horizon ([`BACKTEST.md`](BACKTEST.md));
- books with many distinct allowlist views need a ComputeBudget instruction
  (up to ~740k CU, [`PERFORMANCE.md`](PERFORMANCE.md));
- no loss coverage (`ch_fund` is a stub), and no external audit so far;
- upgrade-authority custody is operational, not enforced.
