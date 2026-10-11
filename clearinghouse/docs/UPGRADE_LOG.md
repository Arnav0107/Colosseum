# Clearinghouse hardening — upgrade log

This is the working log for the `feat/clearinghouse-hardening` branch. It is
written so that a fresh session can resume from this file plus `git log`.

Canonical project status lives in [`docs/STATUS.md`](STATUS.md) (created in
Stage E). This file records *how* we got there and what evidence exists.

## Resume here

- Stage A is **done** (commits `bce171e`..`6626454`). Next: **Stage B** (portfolio account).
- Local build recipe (Windows host, no admin): see section 1b. Source
  `%LOCALAPPDATA%/xwin-bin/sbf-env.sh` in Git Bash before building.

## 1. Environment preflight (2026-10-11, Windows 11, Git Bash)

| Tool | Result |
| --- | --- |
| `rustc --version` | not installed at start → installed per-user via rustup (`stable-x86_64-pc-windows-gnu`): **rustc 1.99.0** |
| `cargo --version` | cargo 1.99.0 |
| `solana --version` | not installed at start → Agave Windows release from `release.anza.xyz/stable`: **solana-cli 4.3.0** (unpacked into a temp dir, not on system PATH) |
| `cargo build-sbf --version` | cargo-build-sbf 4.4.0, platform-tools v1.57. First attempt: platform-tools download **timed out** inside `cargo-build-sbf`; retrying with resumable `curl`. |
| `anchor --version` | **not installed**. No prebuilt Windows Anchor CLI; see notes below. |
| `node --version` | v25.6.1 |
| `npm --version` | 11.9.0 |
| `yarn --version` | **not installed** |
| `gh auth status` | **`gh` not installed** — push account cannot be confirmed via gh (see Section 1.5 of the brief). |
| WSL | not installed |
| MSVC build tools | not installed (reason for using the `-gnu` Rust host) |

Toolchain facts from the lockfile (re-verified):

- `programs/ch_core/Cargo.toml` requests `anchor-lang = "1.1.2"`; `Cargo.lock`
  resolves **anchor-lang 1.2.0**, which pulls the split Solana **3.x** crates
  (`solana-account-info 3.1.1`, `solana-instruction 3.5.1`, `solana-pubkey 3.0/4.4`, …).
- The TS clients use `@coral-xyz/anchor ^0.30.1`.
- CI (`.github/workflows/ci.yml`, job `anchor-tests`) installs **Solana 1.18.26 +
  Anchor CLI 0.30.1**. Neither can build a program that depends on anchor-lang 1.2.0 /
  Solana 3.x crates, and the job never runs `anchor deploy`/`anchor test`, so the
  integration suite runs against a validator **without the program loaded**.

### 1b. Local toolchain recipe (how on-chain checks run on this machine)

All per-user, no system changes. The user approved accepting the Microsoft
build-tools license via xwin (2026-10-11).

1. Rust: rustup with `stable-x86_64-pc-windows-gnu` (default) and
   `stable-x86_64-pc-windows-msvc`.
2. Agave 4.3.0 Windows release unpacked to `%TEMP%/agave-dl`; platform-tools
   v1.57 extracted into `~/.cache/solana/v1.57/platform-tools` (tar needs
   `--force-local` on Windows paths).
3. MSVC CRT + Windows SDK libs/headers via `xwin 0.6.6 --accept-license splat
   --disable-symlinks` into `%LOCALAPPDATA%/xwin`; `rust-lld` copied as
   `lld-link.exe` and used as the MSVC linker (`LIB` points at xwin).
4. LLVM 21.1.8 `clang.exe` (GNU driver, so blst uses its GAS assembly) for C deps;
   `CARGO_FEATURE_PURE=1` so blake3 skips its cl.exe-only SIMD build.
5. Anchor CLI 1.2.0 prebuilt Windows binary. `anchor idl build` must run with
   `RUSTUP_TOOLCHAIN` **unset** (Anchor mangles it into `'{toolchain}'`).
