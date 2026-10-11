# Testing and CI

## Test layers

| Layer | Where | What it proves | Count | Runs where |
| --- | --- | --- | ---: | --- |
| Engine unit + golden | `crates/ch_math/src`, `tests/golden_vectors.rs` | integer math, rounding, v0 golden vectors | 13 + 8 | local, CI |
| Engine properties | `tests/property_tests.rs`, `tests/portfolio_props.rs` | order invariance, caps, monotonicity, F3 regression (+23.7/+59.0/+133.5%) | 5 + 5 | local, CI |
| Stress | `tests/risk_stress.rs` | hypothetical shocks, decorrelation, gaps, 8-venue books, overflow fails closed, Nov-2022 replay from Binance daily data | 12 | local, CI |
| Rust/TS parity | `tests/vectors.rs` + `tests/vectors/portfolio.json` ↔ `testbench/src/vectors.test.ts` | the TS port matches Rust bit-for-bit on 15 cases | 1 + 15 | local, CI |
| Program unit | `programs/ch_core/src` | governance classification and bounds; oracle parsing against **real mainnet bytes** | 13 | local, CI (mock and pyth builds) |
| Venue rules | `crates/ch_client` | `usable_credit`, `max(IM − credit, MM)`, withdrawable equity | 5 | local, CI |
| **Instruction-level** | `crates/ch_svm_tests` (LiteSVM 0.18 on the compiled `.so`) | account constraints, signers, PDAs, error codes, invariants, CPI auth, Pyth path, lifecycle, measured CU | 66 | local, CI |
| Integration | `tests/ch_core.ts` (`anchor test`, validator) | the same program through the TS client | — | CI only (not yet executed) |
| Testbench | `testbench/src/**/*.test.ts(x)` | verification logic, venue rules, input handling, server-rendered a11y contract | 46 | local, CI |
| Testbench smoke | `testbench/scripts/smoke.e2e.ts` | the UI's chain layer end to end | 1 | CI only (not yet executed) |
| Keeper | `services/keeper/src/keeper.test.ts` | decisions, failure classes, two-instance convergence, paused mode, backoff | 8 | local, CI |

LiteSVM suites by file: `portfolio.rs` 28, `governance.rs` 9,
`venue_registry.rs` 8, `risk_model.rs` 6, `venue_cpi.rs` 5, `pyth_oracle.rs` 5,
`account_lifecycle.rs` 4, `perf.rs` 1.

Negative tests assert the **exact** error: `assert_ch_err` / `assert_anchor_err`
in Rust and `expectTxError` (which fails on success and on the wrong error)
in TypeScript. That rule exists because 12 negative tests in v0 could not fail
(F11).

### Evidence that tests can fail

| Check | Result |
| --- | --- |
| F8 registry tests against the **origin/main** binary | the 4 F8 tests fail, the 4 generic ones pass |
| Mutation: snapshot write no longer invalidates | killed |
| Mutation: refresh does not require every registered snapshot | killed |
| Mutation: counters not released | killed |
| Mutation: oracle evidence not required to postdate the baseline | killed (`oracle_evidence_not_newer_than_the_baseline_is_rejected`) |
| Mutation: an input without a label (testbench) | 4 a11y tests fail |
| Audit gate with one register entry removed | exit 1 |

## Running locally

```bash
cd clearinghouse

# Rust workspace (a feature choice is mandatory for ch_core)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features ch_core/mock-oracle,ch_core/dev-venue-signer -- -D warnings
cargo test --workspace --features ch_core/mock-oracle,ch_core/dev-venue-signer
cargo test -p ch_core --features pyth,venue-cpi

# Programs, then instruction-level tests
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,dev-venue-signer
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,venue-cpi --sbf-out-dir target/deploy-cpi
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features pyth,dev-venue-signer --sbf-out-dir target/deploy-pyth
cargo build-sbf --manifest-path programs/mock_perps_a/Cargo.toml
cargo build-sbf --manifest-path programs/mock_perps_b/Cargo.toml
cargo test --manifest-path crates/ch_svm_tests/Cargo.toml     # never skips: missing .so = panic with the build command

# IDL, program ids, TypeScript
anchor idl build -p ch_core -o target/idl/ch_core.json -t target/types/ch_core.ts -- --features mock-oracle,dev-venue-signer
node scripts/ci/check-idl.cjs && node scripts/ci/check-program-ids.cjs
npx tsc --noEmit -p .
(cd testbench && npm ci && npm test && npx tsc --noEmit && npm run build)
(cd services/keeper && npm ci && npm test)
node scripts/ci/check-npm-audit.cjs testbench && node scripts/ci/check-npm-audit.cjs services/keeper

# Needs a local validator
anchor test --validator legacy --skip-build --skip-deploy
./scripts/dev-localnet.sh && npm --prefix testbench run smoke
```

`crates/ch_svm_tests` is its own Cargo workspace, so LiteSVM's dependency pins
never change the program's `Cargo.lock`. A Windows build recipe (xwin, clang,
no admin rights) is in `docs/UPGRADE_LOG.md` §1b.

## CI (`.github/workflows/ci.yml`)

Pinned: Rust 1.99.0, Agave 4.3.0, Anchor CLI 1.2.0 (prebuilt), Node 20.

| Job | Steps |
| --- | --- |
| `rust` | fmt; clippy `-D warnings` for every allowed feature set; **must-fail** checks that ch_core does not compile with no features, with only an oracle or only a venue-auth mode, with both oracles, or with both venue-auth modes; workspace tests; Pyth unit tests; `cargo audit` (rustsec) |
| `sbf` | Agave + Anchor install; program-id consistency; SBF builds (localnet, CPI, Pyth, production, mocks); IDL regenerated and compared with the committed IDL **and** TS type; LiteSVM fmt, clippy and tests; artifact upload |
| `anchor-integration` | `anchor test` with the program genesis-loaded at its `declare_id!`; then a validator with `--bpf-program` and the testbench smoke |
| `testbench` | `npm ci`, vitest, tsc, build, audit gate |
| `keeper` | `npm ci`, tests, audit gate |
| `ci-required` | the single required check: fails unless **every** job above succeeded (a skipped job is a failure) |

**Status:** the workflow passes `actionlint`, but it has **not run on this
branch yet** (it runs on push). Until it does, the integration and smoke rows
above are unexecuted.
