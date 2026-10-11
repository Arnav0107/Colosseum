# Clearinghouse

Cross-venue portfolio margining on Solana. Status, verification state and
readiness: **[`docs/STATUS.md`](docs/STATUS.md)** (canonical; this file does not
restate it).

## In one paragraph

Venue programs report each user's positions to `ch_core` by CPI. Anyone can call
`refresh_portfolio`, which computes one joint risk number over every leg on
every consented venue (stressed parametric risk and scenario losses, see
[`docs/RISK_MODEL.md`](docs/RISK_MODEL.md)) and derives per-leg credit. A venue
applies `max(IM − credit, MM)` to risk-increasing actions only. Credit is never
collateral, never withdrawable, and losing it never by itself liquidates. Any
input change invalidates it in the same instruction; any parameter or model
change voids all credit through the risk epoch.

## Layout

```
programs/ch_core        the protocol (Anchor 1.2.0)
programs/mock_perps_a/b MOCK venues: report by CPI, nothing else
programs/ch_fund        STUB (no loss coverage, by decision: docs/FUND_DESIGN.md)
crates/ch_math          pure integer engine (joint risk, stressed model, caps)
crates/ch_client        venue-side rules (usable_credit, effective_initial_margin)
crates/ch_svm_tests     instruction-level tests on the compiled program (LiteSVM)
tests/                  Anchor TS integration tests
testbench/              localnet UI, independent verification, smoke test
services/keeper         minimal, role-less liveness keeper
services/risk, dashboard/  placeholders, not built
docs/                   STATUS, ARCHITECTURE, RISK_MODEL, THREAT_MODEL, ...
SPEC.md                 accounts, instructions, math (reconciled with the code)
```

## Build features (mandatory)

`ch_core` refuses to compile unless you pick exactly one oracle and exactly one
venue-authentication mode:

| Purpose | Features |
| --- | --- |
| localnet, tests, testbench | `mock-oracle,dev-venue-signer` |
| production | `pyth,venue-cpi` |

```bash
cargo test --workspace --features ch_core/mock-oracle,ch_core/dev-venue-signer
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,dev-venue-signer
cargo test --manifest-path crates/ch_svm_tests/Cargo.toml
```

All commands and CI: [`docs/TESTING_AND_CI.md`](docs/TESTING_AND_CI.md).
Localnet: `./scripts/dev-localnet.sh`, then [`testbench/README.md`](testbench/README.md).

## Trust assumptions (short form; full: [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md))

1. Venue programs report their users' positions honestly, and venues in one
   user's portfolio rely on each other's reports (bounded by caps, not prevented).
2. Governance keys act within on-chain bounds; credit-increasing changes wait
   for a timelock (≥ 216,000 slots in production).
3. Pyth prices (Full verification) for the governed feed ids.
4. Nobody has to be trusted to compute credit: refresh and invalidation are
   permissionless and deterministic.