6. `solana-test-validator` does **not** run here (needs the symlink privilege,
   os error 1314), so the TS integration suite is environment-blocked locally;
   LiteSVM covers instruction-level behaviour instead.

Commands:
```
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle
cargo test --manifest-path crates/ch_svm_tests/Cargo.toml        # LiteSVM, runs the .so
cargo test --workspace --features ch_core/mock-oracle            # pure Rust
(unset RUSTUP_TOOLCHAIN; anchor idl build -p ch_core -o target/idl/ch_core.json -t target/types/ch_core.ts -- --features mock-oracle)
```

## 2. Baseline reproduction (before any change, commit `7c5f4c4`)

| Command | Result |
| --- | --- |
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | **21 passed** (ch_client 3, ch_core oracle 5, ch_math golden 8, ch_math property 5) |
| `testbench: npm ci && npm test` | vitest **5/5 passed** |
| `testbench: npx tsc --noEmit` | clean |
| Anchor integration tests (`tests/ch_core.ts`) | **not run** — no Anchor CLI, no deployable build yet |
| `testbench: npm run smoke` | **not run** — needs a running validator with the program deployed |

Baseline matches the review's numbers.

## 3. Traceability matrix

Status legend: **Open** (not started), **WIP**, **Fixed** (root cause fixed + regression
test passing), **Fixed (env-blocked test)** (fix + test written, test cannot run here),
**Partial**, **Won't fix** (with reason).

