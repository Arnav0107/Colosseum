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
R_p  = max(P, L)
C    = floor( ΣIM · (S − R_p)⁺ / S ) · (10_000 − haircut_bps) / 10_000, then ≤ max_credit_per_user,
       then ≤ (ΣIM − R_p)⁺                                (floor, A2)
c_i  = min( floor(C · IM_i / ΣIM), IM_i · max_credit_bps_of_im / 10_000, IM_i − MM_i )
```

Then protocol budgets: per-venue, per-asset and global outstanding-credit caps
scale the `c_i` down pro rata (floored), in that order.

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
| `GlobalConfig` | `config` | 1,051 | `admin`, `proposed_admin`, `risk_authority`, `venue_manager`, `guardian`, `risk_epoch`, `params: ConfigParams`, `used_venues_bitmap`, `active_venues_bitmap`, `global_outstanding_credit`, `venue_outstanding[64]`, `asset_outstanding[32]`, `paused`, `bump` |
| `RiskModel` (zero-copy) | `risk_model` | 3,760 | `version`, `updated_slot`, `corr[496]` (upper triangle, i32 ppm), `assets[32]{shock_bps, gap_bps, basis_bps}`, `corr_stress_ppm` (δ), `basis_decorr_ppm` (β), `scenario_kind[8]`, `scenarios[8][32]` (i16 bps), `oracle_feeds[32][32]` (Pyth feed ids; zero = none), `bump` |
| `PendingChange` | `pending_change` | 894 | at most one queued `RiskChange` (`Params`, `Correlations`, `AssetRisk`, `Stress`, `Scenario`, `OracleFeed`), `proposer`, `proposed_slot`, `eta_slot` |
| `VenueRegistration` | `venue`, `venue_id` | 115 | `venue_id[32]`, `venue_program_id`, `venue_authority`, `venue_index` (bit in bitmaps), `is_active`, `activation_eta_slot` |
| `UserConsent` | `consent`, `user` | 50 | `user`, `is_active`, `authorized_venues_bitmap` |
| `Portfolio` | `portfolio`, `user` | 738 | `status` (0 Empty, 1 Active, 2 Invalidated), `credit_kind` (1 = TradingPowerOnly), `version`, `risk_epoch`, `risk_model_version`, `computed_slot`, `expires_slot`, `total_credit`, `counted`, `counted_epoch`, `sum_standalone_risk`, `portfolio_risk`, `hedge_benefit_ppm`, `registered[8]{venue_index, asset_id}`, `legs[8]` (inputs as computed, with `baseline_price`, `baseline_conf`, `baseline_publish_ts`, `credit`) |
| `PosSnapshot` | `snapshot`, `user`, `venue_id`, `[asset_id]` | 156 | `user`, `venue_id`, `venue_index`, `asset_id`, `version`, `is_long`, `notional`, `initial_margin`, `maintenance_margin`, `slot`, `timestamp`, `rent_payer` |
| `MockPrice` (`mock-oracle` only) | `mock_price`, `[asset_id]` | 34 | `asset_id`, `price_micro`, `conf_micro`, `publish_ts` |

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
| `initialize(params)` | **admin** (pays) | creates `GlobalConfig`, `RiskModel`, `PendingChange`; all roles = admin; validates bounds |
| `propose_admin(new)` / `accept_admin()` | **admin** / **proposed admin** | two-step admin transfer |
| `set_roles(risk_authority, venue_manager, guardian)` | **admin** | |
| `pause()` | **guardian** or **admin** | blocks `refresh_portfolio` only; credit-removing paths keep working |
| `unpause()` | **admin** | |
| `bump_risk_epoch()` | **guardian** or **admin** | emergency: invalidates all credit |
| `set_mock_price(asset, price, conf, ts)` | **admin** | `mock-oracle` builds only |

### Venues

| Instruction | Who | Effect |
| --- | --- | --- |
| `register_venue(venue_id, program_id, authority, index)` | **venue_manager**, **payer** | `init` only: an id or index can never be re-registered (F8). With `venue-cpi`, `authority` must be `PDA([b"clearinghouse"], program_id)`. Starts inactive with `activation_eta_slot = now + timelock` (active at once if the timelock is 0) |
| `request_venue_activation()` | **venue_manager** | sets the ETA |
| `activate_venue()` | **venue_manager** | after the ETA; bumps epoch |
| `deactivate_venue()` | **venue_manager** or **guardian** | immediate; bumps epoch |

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
| `submit_position_snapshot(venue_id, asset_id, notional, is_long, im, mm)` | **venue_authority**, **payer** | requires an active venue, active consent covering the venue, `0 < MM ≤ IM ≤ notional`, `notional > 0`; registers the leg (≤ 8); invalidates the portfolio in the same instruction; allowed while paused |
| `close_position_snapshot()` | **venue_authority**, or **user** once consent no longer covers the venue or the venue is inactive | unregisters the leg, invalidates the portfolio; rent to `rent_payer` |
| `refresh_portfolio()` + remaining: all registered snapshots, then one oracle account per leg | anyone (fee payer only) | fails unless every registered leg is present once, fresh and consented, the venue is active, the price is fresh and within confidence, the risk model is fresh, and not paused. Releases the old share, computes, applies budgets, counts the new share; status Active; `expires_slot = now + credit_ttl_slots` |
| `invalidate_portfolio()` + optional oracle accounts (one per leg) | anyone | requires a reason: release-only (an invalid portfolio still counted), epoch changed, expired, venue inactive, consent withdrawn, snapshot stale, or an oracle reading **published after the leg's baseline** that moved more than `max_move_bps` or is stale or too uncertain. A wrong oracle account is an error, not a trip |
| `close_user_accounts()` | **user** | requires zero registered snapshots; releases counters; closes consent + portfolio, rent to the user |

### What a venue reads

`ch_client::usable_credit(portfolio, config.risk_epoch, now_slot, venue_index, asset_id)`
returns the leg's credit only if status is Active, the kind is TradingPowerOnly,
the epoch matches and `now_slot ≤ expires_slot`, otherwise 0. The venue then applies
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
- `programs/mock_perps_a`, `mock_perps_b`: one `report_position` instruction
  that CPIs `submit_position_snapshot` signed by their `[b"clearinghouse"]` PDA.
- `programs/ch_fund`: default fund program.

Status of each (implemented, mock, stub or roadmap): [`docs/STATUS.md`](docs/STATUS.md).
