# Clearinghouse hardening — upgrade log

This is the working log for the `feat/clearinghouse-hardening` branch. It is
written so that a fresh session can resume from this file plus `git log`.

Canonical project status lives in [`docs/STATUS.md`](STATUS.md) (created in
Stage E). This file records *how* we got there and what evidence exists.

## Resume here

- Current stage: **A — baseline and safety** (in progress).
- Next action: see the stage checklist at the bottom of this file.

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

## 4. Stage checklist

- [x] A0 Preflight, baseline, this matrix
- [ ] A1 `expectTxError` helper; fix the 12 vacuous tests and every similar pattern
- [ ] A2 F8 registry fix + venue deactivation
- [ ] A3 F12 verification panel fix
- [ ] A4 CI toolchain pin + deploy step
- [ ] A5 `mock-oracle` no longer the default feature
- [ ] B  Portfolio account, joint netting, versions, atomic invalidation, caps, permissionless refresh
- [ ] C  IM-only relief, TradingPowerOnly, risk model, fund decision
- [ ] D  Roles, bounds, timelock, CPI venue auth, oracle, closure review
- [ ] E  Testbench, keeper, measurements, docs
- [ ] F  Final verification, re-rating, push
