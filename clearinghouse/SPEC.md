# Clearinghouse specification

Reconciled with the code on branch `feat/clearinghouse-hardening`. When this
file and the code disagree, the code (and its IDL, `testbench/src/idl/ch_core.json`)
wins, and this file is the bug. Status of every component:
[`docs/STATUS.md`](docs/STATUS.md). Changes from v0: [`docs/MIGRATION.md`](docs/MIGRATION.md).

## 1. What the protocol does

Venues (perpetuals programs) report each user's positions. `ch_core` computes
one joint risk number per user over all legs and all venues, and derives
**trading-power credit**: an amount by which a venue may lower that user's
*initial* margin requirement for new risk.

- Credit never lowers a requirement below maintenance margin: the venue applies
  `max(IM − credit, MM)`, and each leg's credit is ≤ `IM − MM`.
- Credit is not collateral, is never withdrawable, and losing it never by
  itself makes an account liquidatable.
- No program holds user funds. `ch_fund` is a stub; there is no loss coverage
  ([`docs/FUND_DESIGN.md`](docs/FUND_DESIGN.md)).

Units: amounts are micro-USD in `u64` (1 USD = 1,000,000). Rates are bps
(1/10,000) or ppm (1/1,000,000). There is no floating point on chain.

## 2. Math

Full derivation, worked example and stress results:
[`docs/RISK_MODEL.md`](docs/RISK_MODEL.md). Reference implementation:
`crates/ch_math` (`portfolio.rs`, `risk.rs`). The TS port is bit-identical,
enforced by shared vectors.

For legs *i* with notional `N_i`, side `s_i = ±1`, initial margin `IM_i` and
maintenance margin `MM_i`, and per-asset shock `k_a`, gap `g_a`, basis `b_a`:

```
R_i  = ceil(N_i · k_asset(i) / 10_000)                    standalone risk
S    = Σ R_i
P    = isqrt_ceil( ceil( Σ_i Σ_j x_i x_j ρ*_ij / 1e6 ) ),  x_i = s_i · R_i
       ρ*_ij = 1e6 if i = j
             = clamp(ρ(a_i,a_j) ± δ)          different assets: + if same side, − if opposite
             = 1e6 or 1e6 − β                 same asset: same side / opposite side
L    = max over scenarios of the portfolio loss:
       market-wide ±k on every leg; ±g_a on one asset; ±b_a on one leg that
       shares its asset with another leg; each governance scenario vector
R_p  = max(P, L), or max(S, L) when the base ρ sub-matrix over the assets
       present is not positive definite (A4: integer LDLᵀ, pivots ≥ 1 ppm;
       credit is then 0 and Portfolio.flags gets PSD_FALLBACK)
C    = floor( ΣIM · (S − R_p)⁺ / S ) · (10_000 − haircut_bps) / 10_000, then ≤ max_credit_per_user,
       then ≤ (ΣIM − R_p)⁺                                (floor, A2)
c_i  = min( floor(C · IM_i / ΣIM), IM_i · max_credit_bps_of_im / 10_000, IM_i − MM_i )
```

**Counterparty allowlists (A1).** Venue `v` accepts reports only from the
venues in `venue_accepts[v]` (its own bit implied; default 0 = only itself).
For a leg on venue `v`, `view(v)` = the legs of accepted venues present;
`c_view_i` = the formula above over `view(v)` alone (0 for a single leg); the
leg's credit is `min(c_i, c_view_i)` and records `view_mask` = the venues in
`view(v)`. A non-accepted venue's report can therefore never raise it, and one
hedge is never used twice. Code: `ch_math::view::compute_view_credit`.

**Regime throttle (B3).** Each leg's oracle reading gives
`m = ⌈|price − ema| / ema⌉` and `c = ⌈conf / price⌉` (bps). Its scale is
`min(lin(m; throttle_move_start_bps, throttle_move_full_bps), lin(c; throttle_conf_start_bps, max_conf_bps))`,
where `lin` is 1 below start, 0 at full, and linear (ppm, floored) in
between; a start of 0 turns that part off. Every credit is multiplied by the
smallest leg scale (floored). Code: `ch_math::throttle`. `ema` is the oracle's
own EMA (Pyth `ema_price`; `MockPrice.ema_price_micro` on localnet). Price,
confidence and EMA are stored per leg, so anyone can recompute the scale.

Then protocol budgets: per-venue, per-asset and global outstanding-credit caps
scale the credits down pro rata (floored), in that order.

