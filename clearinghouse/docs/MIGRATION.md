# Migration from the v0 (pairwise) protocol

Applies to the `feat/clearinghouse-hardening` branch relative to `origin/main`
at `7c5f4c4`. Nothing was deployed beyond localnet, so there is no on-chain
state to migrate; this lists every breaking change for integrators, scripts,
tests and documentation. Status: [`STATUS.md`](STATUS.md).

## Changes on `feat/venue-proof-and-evidence` (approved items A–C)

Nothing is deployed beyond localnet, so there is still no on-chain state to
migrate: re-initialize localnet. Design and approval:
[`design/CHANGES_FOR_APPROVAL.md`](design/CHANGES_FOR_APPROVAL.md).

| Item | Breaking change | Who must update |
| --- | --- | --- |
| A2 | Credit is additionally capped at `(ΣIM − R_p)⁺` (engine semantics; no layout change) | anyone recomputing credit off-chain: use the TS port or `ch_math` at this commit |
| A3 | `ConfigParams` gains `hedge_break_bps: u16` (after `timelock_slots`): `GlobalConfig` grows 2 bytes; `initialize`, `update_params` and `RiskChange::Params` carry it. Production builds reject 0 | every `initialize` / `update_params` caller (scripts, testbench, keeper fixtures, `tests/ch_core.ts`); IDL and TS types regenerated |
| A4 | Refresh checks the correlation sub-matrix for positive definiteness; if it fails, credit is 0 and `Portfolio.flags` (new `u8` after `hedge_benefit_ppm`, +1 byte) gets `PSD_FALLBACK`; `PortfolioRefreshed` gains `flags`. A non-PSD book no longer reverts with `NegativeVariance`, it refreshes to 0 credit. `ch_math`: `PortfolioCredit.psd_fallback`, `RiskModel::portfolio_risk_report` | account decoders (IDL regenerated); off-chain recomputation (TS port) |
| A1 | `GlobalConfig` + `venue_accepts[64]` (+512 bytes, 1,565 total); `LegState` + `view_mask` (Portfolio 803 bytes); new instruction `set_venue_acceptance(accepts_mask)` (venue authority); new invalidation reason `ACCEPTANCE_REVOKED = 1 << 8`; event `VenueAcceptanceUpdated`. **Default: a venue accepts only its own reports**, so cross-venue credit needs both venues to allowlist. `ch_client::usable_credit` and `Portfolio::usable_credit` gain a `venue_accepts_now` argument; `LegCredit` gains `view_mask`. Refreshes with partial views may need a `SetComputeUnitLimit` instruction | every venue integration (call `set_venue_acceptance`, pass the mask to `usable_credit`); keeper (compute budget, done); testbench smoke (allowlists A↔B, done); IDL and TS types |
| A7 | `initialize(params, risk_authority, venue_manager, guardian)` (+3 pubkeys). Production builds reject non-distinct role keys in `initialize`, `set_roles` and `accept_admin` (new error `RolesNotDistinct`, appended) | deployment scripts; testbench and `tests/ch_core.ts` pass the admin three times (localnet) |
| A5 | `ConfigParams` + `max_leg_notional`, `max_venue_notional_per_user`, `max_notional_growth_bps`, `notional_growth_window_slots`, `notional_growth_floor`; `PosSnapshot` + `window_start_slot`, `window_base_notional`, `mark_price` (180 bytes); `Portfolio` + `registered_notional[8]`; `submit_position_snapshot` gains `mark_price_micro: u64`, a `risk_model` account (after `system_program`) and an optional oracle remaining account; 5 new errors (appended). Reports that break a bound now fail | every venue's CPI (account list + data); mock venues, testbench, `tests/ch_core.ts`, LiteSVM harness (done); IDL |
| B3 | `ConfigParams` + `throttle_move_start_bps`, `throttle_move_full_bps`, `throttle_conf_start_bps`; `LegState` + `baseline_ema`; `Portfolio.flags` bit 1 `THROTTLED`; the oracle reading carries the EMA (Pyth `ema_price`, fail closed if ≤ 0); `MockPrice` + `ema_price_micro`, `set_mock_price` + `ema` argument (localnet) | IDL; testbench (`setMockPrice` takes an optional EMA), `tests/ch_core.ts`; off-chain verifiers (apply the throttle; the TS port has `legScalePpm`) |

