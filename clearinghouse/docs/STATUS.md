# Status

**Canonical.** Other documents link here instead of restating status. Last
updated 2026-10-11 on branch `feat/clearinghouse-hardening`.

**Verdict:** a hardened research prototype. It runs on localnet. It is **not**
ready for mainnet or for real funds: there is no external audit, no
deployment, no loss coverage, and venues must trust each other's reports
([KNOWN_LIMITATIONS](KNOWN_LIMITATIONS.md)). The review findings and their
closure are in [`UPGRADE_REPORT.md`](UPGRADE_REPORT.md).

## Components

This is the **only** status table in the repository. Other documents link
here instead of restating it.

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

## Capabilities

What credit is and is not. Explained in [`RISK_MODEL.md`](RISK_MODEL.md) §1;
claim by claim against the original idea in [`PITCH_VS_PROTOTYPE.md`](PITCH_VS_PROTOTYPE.md).

| Capability | State | Evidence |
| --- | --- | --- |
| (a) Calculated margin relief | **Implemented** | `crates/ch_math`, `refresh_portfolio` |
| (b) Venue-recognized trading-power credit (`TradingPowerOnly`) | **Implemented**, for venues that integrate `ch_client` | `crates/ch_client` tests; no real venue integrated |
| (c) Enforceable collateral / guarantee coverage | **Not built** | [`FUND_DESIGN.md`](FUND_DESIGN.md) |
| (d) Cross-venue P&L settlement (moving gains between venues) | **Not built** (out of scope by design) | — |
| Withdrawable credit; credit that changes liquidation thresholds | **Dropped** (by design) | `ch_client` tests `f1_cost2_credit_cannot_prevent_or_cause_liquidation`, `invalidation_never_changes_the_liquidation_threshold` |
| Default fund, loss waterfall, circuit breaker | **Roadmap** | `ch_fund` stub; the guardian pause only removes credit |
| Unwind coordinator, fee model, governance voting | **Roadmap** | no code |

## Verification state

| Check | State |
| --- | --- |
| Rust workspace tests (62), LiteSVM (66), ch_core Pyth unit tests (13) | pass locally |
| clippy `-D warnings` (4 feature sets), fmt, must-fail feature combos | pass locally |
| Testbench vitest (46), tsc, build; keeper tests (8) | pass locally |
| IDL + TS type match the build; program ids consistent | pass locally |
| npm production advisories | 6 accepted after reachability review (register + review date); 0 unreviewed |
| `cargo audit` (cargo-audit 0.22.2) | 0 vulnerabilities in both lockfiles; unmaintained warnings only (`bincode` via Solana crates; 4 more in test-only dependencies). Yanked-crate check not run locally (index update failed) |
| `anchor test` (TS integration) and testbench smoke on a validator | **not executed**: no validator possible on the development host; wired into CI |
| GitHub Actions on this branch | **not run yet** (no push at the time of writing) |
| External audit | **none** |

## Verification state (baseline)

Branch `feat/venue-proof-and-evidence` at `c88ed75` (start of branch), run on
2026-10-12 on the Windows development host. Rules: [`WORKFLOW.md`](WORKFLOW.md).
Every command runs from `clearinghouse/` after
`source "$LOCALAPPDATA/xwin-bin/sbf-env.sh"` (toolchain recipe:
`UPGRADE_LOG.md` §1b).

**Tools:**

| Tool | Version |
| --- | --- |
| rustc | 1.99.0 (b940084d7 2026-09-28) |
| cargo | 1.99.0 |
| solana-cli | 4.3.0 (Agave) |
| cargo-build-sbf | 4.4.0, platform-tools v1.57 |
| anchor-cli | 1.2.0 |
| node / npm / yarn | 25.6.1 / 11.9.0 / 1.22.22 (CI pins Node 20) |
| cargo-audit | 0.22.2 (prebuilt release binary) |
| actionlint | 1.7.7 |

**Results:**