| ID | File / function | Verdict | Root cause | Planned correction | Regression test | Status | Remaining risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| F1 | `ch_core::PosSnapshot`, `compute_credit`; `programs/ch_fund` | **Confirmed.** Arithmetic re-derived: hedged golden pair with 20% haircut gives B a credit of 5,470,177,871; A's effective requirement 10,000 − 5,470.18 = **$4,529.82** > $2,000 equity → still liquidatable. No equity/MM field; `ch_fund` is a 22-line stub; `fund_cap` is never called on-chain. | Credit is computed from IM only and presented as if it moved collateral. Nothing bounds credit by a loss-absorbing resource or by the venue's maintenance margin. | IM-only relief capped at `IM − MM` per leg, `TradingPowerOnly` flag, global outstanding-credit cap, explicit 4-capability separation; fund decision documented (Stage C). | Stage C | Open | Venue must enforce the rule; no loss coverage exists. |
| F2 | `submit_position_snapshot` (~L213), `MarginCredit` (~L1332), `revoke_if_basis_gone` (~L567) | **Confirmed** by code reading: the snapshot write never touches credit accounts; credit stores no input binding; basis-gone only fires on consent/venue/staleness/notional==0. | Credit is a separate account with no reference to the inputs it was derived from. | Portfolio PDA with snapshot versions + atomic invalidation inside every snapshot/consent write (Stage B). | Stage B (shrink, flip, removal) | Open | — |
| F3 | Credit PDA seeds `[credit,user,venue]` (~L1007/1016), per-call `max_credit_per_user` (~L436) | **Confirmed numerically** (scratch program over `ch_math`, haircut 20%, ρ=0.8, 1 long leg + n equal-size short legs on n venues): pairwise vs joint credit = **+23.7% / +59.0% / +133.5%** for n = 2 / 3 / 5. For n=3, margin held 18.12B < netted risk 22.80B. (With the hedge *split* across venues the pairwise scheme instead *under*-credits; the error is in both directions.) | Each pair is netted independently and the per-venue credit account is overwritten/accumulated across pairs. | Single joint computation per user over all legs (Stage B). | Stage B (permanent version of the scratch repro) | Open | — |
| F4 | Snapshot seeds `[snapshot,user,venue]` (~L900); `[[i64;8];8]` | **Confirmed.** | Seeds omit asset; matrix size hard-coded. | Seeds `[snapshot,user,venue_id,asset_id]`; larger asset universe (Stage B). | Stage B | Open | Bounded `MAX_LEGS`/`MAX_ASSETS` remain (documented). |
| F5 | `snapshot.snapshot_price` overwritten (~L220), read by guard (~L379, ~L1234) | **Confirmed.** A venue re-submitting the same position resets the baseline the guard compares against. | Baseline lives on the mutable input, not on the credit. | Baseline price/conf stored in the portfolio at computation time (Stage B). | Stage B | Open | — |
| F6 | `check_leg_guard_tripped` (~L1207) | **Confirmed.** Any single leg moving > `max_move_bps` revokes, irrespective of hedge P&L. Permissionless. | Guard is per-leg and binary. | IM-only relief makes revocation non-liquidating (Stage C); breach threshold kept but documented as a *recompute trigger*, not a solvency event. | Stage C | Open | Credit can still be withdrawn in a crash (by design: trading power, not collateral). |
| F7 | `UpdateCorrelations` constraint (~L924), `ComputeCredit` keeper constraint (~L948), `validate_config_params` (~L20) | **Confirmed.** Keeper alone sets correlations and mints credit; admin can also set correlations; haircut 0 and cap 100% accepted; no timelock. | One role (keeper) is both risk oracle and minter. | Separate roles, floors/ceilings, per-update deltas, propose→apply timelock for risk-increasing changes, permissionless deterministic refresh (Stage D/B). | Stage D | Open | Admin/risk keys still trusted within bounds. |
| F8 | `register_venue` (~L122) | **Confirmed** by code reading: when the index bit is already set, the check compares the *stored* `venue_id` of the PDA being written with the argument — which is the same PDA, so re-registering existing venue X with Y's index passes and both venues then share one consent bit. No instruction ever sets `is_active = false`. | Uniqueness check reads the wrong account; no lifecycle instruction. | Index-keyed PDA (`[venue_index, idx]`) + `set_venue_active` + `active_venues_bitmap` (Stage A). | Stage A | Open | — |
| F9 | `oracle.rs` (~L143), `ch_core/Cargo.toml` L11 | **Confirmed.** `default = ["mock-oracle"]`; `pyth` path returns `OracleNotConfigured`. | Mock is the default feature. | No default oracle; `compile_error!` unless exactly one backend; Pyth path (Stage A/D). | Stage A/D | Open | — |
| F10 | `SubmitPositionSnapshot.venue_authority` (~L912) | **Confirmed.** `venue_program_id` is stored but never read. | Hot-key signer used as a CPI stand-in. | CPI-PDA auth in production; keypair only under `dev-venue-signer` (Stage D). | Stage D | Open | Venues still self-report. |
| F11 | `tests/ch_core.ts`; `.github/workflows/ci.yml` | **Confirmed exactly.** Script over all 33 `expect.fail` sites: 18 rethrow `AssertionError`; **12 are vacuous** (L607, 628, 656, 679, 740, 863, 932, 1071, 1090, 1131, 1170, 1208 — the fail message contains the asserted substring and the catch swallows it); 3 more (L520, 558, 1043) are unguarded but not vacuous only because of letter case. CI toolchain (Solana 1.18 / Anchor 0.30) cannot build anchor-lang 1.2.0 and never deploys. | Ad-hoc try/catch pattern; CI pinned to an old toolchain. | `expectTxError` helper used by every negative test; CI on Agave 3.x/4.x + Anchor CLI 1.x with `anchor test` (Stage A). | Stage A | Open | — |
| F12 | `testbench/src/App.tsx` (~L1079, ~L123/1436) | **Confirmed.** Chain initialized with 2000 bps haircut and 75% cap; TS check calls `creditTotal(sumReq, combined, 0)` → expected 6,837,722,339 vs on-chain 5,470,177,871 ⇒ MISMATCH on the happy path. Price countdown uses `Date.now()` at page load. | TS expectation ignores on-chain config. | Read config on-chain; shared credit function (Stage A, reworked in Stage E). | Stage A | Open | — |
| F13 | `SPEC.md`, `testbench/README.md`, root `README.md` | **Confirmed.** SPEC §4.1 argument order wrong for `update_user_consent` (code: `is_active, bitmap`) and `submit_position_snapshot` (code: `venue_id, asset_id, notional, is_long, required_margin`); §4.2/4.3 list instructions that do not exist; §3.1 `GlobalConfig` layout differs (field order, `max_price_age_secs` type, reserved size, missing fields). Root README claims the script "funds test accounts" (it airdrops only the deployer). "Verifiable" credits are keeper-attested. | Docs written ahead of/independently from code. | Reconcile in Stage E; single `docs/STATUS.md`. | Stage E (doc review) | Open | — |
| F14 | `services/keeper/src/index.ts`; `MAX_CREDIT_TTL_SLOTS` (L16) | **Confirmed.** Keeper is a stub; credits expire ≤ 3,000 slots. | Credit computation is keeper-gated and nothing automates it. | Permissionless refresh (Stage B) + minimal idempotent keeper (Stage E). | Stage E | Open | Liveness depends on someone calling refresh. |