## Economic semantics (read this first)

| v0 | Now | Why |
| --- | --- | --- |
| Credit per venue *pair*, accumulated across pairs | One joint computation per user over all legs | Pairwise over-credit +23.7% / +59.0% / +133.5% with 2/3/5 hedge venues (F3) |
| Risk = required margin, netted with ρ | Risk = notional × per-asset shock; direction-aware correlation stress; market-wide, gap, basis and governance scenarios; `R_p = max(parametric, scenarios)` | Margin is not risk; v0 freed $10,940 against an $18,020 realized loss in the Nov-2022 replay |
| `credit = (ΣIM − σ)(1 − h)` | `credit = floor(ΣIM·(S − R_p)/S)·(1 − h)` (identical when risk = IM) | Generalizes v0 to a risk measure that is not IM |
| Pro-rata remainder to the last leg | Remainder dropped (each share floored) | Leg order must not change any result |
| Credit could reduce a venue requirement to 0 | Credit per leg ≤ `IM − MM`; venue applies `max(IM − credit, MM)` | Losing credit never by itself liquidates (F1, F6) |
| Credit valid until TTL regardless of input changes | Any snapshot write / close / consent change invalidates in the same instruction; any parameter, risk-model or venue-lifecycle change bumps `risk_epoch` | F2 |
| Price-guard baseline on the (rewritable) snapshot | Baseline price + confidence stored in the portfolio at computation | F5 |
| Keeper-only `compute_credit` | Permissionless, deterministic `refresh_portfolio` | F14: keeper is liveness only |
| `max_credit_per_user` per call | Per-portfolio cap + per-venue, per-asset and global outstanding-credit caps with counters | F3, F1 (bounded outstanding credit) |

## Accounts

| v0 | Now |
| --- | --- |
| `GlobalConfig` (keeper_authority, default_fund_program, flat params, reserved[31]) | `GlobalConfig { admin, proposed_admin, risk_authority, venue_manager, guardian, risk_epoch, params: ConfigParams, used_venues_bitmap, active_venues_bitmap, global_outstanding_credit, venue_outstanding[64], asset_outstanding[32], paused, bump }` |
| `VenueRegistration` (with `weight_bps`) | same minus `weight_bps`, plus `activation_eta_slot` |
| `UserConsent` | unchanged |
| `PosSnapshot` seeds `[snapshot, user, venue_id]`, field `required_margin`, `snapshot_price` | seeds `[snapshot, user, venue_id, [asset_id]]`; fields `venue_index, version, notional, initial_margin, maintenance_margin, rent_payer`; no price |
| `MarginCredit` `[credit, user, venue_id]` | **removed** → `Portfolio` `[portfolio, user]`; each `LegState` stores `baseline_price`, `baseline_conf`, `baseline_publish_ts` |
| `CorrelationMatrix` `[correlations]`, `[[i64;8];8]` | **removed** → `RiskModel` `[risk_model]`, zero-copy, 32 assets (upper-triangle i32), per-asset shock/gap/basis, δ, β, 8 labelled scenarios, Pyth feed id per asset (`oracle_feeds`) |
| `MockPrice` | unchanged (only with `--features mock-oracle`) |
| — | `PendingChange` `[pending_change]`: at most one queued risk change (`RiskChange` enum, proposer, proposed/eta slot) |
| `ConfigParams` | adds `timelock_slots` |

## Instructions

