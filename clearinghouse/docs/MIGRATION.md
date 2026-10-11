# Migration from the v0 (pairwise) protocol

Applies to the `feat/clearinghouse-hardening` branch relative to `origin/main`
at `7c5f4c4`. Nothing was deployed beyond localnet, so there is no on-chain
state to migrate; this lists every breaking change for integrators, scripts,
tests and documentation. Status: [`STATUS.md`](STATUS.md).

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
| `GlobalConfig` (keeper_authority, default_fund_program, flat params, reserved[31]) | `GlobalConfig { admin, proposed_admin, risk_epoch, params: ConfigParams, used_venues_bitmap, active_venues_bitmap, global_outstanding_credit, venue_outstanding[64], asset_outstanding[32], paused, bump }` |
| `VenueRegistration` (with `weight_bps`) | same minus `weight_bps` |
| `UserConsent` | unchanged |
| `PosSnapshot` seeds `[snapshot, user, venue_id]`, field `required_margin`, `snapshot_price` | seeds `[snapshot, user, venue_id, [asset_id]]`; fields `venue_index, version, notional, initial_margin, maintenance_margin, rent_payer`; no price |
| `MarginCredit` `[credit, user, venue_id]` | **removed** → `Portfolio` `[portfolio, user]` |
| `CorrelationMatrix` `[correlations]`, `[[i64;8];8]` | **removed** → `RiskModel` `[risk_model]`, zero-copy, 32 assets (upper-triangle i32), per-asset shock/gap/basis, δ, β, 8 labelled scenarios |
| `MockPrice` | unchanged (only with `--features mock-oracle`) |

## Instructions

| v0 | Now |
| --- | --- |
| `initialize(InitConfigParams)` | `initialize(ConfigParams)`; also creates `RiskModel` |
| `update_params(UpdateConfigParams)` | `update_params(ConfigParams)`; bumps `risk_epoch` |
| `register_venue(venue_id, program_id, authority, index, weight_bps)` | `register_venue(venue_id, program_id, authority, index)`; `init` only (F8) |
| — | `set_venue_active(is_active)`; bumps `risk_epoch` |
| `update_user_consent(is_active, bitmap)` | same args; now also creates/invalidates the `Portfolio` |
| `submit_position_snapshot(venue_id, asset_id, notional, is_long, required_margin)` | `submit_position_snapshot(venue_id, asset_id, notional, is_long, initial_margin, maintenance_margin)`; separate `payer`; registers the leg and invalidates the portfolio |
| `invalidate_snapshot` | **removed** → `close_position_snapshot` (venue, or user once consent no longer covers the venue / venue inactive) |
| `update_correlations([[i64;8];8])` | `update_correlations(Vec<CorrEntry{a,b,rho:i32}>)` (≤ 128 per tx) |
| — | `set_asset_risk`, `set_stress_params`, `set_scenario` |
| `compute_credit` (keeper) | **removed** → `refresh_portfolio` (permissionless; remaining accounts = all registered snapshots, then one oracle per leg) |
| `revoke_credit`, `revoke_if_unsafe`, `revoke_if_basis_gone` | **removed** → `invalidate_portfolio` (permissionless, needs a reason) |
| `set_keeper` | **removed** (no keeper role; keepers are liveness only) |
| `propose_admin`, `accept_admin`, `set_paused`, `set_mock_price` | unchanged signatures |

Pause semantics changed: pause now blocks only `refresh_portfolio`. Snapshot
writes, closes, consent changes and invalidation keep working, because they
only ever remove credit.

Error codes were renumbered (enum reordered, variants added and removed). Match
on error **names** (the TS tests do, via `expectTxError`).

## Build features

`mock-oracle` is no longer a default feature. `ch_core` fails to compile
unless exactly one of `mock-oracle` / `pyth` is selected:
`cargo build-sbf -- --features mock-oracle` (localnet/tests) or `--features pyth`.

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