**Hedge break (A3, on-chain):** when `ConfigParams.hedge_break_bps = h > 0`,
the scenario set also contains "every leg adverse at once by
`ceil(shock · h / 10,000)` bps" (each leg's loss rounded up), so
`R_p ≥ S · h / 10,000` whatever `ρ` says. `ch_core` wraps `StressedRiskModel`
in `ch_math::risk::HedgeBreakModel`. Bounds: `2,500 ≤ h ≤ 10,000`; `h = 0`
(disabled) only in `mock-oracle` builds. Default in scripts and fixtures:
5,000. Raising `h` applies at once; lowering it waits for the timelock.
Sensitivity: [`docs/RISK_MODEL.md`](docs/RISK_MODEL.md) §3a.

The v0 functions (`combined_risk`, `credit_total`, `split_pro_rata`) remain in
`ch_math` with their golden vectors (`6_324_555_321`, `13_675_444_679`,
`[6_837_722_339, 6_837_722_340]`) as the margin-as-risk reference model.
They are not what the program uses.

Worked example (pinned in Rust unit tests, the parity vectors, LiteSVM and the
testbench): long SOL / short ETH, $50,000 notional each, IM $10,000, MM
$5,000, ρ = 0.8, shocks 10% / 8%, δ = 0.1, haircut 20% →
S = $9,000, R_p = $5,000 (SOL alone gaps −10%; the market-wide move costs only
$1,000), hedge benefit 44.44%,
credit $3,555.555555 per leg (7,111,111,110 micro-USD in total).

## 3. Accounts (`ch_core`)

Seeds are byte strings. Sizes include the 8-byte discriminator (measured:
[`docs/PERFORMANCE.md`](docs/PERFORMANCE.md)).

| Account | Seeds | Bytes | Contents |
| --- | --- | ---: | --- |
| `GlobalConfig` | `config` | 1,599 | `admin`, `proposed_admin`, `risk_authority`, `venue_manager`, `guardian`, `risk_epoch`, `params: ConfigParams`, `used_venues_bitmap`, `active_venues_bitmap`, `global_outstanding_credit`, `venue_outstanding[64]`, `asset_outstanding[32]`, `paused`, `bump`, `venue_accepts[64]` (A1 allowlists) |
| `RiskModel` (zero-copy) | `risk_model` | 3,760 | `version`, `updated_slot`, `corr[496]` (upper triangle, i32 ppm), `assets[32]{shock_bps, gap_bps, basis_bps}`, `corr_stress_ppm` (δ), `basis_decorr_ppm` (β), `scenario_kind[8]`, `scenarios[8][32]` (i16 bps), `oracle_feeds[32][32]` (Pyth feed ids; zero = none), `bump` |
| `PendingChange` | `pending_change` | 894 | at most one queued `RiskChange` (`Params`, `Correlations`, `AssetRisk`, `Stress`, `Scenario`, `OracleFeed`), `proposer`, `proposed_slot`, `eta_slot` |
| `VenueRegistration` | `venue`, `venue_id` | 115 | `venue_id[32]`, `venue_program_id`, `venue_authority`, `venue_index` (bit in bitmaps), `is_active`, `activation_eta_slot` |
| `UserConsent` | `consent`, `user` | 50 | `user`, `is_active`, `authorized_venues_bitmap` |
| `Portfolio` | `portfolio`, `user` | 867 | `status` (0 Empty, 1 Active, 2 Invalidated), `credit_kind` (1 = TradingPowerOnly), `version`, `risk_epoch`, `risk_model_version`, `computed_slot`, `expires_slot`, `total_credit`, `counted`, `counted_epoch`, `sum_standalone_risk`, `portfolio_risk`, `hedge_benefit_ppm`, `flags` (bit 0 `PSD_FALLBACK`, A4; bit 1 `THROTTLED`, B3), `registered[8]{venue_index, asset_id}`, `registered_notional[8]` (A5b), `legs[8]` (inputs as computed, with `baseline_price`, `baseline_conf`, `baseline_publish_ts`, `credit`, `view_mask` (A1), `baseline_ema` (B3)) |
| `PosSnapshot` | `snapshot`, `user`, `venue_id`, `[asset_id]` | 180 | `user`, `venue_id`, `venue_index`, `asset_id`, `version`, `is_long`, `notional`, `initial_margin`, `maintenance_margin`, `slot`, `timestamp`, `rent_payer`, `window_start_slot`, `window_base_notional` (A5c), `mark_price` (A5d) |
| `MockPrice` (`mock-oracle` only) | `mock_price`, `[asset_id]` | 42 | `asset_id`, `price_micro`, `conf_micro`, `publish_ts`, `bump`, `ema_price_micro` (B3) |

`ConfigParams` and its bounds (production build; enforced by `initialize` and every update):