### Interactions found while verifying

- **F2 × F5:** a venue can reset the price-guard baseline simply by re-submitting the
  same position, and that same resubmission leaves the old credit alive.
- **F8 × consent:** an index collision makes a user's consent to venue Y also
  authorize venue X (same bitmap bit), and credits computed for X are checked
  against Y's bit.
- **F3 × `max_credit_per_user`:** the per-user cap is applied per pair call, so it
  does not bound a user's aggregate credit.
- **F11 × everything:** none of the on-chain negative paths has ever been proven to
  fail closed in CI, because CI never deployed the program.

## 4. Stage A results

| Check | Result |
| --- | --- |
| `cargo test --workspace --features ch_core/mock-oracle` | 21 passed |
| `cargo test --manifest-path crates/ch_svm_tests/Cargo.toml` | 8 passed (F8) |
| Same 8 LiteSVM tests against the **origin/main** `.so` | 4 F8 tests fail as expected, 4 generic pass |
| `ts-mocha tests/helpers/expectTxError.test.ts` (ts-node) | 7 passing |
| `npx tsc --noEmit -p tsconfig.json` (integration suite) | clean — first time it type-checks |
| testbench `vitest` / `tsc` / `build` | 7 passed / clean / ok |
| actionlint on `ci.yml` | clean |
| `anchor test` locally | **blocked** (validator needs symlink privilege) |

Additional findings while doing Stage A (not in the original review):
- **F11b** root TypeScript 4.9.5 cannot parse the locked `@types/node` 26 →
  `ts-mocha` would have failed before running any test. Fixed (TS 5.9.3).
- **F11c** `tests/ch_core.ts` never type-checked against Anchor ≥0.30
  (`.accounts()` with resolvable PDAs; a doubled `.accounts()` call). Fixed.
- **F11d** no program keypair for `declare_id!` is in the repo, so
  `anchor deploy` / the old localnet script would deploy to the wrong address.
  Fixed with genesis/`--bpf-program` loading.
- **Dependency hygiene:** adding litesvm to the main workspace would silently
  re-resolve 5 crates in the program's own dependency graph; the test crate is
  a separate workspace.

## 5. Stage checklist

- [x] A0 Preflight, baseline, this matrix
- [x] A1 `expectTxError` helper; all 33 negative tests converted (`1afd67e`)
- [x] A2 F8 registry fix + venue deactivation; LiteSVM harness (`de65208`)
- [x] A3 F12 verification panel fix (`db5d6a4`)
- [x] A4 CI toolchain pin + genesis-loaded program + gate job (`6626454`)
- [x] A5 `mock-oracle` no longer the default feature (`f20ecb4`)
- [ ] B  Portfolio account, joint netting, versions, atomic invalidation, caps, permissionless refresh
- [ ] C  IM-only relief, TradingPowerOnly, risk model, fund decision
- [ ] D  Roles, bounds, timelock, CPI venue auth, oracle, closure review
- [ ] E  Testbench, keeper, measurements, docs
- [ ] F  Final verification, re-rating, push