| # | Check | Command | Result |
| ---: | --- | --- | --- |
| 1 | fmt, program workspace | `cargo fmt --all -- --check` | PASS |
| 2 | fmt, LiteSVM workspace | `cd crates/ch_svm_tests && cargo fmt --all -- --check` | PASS |
| 3 | clippy, localnet features | `cargo clippy --workspace --all-targets --features ch_core/mock-oracle,ch_core/dev-venue-signer -- -D warnings` | PASS |
| 4 | clippy, production | `cargo clippy -p ch_core --all-targets --features pyth,venue-cpi -- -D warnings` | PASS |
| 5 | clippy, CPI suite config | `cargo clippy -p ch_core --all-targets --features mock-oracle,venue-cpi -- -D warnings` | PASS |
| 6 | clippy, Pyth suite config | `cargo clippy -p ch_core --all-targets --features pyth,dev-venue-signer -- -D warnings` | PASS |
| 7 | clippy, LiteSVM crate | `cd crates/ch_svm_tests && cargo clippy --all-targets -- -D warnings` | PASS |
| 8 | invalid feature sets must not compile | `cargo check -p ch_core --features "<f>"` for `""`, `mock-oracle`, `dev-venue-signer`, `pyth`, `venue-cpi`, `mock-oracle,pyth,venue-cpi`, `mock-oracle,venue-cpi,dev-venue-signer` | PASS (all 7 fail to compile, as required) |
| 9 | `cargo test --workspace` (literal, no features) | `cargo test --workspace` | FAILS BY DESIGN: `compile_error!` "no oracle backend selected" / "no venue authentication selected" (fail-closed, F9/F10) |
| 10 | workspace tests | `cargo test --workspace --features ch_core/mock-oracle,ch_core/dev-venue-signer` | PASS: 62 passed, 0 failed |
| 11 | ch_core tests, Pyth backend | `cargo test -p ch_core --features pyth,venue-cpi` | PASS: 13 passed |
| 12 | `anchor build` (literal, no features) | `anchor build -p ch_core` | FAILS BY DESIGN: same `compile_error!` as #9 |
| 13 | SBF build, localnet | `cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,dev-venue-signer` | PASS |
| 14 | SBF build, CPI suite | `… --features mock-oracle,venue-cpi --sbf-out-dir target/deploy-cpi` | PASS |
| 15 | SBF build, Pyth suite | `… --features pyth,dev-venue-signer --sbf-out-dir target/deploy-pyth` | PASS |
| 16 | SBF build, production | `… --features pyth,venue-cpi --sbf-out-dir target/deploy-prod` | PASS |
| 17 | SBF build, mock venues and ch_fund | `cargo build-sbf --manifest-path programs/{mock_perps_a,mock_perps_b,ch_fund}/Cargo.toml` | PASS |
| 18 | LiteSVM tests (compiled programs) | `cargo test --manifest-path crates/ch_svm_tests/Cargo.toml` | PASS: 66 passed, 0 failed |
| 19 | IDL build | `anchor idl build -p ch_core -o target/idl/ch_core.json -t target/types/ch_core.ts -- --features mock-oracle,dev-venue-signer` (with `RUSTUP_TOOLCHAIN` unset) | PASS |
| 20 | IDL + TS type match committed copies | `node scripts/ci/check-idl.cjs` | PASS |
| 21 | program ids consistent | `node scripts/ci/check-program-ids.cjs` | PASS |
| 22 | integration tests type-check | `npx tsc --noEmit -p .` | PASS |
| 23 | keeper tests | `cd services/keeper && npm test` | PASS: 8 passed, 0 failed |
| 24 | testbench vitest | `cd testbench && npx vitest run` | PASS: 46 passed |
| 25 | testbench tsc | `cd testbench && npx tsc --noEmit` | PASS |
| 26 | testbench build | `cd testbench && npm run build` | PASS (chunk-size and polyfill `eval` warnings, not suppressed) |
| 27 | npm production audit gate | `node scripts/ci/check-npm-audit.cjs testbench` / `… services/keeper` | PASS: 6 advisories each, all in the reviewed register |
| 28 | cargo audit, program lockfile | `cargo-audit audit` | PASS: 0 vulnerabilities; 1 unmaintained warning (RUSTSEC-2025-0141 `bincode`) |
| 29 | cargo audit, LiteSVM lockfile | `cd crates/ch_svm_tests && cargo-audit audit` | PASS: 0 vulnerabilities; 5 unmaintained warnings |
| 30 | workflow lint | `actionlint -shellcheck= ../.github/workflows/ci.yml` | PASS |
| 31 | measured costs still reproduce | `cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test perf -- --nocapture` | PASS: refresh at 8 legs 147,668 CU; 645-byte tx |
| 32 | local validator | `solana-test-validator --reset --quiet --ledger <tmp>` | **BLOCKED**: panics with `Os { code: 1314, "A required privilege is not held by the client." }` (symlink privilege on Windows) |
| 33 | Anchor TS integration tests | `anchor test --validator legacy --skip-build --skip-deploy` | **BLOCKED** by #32 (not run) |
| 34 | testbench smoke on localnet | `./scripts/dev-localnet.sh` then `npm --prefix testbench run smoke` | **BLOCKED** by #32 (not run) |
| 35 | GitHub Actions on this branch | (runs on push) | **NOT RUN**: branch not pushed; whether CI ran on `feat/clearinghouse-hardening` after it was pushed is unknown here (`gh` not installed) |

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