| Field | Bound |
| --- | --- |
| `max_venues` | 1..=64 |
| `haircut_bps` | 500..=10,000; may fall by at most 1,000 per change |
| `credit_ttl_slots` | 1..=3,000 |
| `snapshot_max_age_slots` | 1..=216,000 |
| `max_credit_per_user`, `_per_venue`, `_per_asset`, `global_credit_cap` | each may at most double per change (or rise to $1,000 from below) |
| `max_credit_bps_of_im` | ≤ 9,000 |
| `risk_model_max_age_slots` | 1..=6,480,000 |
| `max_price_age_secs` | 1..=3,600 |
| `max_conf_bps` | 1..=1,000 |
| `max_move_bps` | 1..=2,000 (hard bound); default in scripts and fixtures: 300 |
| `timelock_slots` | 216,000..=1,296,000 (0 allowed only in `mock-oracle` builds) |
| `max_leg_notional` (A5a) | > 0; may at most double per change; lowering applies at once |
| `max_venue_notional_per_user` (A5b) | > 0; may at most double per change; lowering applies at once |
| `max_notional_growth_bps` (A5c) | 0 = no growth limit (loosest); lowering applies at once, raising or disabling waits |
| `notional_growth_window_slots` (A5c) | 1..=216,000; lengthening applies at once |
| `notional_growth_floor` (A5c) | may at most double per change; lowering applies at once |
| `throttle_move_start_bps`, `throttle_move_full_bps` (B3) | 0 = off; otherwise start < full ≤ 2,000; default 50 / 150. Lowering applies at once, raising or turning off waits |
| `throttle_conf_start_bps` (B3) | 0 = off; otherwise < `max_conf_bps`; default 50 |
| `hedge_break_bps` (A3) | 2,500..=10,000; 0 (disabled) allowed only in `mock-oracle` builds; default in scripts: 5,000. Raising applies at once, lowering waits for the timelock |

Risk model bounds: shock 100..=10,000 bps (or 0 = unsupported asset), gap ≥ shock,
basis ≥ 10 bps; a shock may at most halve or double per change; |Δρ| ≤ 0.25 per
change; δ ≥ 0.05; β ≥ 0.01; scenario moves |m| ≤ 10,000 bps.

## 4. Instructions (`ch_core`)

Signers in **bold**. "Bumps epoch" = increments `risk_epoch` and zeroes the
outstanding counters, so every existing credit becomes unusable at once.

### Administration and roles

| Instruction | Who | Effect |
| --- | --- | --- |
| `initialize(params, risk_authority, venue_manager, guardian)` | **admin** (pays) | creates `GlobalConfig`, `RiskModel`, `PendingChange`; sets the roles; validates bounds. A7: production builds (`pyth` + `venue-cpi`) require four distinct, non-default keys (`RolesNotDistinct`) |
| `propose_admin(new)` / `accept_admin()` | **admin** / **proposed admin** | two-step admin transfer; A7: in production the new admin must differ from the three roles |
| `set_roles(risk_authority, venue_manager, guardian)` | **admin** | A7: in production, all four keys distinct |
| `pause()` | **guardian** or **admin** | blocks `refresh_portfolio` only; credit-removing paths keep working |
| `unpause()` | **admin** | |
| `bump_risk_epoch()` | **guardian** or **admin** | emergency: invalidates all credit |
| `set_mock_price(asset, price, conf, ts, ema)` | **admin** | `mock-oracle` builds only; `ema` must be > 0 when read |

### Venues

