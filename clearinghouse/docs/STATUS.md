# Status

**Canonical.** Other documents link here instead of restating status. Last
updated 2026-10-11 on branch `feat/clearinghouse-hardening`.

**Verdict:** a hardened research prototype. It runs on localnet. It is **not**
ready for mainnet or for real funds: there is no external audit, no
deployment, no loss coverage, and venues must trust each other's reports
([KNOWN_LIMITATIONS](KNOWN_LIMITATIONS.md)). The review findings and their
closure are in [`UPGRADE_REPORT.md`](UPGRADE_REPORT.md).

## Components

| Component | State | Evidence |
| --- | --- | --- |
| `ch_core` portfolio credit (joint, stressed model, TradingPowerOnly, IM−MM caps, budgets) | **Implemented** | 66 LiteSVM tests on the compiled program; engine property, stress and parity suites |
| Atomic invalidation, risk epoch, exactly-once counters | **Implemented** | LiteSVM invariant tests, 4 mutation checks |
| Governance: roles, bounds, step limits, timelock, guardian | **Implemented** | `governance.rs` unit tests, `tests/governance.rs` |
| Venue authentication by CPI (`venue-cpi`) | **Implemented** | `tests/venue_cpi.rs` with the two mock venue programs |
| Pyth `PriceUpdateV2` oracle (`pyth`) | **Implemented, not cluster-tested** | parser tested against real mainnet bytes; `tests/pyth_oracle.rs` on the compiled `pyth` build; never run against the live receiver |
| Mock oracle (`mock-oracle`) | **Mock**, localnet/tests only | compile-time exclusive with `pyth` |
| Dev venue signer (`dev-venue-signer`) | **Mock**, localnet/tests only | compile-time exclusive with `venue-cpi` |
| Account closure (snapshots, consent + portfolio) | **Implemented** | `tests/account_lifecycle.rs` |
| `ch_client` venue rules | **Implemented** | unit tests |
| `mock_perps_a/b` | **Mock** venues (CPI reporting only) | used by `venue_cpi.rs` |
| `ch_fund` default fund | **Stub by decision**, no loss coverage | [FUND_DESIGN](FUND_DESIGN.md) |
| Keeper (`services/keeper`) | **Implemented (minimal)** | 8 tests, incl. two-instance convergence; not run against a validator |
| Testbench | **Implemented** (localnet tool) | 46 tests; 390 px layout measured; smoke wired to CI, not yet executed |
| `services/risk`, `dashboard/` | **Not built** (placeholders) | — |

## Verification state

| Check | State |
| --- | --- |
| Rust workspace tests (62), LiteSVM (66), ch_core Pyth unit tests (13) | pass locally |
| clippy `-D warnings` (4 feature sets), fmt, must-fail feature combos | pass locally |
| Testbench vitest (46), tsc, build; keeper tests (8) | pass locally |
| IDL + TS type match the build; program ids consistent | pass locally |
| npm production advisories | 6 accepted after reachability review (register + review date); 0 unreviewed |
| `cargo audit` | see UPGRADE_REPORT (Stage F) |
| `anchor test` (TS integration) and testbench smoke on a validator | **not executed**: no validator possible on the development host; wired into CI |
| GitHub Actions on this branch | **not run yet** (no push at the time of writing) |
| External audit | **none** |

## Documents

| Topic | Document |
| --- | --- |
| Specification (accounts, instructions, math) | [`../SPEC.md`](../SPEC.md) |
| Architecture | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Risk model | [`RISK_MODEL.md`](RISK_MODEL.md) |
| Threat model (trust boundaries) | [`THREAT_MODEL.md`](THREAT_MODEL.md) |
| Known limitations | [`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) |
| Venue integration | [`VENUE_INTEGRATION.md`](VENUE_INTEGRATION.md) |
| Operations | [`OPERATIONS.md`](OPERATIONS.md) |
| Testing and CI | [`TESTING_AND_CI.md`](TESTING_AND_CI.md) |
| Performance (measured) | [`PERFORMANCE.md`](PERFORMANCE.md) |
| Default fund decision | [`FUND_DESIGN.md`](FUND_DESIGN.md) |
| Breaking changes from v0 | [`MIGRATION.md`](MIGRATION.md) |
| Review findings and closure | [`UPGRADE_REPORT.md`](UPGRADE_REPORT.md) |
| Working log | [`UPGRADE_LOG.md`](UPGRADE_LOG.md) |