| v0 | Now |
| --- | --- |
| `initialize(InitConfigParams)` | `initialize(ConfigParams)`; also creates `RiskModel` and `PendingChange`; all four roles start as the admin |
| `update_params(UpdateConfigParams)` | `update_params(ConfigParams)`; `risk_authority` signs; accounts `{config, risk_model, pending_change, authority}`; bounds + step limits; credit-reducing changes apply at once, others queue for `timelock_slots`; applying bumps `risk_epoch` |
| `register_venue(venue_id, program_id, authority, index, weight_bps)` | `register_venue(venue_id, program_id, authority, index)`; `init` only (F8); signer is `venue_manager` (account `authority`) plus a separate `payer`; with `venue-cpi` the venue authority must be the venue program's `[b"clearinghouse"]` PDA; the venue starts **inactive** with an activation ETA (unless `timelock_slots = 0`) |
| — | `deactivate_venue` (venue_manager or guardian, immediate), `request_venue_activation`, `activate_venue` (after the timelock); each lifecycle change bumps `risk_epoch` |
| `update_user_consent(is_active, bitmap)` | same args; now also creates/invalidates the `Portfolio` |
| `submit_position_snapshot(venue_id, asset_id, notional, is_long, required_margin)` | `submit_position_snapshot(venue_id, asset_id, notional, is_long, initial_margin, maintenance_margin)`; separate `payer`; registers the leg and invalidates the portfolio |
| `invalidate_snapshot` | **removed** → `close_position_snapshot` (venue, or user once consent no longer covers the venue / venue inactive) |
| `update_correlations([[i64;8];8])` | `update_correlations(Vec<CorrEntry{a,b,rho:i32}>)` (≤ 128 per tx) |
| — | `set_asset_risk`, `set_stress_params`, `set_scenario` (same governance path as `update_params`; correlations move ≤ 0.25 per update and always queue) |
| — | `apply_pending_change` (risk_authority or admin, after ETA, re-validated), `cancel_pending_change` (risk_authority, guardian or admin) |
| — | `set_roles(risk_authority, venue_manager, guardian)` (admin) |
| — | `bump_risk_epoch` (guardian or admin): emergency invalidation of all credit |
| `compute_credit` (keeper) | **removed** → `refresh_portfolio` (permissionless; remaining accounts = all registered snapshots, then one oracle per leg) |
| `revoke_credit`, `revoke_if_unsafe`, `revoke_if_basis_gone` | **removed** → `invalidate_portfolio` (permissionless, needs a reason; accounts `{config, risk_model, user_consent, portfolio}` + optionally one oracle per leg). An oracle reading counts as evidence only if it was published **after** the leg's baseline; older readings are ignored |
| — | `set_oracle_feed(asset_id, feed_id)` (risk governance; setting/switching queues behind the timelock, clearing applies at once) |
| — | `close_user_accounts` (user; all snapshots closed first; releases counters; rent to the user). Re-creating starts an EMPTY portfolio |
| `set_keeper` | **removed** (no keeper role; keepers are liveness only) |
| `propose_admin`, `accept_admin`, `set_mock_price` | unchanged signatures |
| `set_paused(bool)` | split: `pause` (guardian or admin; account `authority`) and `unpause` (admin only) |

Pause semantics changed: pause now blocks only `refresh_portfolio`. Snapshot
writes, closes, consent changes and invalidation keep working, because they
only ever remove credit.

Error codes were renumbered (enum reordered, variants added and removed). Match
on error **names** (the TS tests do, via `expectTxError`).

## Build features

`ch_core` has no default features and fails to compile unless it gets exactly
one oracle feature **and** exactly one venue-authentication feature:

| Feature | Meaning |
| --- | --- |
| `mock-oracle` | admin-set prices; localnet/tests only; also sets `MIN_TIMELOCK_SLOTS = 0` |
| `pyth` | Pyth `PriceUpdateV2` accounts (owner `rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ`), parsed without the Pyth SDK; Full verification only; feed id must equal `risk_model.oracle_feeds[asset]` (all zeros = fail closed) |
| `dev-venue-signer` | venue authority may be any keypair (localnet/tests) |
| `venue-cpi` | venue authority must be the venue program's `[b"clearinghouse"]` PDA, so snapshots arrive only by CPI from that program (F10) |