| Instruction | Who | Effect |
| --- | --- | --- |
| `register_venue(venue_id, program_id, authority, index)` | **venue_manager**, **payer** | `init` only: an id or index can never be re-registered (F8). With `venue-cpi`, `authority` must be `PDA([b"clearinghouse"], program_id)`. Starts inactive with `activation_eta_slot = now + timelock` (active at once if the timelock is 0) |
| `request_venue_activation()` | **venue_manager** | sets the ETA |
| `activate_venue()` | **venue_manager** | after the ETA; bumps epoch |
| `deactivate_venue()` | **venue_manager** or **guardian** | immediate; bumps epoch |
| `set_venue_acceptance(accepts_mask)` (A1) | **venue_authority** (production: the venue program's PDA, by CPI) | sets `venue_accepts[venue_index]` (own bit stored cleared). Removing a venue voids every credit whose `view_mask` contains it at once (no epoch bump); adding one applies at the next refresh. Allowed while paused |

### Risk governance

`update_params`, `update_correlations(entries ≤ 128)`, `set_asset_risk`,
`set_stress_params`, `set_scenario`, `set_oracle_feed`. Signer: **risk_authority**.

Each change is checked against the bounds and against the current value (step
limits). If it can only **reduce** credit (for example a higher haircut, a
lower cap, a disabled asset, a new scenario in an empty slot, or a cleared
oracle feed), it applies immediately. Otherwise it is queued in
`PendingChange` for `timelock_slots`; one change can be pending at a time.
Correlation changes always queue. Applying a change bumps the model version
and the epoch.

| Instruction | Who | Effect |
| --- | --- | --- |
| `apply_pending_change()` | **risk_authority** or **admin** | after the ETA; re-validated against the state at apply time |
| `cancel_pending_change()` | **risk_authority**, **guardian** or **admin** | |

### Users, venues' reports, credit

| Instruction | Who | Effect |
| --- | --- | --- |
| `update_user_consent(is_active, bitmap)` | **user** | creates consent + portfolio on first use; invalidates the portfolio |
| `submit_position_snapshot(venue_id, asset_id, notional, is_long, im, mm, mark_price_micro)` + `risk_model` account; remaining: the asset's oracle when `mark_price_micro > 0` | **venue_authority**, **payer** | requires an active venue, active consent covering the venue, `0 < MM ≤ IM ≤ notional`, `notional > 0`; A5: `notional ≤ max_leg_notional` (`LegNotionalCapExceeded`); this user's Σ notional on this venue ≤ `max_venue_notional_per_user` (`VenueNotionalCapExceeded`); within a window of `notional_growth_window_slots`, `notional ≤ max(base · (1 + g), base + floor)`, base = notional at window start, 0 for a new snapshot (`NotionalGrowthTooFast`); a non-zero mark must be within `max_move_bps` of the oracle (`MarkPriceDeviation`, `MarkPriceOracleMissing`). Registers the leg (≤ 8); invalidates the portfolio in the same instruction; allowed while paused |
| `close_position_snapshot()` | **venue_authority**, or **user** once consent no longer covers the venue or the venue is inactive | unregisters the leg, invalidates the portfolio; rent to `rent_payer` |
| `refresh_portfolio()` + remaining: all registered snapshots, then one oracle account per leg | anyone (fee payer only) | fails unless every registered leg is present once, fresh and consented, the venue is active, the price is fresh and within confidence, the risk model is fresh, and not paused. Releases the old share, computes, applies budgets, counts the new share; status Active; `expires_slot = now + credit_ttl_slots` |
| `invalidate_portfolio()` + optional oracle accounts (one per leg) | anyone | requires a reason: release-only (an invalid portfolio still counted), epoch changed, expired, venue inactive, consent withdrawn, snapshot stale, acceptance revoked (A1: a leg's view contains a venue its venue no longer accepts), or an oracle reading **published after the leg's baseline** that moved more than `max_move_bps` or is stale or too uncertain. A wrong oracle account is an error, not a trip |
| `close_user_accounts()` | **user** | requires zero registered snapshots; releases counters; closes consent + portfolio, rent to the user |

### What a venue reads

`ch_client::usable_credit(portfolio, config.risk_epoch, config.venue_accepts[venue_index], now_slot, venue_index, asset_id)`
returns the leg's credit only if status is Active, the kind is TradingPowerOnly,
the epoch matches, `now_slot ≤ expires_slot`, and (A1) the leg's `view_mask`
is within the venue's current allowlist (own bit implied), otherwise 0. The venue then applies
`ch_client::effective_initial_margin(im, mm, credit) = max(IM − credit, MM)`.
See [`docs/VENUE_INTEGRATION.md`](docs/VENUE_INTEGRATION.md).

## 5. Oracles

| Build | Price account | Checks |
| --- | --- | --- |
| `mock-oracle` | `MockPrice` PDA for the asset | owner = program, PDA matches, asset matches |
| `pyth` | Pyth Receiver `PriceUpdateV2` (owner `rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ`) | discriminator, **Full** verification only, `feed_id == risk_model.oracle_feeds[asset]` (zero = fail closed), price > 0, exponent conversion to micro-USD |

Both builds then apply `check_price`: age ≤ `max_price_age_secs` (and not more
than that far in the future) and `conf / price ≤ max_conf_bps`.

## 6. Build features

`ch_core` refuses to compile unless it has exactly one oracle feature
(`mock-oracle` | `pyth`) and exactly one venue-authentication feature
(`dev-venue-signer` | `venue-cpi`). Production: `pyth,venue-cpi`.

## 7. Errors

Match on error **names** (the numeric codes changed in this branch). The
authoritative list is `programs/ch_core/src/errors.rs` and the IDL `errors`
section.

## 8. Other programs and crates

- `crates/ch_math`: the engine (pure integer math, no Solana dependencies).
- `crates/ch_client`: venue-side helpers `usable_credit`,
  `effective_initial_margin`, `maintenance_requirement`, `withdrawable_equity`.
- `programs/mock_perps_a`, `mock_perps_b`: `report_position` (CPI
  `submit_position_snapshot`) and `set_acceptance` (CPI
  `set_venue_acceptance`, A1), both signed by their `[b"clearinghouse"]` PDA.
  MOCK: not access-controlled.
- `programs/ch_fund`: default fund program.

Status of each (implemented, mock, stub or roadmap): [`docs/STATUS.md`](docs/STATUS.md).