Builds: localnet/tests `--features mock-oracle,dev-venue-signer`; CPI test
suite `--features mock-oracle,venue-cpi --sbf-out-dir target/deploy-cpi`;
production `--features pyth,venue-cpi`.

Governance bounds (production): haircut ≥ 5%, `max_credit_bps_of_im` ≤ 90%,
timelock between 216,000 and 1,296,000 slots, haircut may drop ≤ 10 points per
change, shocks/gaps/basis have floors and step limits. See
`programs/ch_core/src/governance.rs`.

## `ch_client`

`effective_required_margin(own_required, credit, valid_until, now)` is removed:
it could return less than maintenance margin. Use `usable_credit(view,
config_epoch, now_slot, venue_index, asset_id)` then
`effective_initial_margin(im, mm, credit)`.

## Tests

The old `tests/ch_core.ts` exercised the removed pairwise instructions. Its
coverage maps to:

| Old test area | New coverage |
| --- | --- |
| Golden vectors (haircut 0 / 20%) | `ch_math` golden + vectors (unchanged functions); engine golden in `portfolio.rs` unit tests; stressed goldens in `tests/vectors/portfolio.json` |
| Snapshot auth / consent / venue bit failures | `ch_svm_tests/tests/portfolio.rs::snapshot_validation_rejects_contradictory_inputs`; `tests/ch_core.ts` "refresh and snapshot input validation" |
| Wrong oracle accounts | `refresh_rejects_…` (wrong oracle), `wrong_oracle_in_invalidate_is_an_error_not_a_trip` |
| Stale snapshot / stale price / wide confidence / price moved | `stale_snapshot_allows_invalidation_and_blocks_refresh`, `failed_refresh_never_creates_usable_credit`, `price_breach_vs_stored_baseline_allows_invalidation` |
| Correlation validation and staleness | `risk_model.rs`, TS "rejects out-of-range correlations" (staleness: `risk_model_max_age_slots`) |
| Expired credit | `expiry_zeroes_usable_credit_and_invalidation_releases_exactly_once` |
| Revocation paths | `invalidate_portfolio` tests (all reasons), consent / venue / snapshot atomic invalidation tests |
| Distinct venues / margin validation | `refresh_rejects_incomplete_duplicate_foreign_and_fake_snapshots`, snapshot validation |
| Register venue validation | `ch_svm_tests/tests/venue_registry.rs` (+ F8 regressions) |
| Admin controls / pause | `config_update…`, `pause_blocks_credit_creation_but_not_removal`, TS "governance" block |

## Testbench and keeper

| v0 | Now |
| --- | --- |
| `testbench/src/App.tsx` (one 1,900-line file, pairwise API) | `src/chain/` (`Bench`), `src/model/` (views, verification), `src/components/` (one per panel) |
| expectation computed from UI defaults and the browser clock | recomputed from on-chain config, risk model, stored legs, counters and the Clock sysvar (F12) |
| `npm run smoke` (`node --experimental-strip-types scripts/smoke.ts`) | `npm run smoke` (vitest, `vitest.smoke.config.ts`, `scripts/smoke.e2e.ts`); needs a running localnet |
| `npm run sync-idl` copied the JSON IDL | also copies the generated TS type to `src/idl/ch_core.ts`; CI checks both |
| dev server on `0.0.0.0` | `127.0.0.1` only |
| `services/keeper` stub | minimal keeper: `KEEPER_RPC_URL`, `KEEPER_KEYPAIR`, `KEEPER_ORACLES` (`mock` or a JSON map) |
| `scripts/dev-localnet.sh` built with `mock-oracle` | builds with `mock-oracle,dev-venue-signer` and syncs the IDL + type |

