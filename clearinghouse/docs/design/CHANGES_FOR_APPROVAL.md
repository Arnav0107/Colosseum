# On-chain changes for approval (Sections A–C)

Status: **awaiting owner approval.** Nothing in `programs/` has been changed.
Rules: [`../WORKFLOW.md`](../WORKFLOW.md) §4 (approval gate). Written
2026-10-12 against `e345bc5` on `feat/venue-proof-and-evidence`.

Each item can be approved, edited or rejected on its own; the IDs match the
task list. Section 0 lists the decisions that need your answer. Section 9
gives the combined layout and migration summary.

**Invariants every item preserves** (each is re-tested, see "Tests"):
credit stays `TradingPowerOnly` and is never withdrawable; per-leg credit
≤ `IM − MM`; credit never touches MM or liquidation; no Clearinghouse program
holds user funds; every new check fails closed (an error or zero credit, never
more credit); `mock-oracle` and `dev-venue-signer` stay non-default; the TS
port stays bit-identical (vectors regenerated in the same commit).

**Effect on the worked example** (long SOL / short ETH, $50k each): with A1
(both venues allowlist each other), A2, A3 at 5,000 bps, A4 (ρ = 0.8 is
positive definite) and B3 (price = EMA), credit stays **7,111,111,110**
micro-USD. The arithmetic below explains why; the LiteSVM and vector tests
will confirm it, and if any of them disagrees I will report it, not adjust it.

---

## 0. Decisions needed from you

| # | Question | My recommendation |
| --- | --- | --- |
| Q1 | A1: store each venue's allowlist in `GlobalConfig` (one account, no extra refresh accounts) or in a separate account per venue? | `GlobalConfig` |
| Q2 | A1: when a venue removes another venue from its allowlist, kill only the affected credits (a per-leg "view mask" checked by `usable_credit`) or bump the global epoch (kills every user's credit, so any venue could use it to void all credit)? | per-leg view mask |
| Q3 | A4: when the correlation sub-matrix is not positive definite, the fallback is `R_p = max(L, S)`, which means **zero credit** for that portfolio (as you wrote). OK? | yes |
| Q4 | A5b: "per-venue notional cap". A protocol-wide total per venue needs a counter written by every snapshot, which serializes all of a venue's reports on one account. A per-(user, venue) cap needs no shared write. Which one? | per-(user, venue); protocol-wide only if you accept the throughput limit |
| Q5 | A5c: a report that grows notional faster than allowed is **rejected**, so the venue's trade that triggered it fails. OK, or only flag it? | reject (fail closed); generous defaults |
| Q6 | A5d: the optional mark price changes the `submit_position_snapshot` signature, so every venue's CPI changes. Approve? | yes, while nothing is deployed |
| Q7 | A3: in production builds, should `hedge_break_bps = 0` (disabled) be forbidden? | yes, minimum 2,500 in production |
| Q8 | A7b (optional): in production, also require `admin`, `risk_authority` and `venue_manager` to be off-curve addresses (multisig vault PDAs). | no for now: document it in OPERATIONS instead |
| Q9 | C1: a new program `mock_perps_engine` built twice (two program ids, instances A and B), leaving `mock_perps_a/b` as they are? | yes |

---

## A1. Counterparty allowlist (H3, KNOWN_LIMITATIONS #1)

**Problem.** Credit is computed jointly over all of a user's legs. A venue
that reports a fake offsetting leg creates credit that *other* venues apply.
Caps bound the damage but do not prevent it.

**Design.**

1. Each venue keeps an allowlist `accepts[v]: u64` (bit `j` set: venue `v`
   accepts hedges reported by venue `j`). Its own bit is always implied.
   **Default 0**: a venue accepts only its own reports, so cross-venue credit
   is zero until both sides opt in.
2. Only the venue sets its own list, through a new instruction signed by its
   `venue_authority` (in production, the venue program's PDA by CPI).
3. At refresh, `ch_core` computes:
   - the **joint** credit over all legs, exactly as today (`c_joint_i`);
   - for each venue `v` that holds legs, the **view** credit over only the
     legs of venues that `v` accepts (`c_view_i` for `v`'s own legs). Views
     with identical leg sets are computed once. A view with one leg is 0
     without computing. A view equal to the full set reuses the joint result.
   - final `c_i = min(c_joint_i, c_view_i)`, then the per-user cap over the
     sum, then the per-venue, per-asset and global budgets as today.

   Why both: `c_view_i` makes `c_i` independent of reports from venues `v`
   did not accept, so a non-allowlisted venue **cannot raise** another venue's
   credit. `c_joint_i` keeps today's F3 guarantee that one hedge is never
   used twice (without it, a short on B could back a long on A in A's view and
   a long on C in C's view). A fake leg from a non-accepted venue can only
   *lower* `c_joint_i`, which is safe.
4. Each `LegState` records the view mask used (`view_mask`). Credit is usable
   only while `view_mask ⊆ accepts[v] | (1 << v)`. Removing a venue from the
   allowlist therefore makes every credit that used it unusable **in the same
   instruction**, for every user, without an epoch bump. Adding a venue
   changes nothing until the next refresh.
5. `invalidate_portfolio` gains a reason `ACCEPTANCE_REVOKED` (bit 8), so
   the counters of such a portfolio can be released permissionlessly. Until
   then they over-count, as in KNOWN_LIMITATIONS #10 (conservative).

**Changes.**

| Kind | Change |
| --- | --- |
| Account | `GlobalConfig` + `venue_accepts: [u64; 64]` (+512 bytes) |
| Account | `LegState` + `view_mask: u64` (+8 bytes × 8 legs) |
| Instruction (new) | `set_venue_acceptance(accepts_mask: u64)`; accounts `config` (mut), `venue_registration` (`[b"venue", venue_id]`), `venue_authority` (signer, must equal the registration's). Allowed while paused (it can only lower credit, or raise it at the next refresh, which pause blocks) |
| Instruction | `refresh_portfolio`: semantics above. No new accounts |
| Instruction | `invalidate_portfolio`: new reason bit `ACCEPTANCE_REVOKED = 1 << 8` |
| Venue API | `ch_core::Portfolio::usable_credit` and `ch_client::usable_credit` take the venue's current `accepts` mask; `PortfolioView`/`LegCredit` gain `view_mask`. **Breaking for integrators** (no one is integrated) |
| Event | `VenueAcceptanceUpdated { venue_index, accepts_mask }` |
| Errors | none new (`Unauthorized`, `VenueInactive` reused) |
| Mock venues | `mock_perps_a/b` gain `set_acceptance(mask)` (CPI) so tests use the production path |

**Compute.** Today 8 legs cost 147,668 CU. Two cases cost one engine run
plus a mask comparison: nobody allowlists anyone (every view is one venue's
own legs), and everybody allowlists everybody (every view is the full set).
The worst case is 8 different views of 2+ legs (≈ 8 engine runs) and will
not fit in 200,000 CU. I will measure every case in `perf.rs`. If a case is
over 200k, refresh for that case needs a `SetComputeUnitLimit` instruction
(≤ 1.4M). I will add it to the keeper and testbench and document it in
PERFORMANCE.md. A failed refresh is safe: credit simply stays unusable.

**Tests (LiteSVM, CPI build with real venue programs).**
`malicious_venue_fake_offset_gives_zero_to_non_allowlisting_venue` (venue C
reports a fake short that hedges A's long; A has not allowlisted C → A's
usable credit 0); `allowlisting_restores_credit_after_refresh`;
`unallowlisting_removes_credit_in_the_same_instruction` (usable credit 0
right after `set_venue_acceptance`, before any refresh or epoch change);
`acceptance_revoked_is_a_valid_invalidation_reason_and_releases_counters`;
`only_the_venue_authority_can_set_acceptance`;
`one_hedge_is_not_used_twice_across_views` (F3 regression under views);
`per_user_cap_holds_across_views`. ch_math: property test that
`c_i ≤ c_joint_i` and `c_i` is unchanged when a non-accepted leg is added,
removed or changed (randomized).

**Risks.** Higher refresh CU (above). Venues must now opt in to cross-venue
credit, so the demo and smoke allowlist A↔B. A venue that allowlists a
dishonest venue is exposed to it, as it chose to be.

---

## A2. Credit floor `credit ≤ (ΣIM − R_p)⁺`

**Problem.** KNOWN_LIMITATIONS #19: credit depends on the ratio `R_p / S`,
not the level. Even when `R_p = ΣIM` the worked example still gets $7,111.

**Design.** In `ch_math::portfolio::compute_portfolio_credit`:
`total = min( floor(raw · (1 − haircut)), max_credit_per_user, (ΣIM − R_p)⁺ )`.
Per-leg caps follow as today. `ch_core` uses it unchanged, so this is an
engine change with on-chain effect. The TS port gets the same line; the vectors
are regenerated (`CH_UPDATE_VECTORS=1`); both suites updated in one commit.

**Changes.** No account, instruction or error change. On-chain semantics:
credit can only go down.

**Tests.** Property (random books and models): if `ΣIM ≥ R_p`, then
`Σ max(IM_i − c_i, MM_i) ≥ R_p`; if `R_p ≥ ΣIM`, credit is 0. The existing
"scaling every shock" test changes from "credit stays $7,111.11" to "credit is
0 at 4×", with a comment. `requirement_can_fall_below_worst_loss…` is kept
and its new value asserted. Backtest before/after published in BACKTEST.md
(as measured earlier, it removes 0 of the 1 breach; re-run after the change).

**Risks.** Less credit for books where venue IM is close to model risk. That is the purpose.

---

## A3. Hedge-break scenario on-chain

**Problem.** `HedgeBreakModel` (the ρ-independent backstop) exists only in
`ch_math`.

**Design.** New governed parameter `ConfigParams.hedge_break_bps`. Bounds:
`ch_math::risk::hedge_break_bps_is_valid` (0 or 2,500..=10,000); in
production builds ≥ 2,500 if Q7 = yes. `compute_credit` builds `HedgeBreakModel`
around `StressedRiskModel`. Classification in `params_reduce_credit`:
`new ≥ old` is credit-reducing (applies at once). A lower value, including
disabling it, waits for the timelock. Fixtures, scripts and `services/risk/params`
default to **5,000** (provisional; calibration gives 3,046, so 5,000 is the
more conservative choice).

**Changes.** `ConfigParams` + `hedge_break_bps: u16` (IDL type change for
`initialize`, `update_params`, `RiskChange::Params`). Error: reuses
`InvalidConfigParams`. Keeper: none. Testbench types and fixtures: yes.
Worked example: unchanged at 5,000 (SOL gap $5,000 dominates the $4,500 hedge
break; RISK_MODEL §3a).

**Tests.** `governance.rs` unit tests: bounds, classification in both
directions. LiteSVM: `hedge_break_increase_applies_immediately`,
`hedge_break_decrease_waits_for_timelock`,
`same_asset_basis_trade_credit_matches_engine_with_hedge_break` (≈ $8,000
rather than $10,000, from RISK_MODEL §3a), plus vectors.

---

## A4. Positive-definiteness check on-chain (KNOWN_LIMITATIONS #18)

**Problem.** The program accepts any bounded matrix. In the 20,000-matrix fuzz,
18,154 non-PSD matrices produced a positive, possibly understated, parametric
risk.

**Design.** New pure function in `ch_math` (and the TS port):
`corr_is_positive_definite(assets_present, rho) -> bool`. It runs a
fixed-point LDLᵀ over the **base** correlation sub-matrix of the distinct
assets present (≤ 8), with integer arithmetic only (i128, scale 1e12).
A matrix is accepted only if every pivot is ≥ τ = 1 ppm after a rounding
margin. Consequences: accepted ⇒ positive definite; λ_min ≥ ~1e-6 ⇒
accepted; exactly singular matrices (for example ρ = 1 between two different
assets) are rejected, which errs on the safe side.
Why the base matrix: the direction-aware stress only adds `|x_i x_j|·δ` per
term, so `xᵀρ*x ≥ xᵀρx ≥ 0` when the base matrix is PSD. Checking `ρ*`
would wrongly reject sound books.

If the check fails (and ≥ 2 distinct assets are present), the engine
uses `R_p = max(L, S)` and skips the parametric term. Credit is therefore 0
(Q3). Single-asset books skip the check. The portfolio records a flag
`PSD_FALLBACK` and the refresh event reports it.

**Changes.** No new instruction or error. `Portfolio` + `flags: u8` (bit 0
PSD fallback, bit 1 throttled by B3). `PortfolioRefreshed` event + `flags`.

**Compute.** One LDLᵀ of ≤ 8×8 per refresh (per distinct asset set when A1
views apply; computed once and reused). Measured in `perf.rs` and recorded in
PERFORMANCE.md. Target: refresh at 8 legs ≤ 200k in the non-view cases.

**Tests.** `non_psd.rs` (20,000 matrices): every matrix that the f64 check
calls non-PSD now gives credit 0. Every matrix the integer check accepts has
f64 λ_min ≥ 0 (minus a 1e-9 tolerance). Adversarial near-singular set.
Vectors: one non-PSD case (parity). LiteSVM:
`non_psd_correlation_gives_zero_credit_and_sets_flag` (three assets, pairwise
ρ = −0.9).

**Risks.** Governance can no longer set ρ = ±1 between different assets and
still give credit (use ±0.999). CU (above).

---

## A5. Position-report sanity bounds

**Problem.** Notional, IM and MM are reported unverified.

**Design**, four separately approvable parts. New `ConfigParams` fields; 0
means "off" only where stated. Every check is in `submit_position_snapshot`
and fails the report (fail closed).

| Part | Rule | Params (provisional defaults) | Error |
| --- | --- | --- | --- |
| A5a | `notional ≤ max_leg_notional` | `max_leg_notional` (> 0 required; $10M localnet) | `LegNotionalCapExceeded` |
| A5b | Σ notional of this user's legs on this venue (after the report) ≤ `max_venue_notional_per_user`. Kept exact with a new `Portfolio.registered_notional[8]`, set on report and cleared on close | `max_venue_notional_per_user` ($20M) | `VenueNotionalCapExceeded` |
| A5c | Growth: inside a window of `notional_growth_window_slots`, `new ≤ max(base · (1 + g), base + floor)` where `base` = notional at window start (0 for a new snapshot). The window restarts once it has elapsed | `max_notional_growth_bps` (10,000 = 2× per window; 0 = off), `notional_growth_window_slots` (150, ~1 min), `notional_growth_floor` ($1M) | `NotionalGrowthTooFast` |
| A5d | Optional `mark_price_micro` argument (0 = not reported). If set, `remaining_accounts[0]` must be the asset's oracle (same checks as refresh), and the gap from the oracle price must be ≤ `max_move_bps`. The mark is stored in the snapshot | none (reuses `max_move_bps`) | `MarkPriceDeviation`, `MarkPriceOracleMissing` |

Classification: raising any cap or growth parameter waits for the timelock;
lowering applies at once.

**What these do and do not achieve.** They bound how big and how fast a
fake leg can be. They do not prove a position exists: a new fake leg up to
`max_leg_notional` is still possible, and A1 is the real fix. A5d only
proves the venue used a sane price. It cannot check size, because the
protocol never sees size. This will be written in THREAT_MODEL as defence in depth.

**Changes.** `PosSnapshot` + `window_start_slot: u64`, `window_base_notional: u64`,
`mark_price: u64` (+24 bytes). `Portfolio` + `registered_notional: [u64; 8]`
(+64 bytes). `submit_position_snapshot` gains `mark_price_micro: u64` (A5d,
signature change, Q6) and an optional remaining account. Venue CPI layouts
(mock venues, docs, VENUE_INTEGRATION) updated. Errors appended at the end
of the enum so existing codes keep their numbers.

**Tests.** One LiteSVM rejection test per error, with exact error matching,
plus boundary-accept tests (`== cap` accepted; growth exactly at the limit
accepted; window rollover resets the base; close then re-open counts as
`base = 0`; mark exactly at `max_move_bps` accepted, +1 bps rejected; mark
with the wrong oracle account rejected as `InvalidOracleAccount`).

**Risks.** A5c can fail a legitimate large trade (Q5). Venues must size the
window and floor for their markets. A5d adds one oracle read (~measured) to
reports that use it.

---

## A6. Oracle drift default (already done, no approval needed)

Done in `0910b12`: fixtures and scripts use `max_move_bps = 300`, the hard
bound stays 2,000, and `crates/ch_svm_tests/tests/portfolio.rs` has the 15%
test (`invalidate_portfolio` succeeds with exactly `PRICE_BREACH` using
`default_params()`). Listed here only so the plan is complete. I will re-run it.

---

## A7. Governance hardening

**Problem.** `initialize` makes the admin every role. Nothing stops a
deployment from keeping one key for all four roles.

**Design.** In production builds (`cfg(all(feature = "pyth", feature = "venue-cpi"))`):
- `initialize(params, risk_authority, venue_manager, guardian)`: the four keys
  must be pairwise distinct and non-default (`RolesNotDistinct`). Other builds
  accept any non-default keys, so localnet can still use one key.
- `set_roles`: same rule, including the current admin.
- `accept_admin`: the new admin must differ from the three roles.

Pure helper `governance::roles_distinct` (unit-tested natively in both builds).
OPERATIONS.md gets the multisig requirements and an upgrade-authority custody
runbook (off-chain, done regardless). A7b (Q8): optionally require off-curve
addresses for the three cold roles. Not recommended now.

**Changes.** `initialize` signature (+3 pubkeys, all builds). New error
`RolesNotDistinct`. Scripts, testbench and smoke `initialize` calls updated.

**Tests.** Unit tests for `roles_distinct`. LiteSVM loads the **production
build** (`target/deploy-prod/ch_core.so`, already built by CI) and checks:
`prod_initialize_rejects_shared_role_keys` (each of the 6 pairs);
`prod_set_roles_rejects_shared_keys`; `prod_accept_admin_rejects_role_key`;
the accept paths. The localnet build still accepts a shared key.

---

## B3. Regime-aware credit throttle

**Problem.** Today the only reactions to a fast market are invalidation, at
`max_move_bps` from the baseline, and refresh failing when confidence is wider
than `max_conf_bps`. Credit is either full or zero.

**Design.** At refresh, for each leg compute two ratios from the oracle
reading. Pyth `PriceUpdateV2` carries `ema_price` and is already Full-verified:
- `m` = |price − ema_price| / ema_price (bps, rounded up);
- `c` = conf / price (bps, rounded up).

Per leg, `s = min(lin(m; move_start, move_full), lin(c; conf_start, max_conf_bps))`,
where `lin(x; a, b)` is 1 if `x ≤ a`, 0 if `x ≥ b`, and linear in between
(ppm, floored). The portfolio scale is the minimum over its legs. Every
`c_i` becomes `floor(c_i · scale / 1e6)` before budgets. Pure function
`ch_math::throttle_scale_ppm`, ported to TS, with vectors. Flag bit 1 =
throttled.

| Param (`ConfigParams`) | Bound | Provisional default |
| --- | --- | --- |
| `throttle_move_start_bps` | ≤ `throttle_move_full_bps` | 100 |
| `throttle_move_full_bps` | ≤ 2,000 | 300 |
| `throttle_conf_start_bps` | ≤ `max_conf_bps` | half of `max_conf_bps` |

0 disables a ratio. For classification, 0 counts as +∞ (loosest), so
disabling waits for the timelock. Lowering thresholds applies at once.

Oracle changes: `PriceReading` + `ema_price_micro` (Pyth: parse the existing
`ema_price` field, same exponent; `ema_price ≤ 0` fails closed). Mock:
`MockPrice` + `ema_price_micro` and `set_mock_price` + one argument
(localnet only).

**Backtest.** The backtest has no Pyth EMA or confidence data. It will use
an EMA of 20-minute closes with Pyth's ~1-hour horizon as a stand-in, and
confidence will be **not modelled**. Both points will be labelled in
BACKTEST.md. I will report whether the 2022-11-09 breach goes away at
H = 1 day and at H = 20 minutes. If it does not, I will say so.

**Changes.** `ConfigParams` + 3 × u16. `MockPrice` + 8 bytes (localnet).
`set_mock_price` signature (localnet). No new errors.

**Tests.** ch_math unit and vectors for `lin` and the scale. LiteSVM:
`throttle_scales_credit_linearly_between_thresholds`,
`throttle_zero_at_full_move`, `wide_confidence_throttles`,
`throttle_threshold_lowering_applies_immediately_raising_waits`. Pyth parser
test for `ema_price` against the recorded mainnet bytes.

**Risks.** The EMA is the oracle's own, so credit drops when the market
moves fast. That is intended, but it also means credit is lower in the
volatile periods when hedgers want it most.

---

## C1. Reference venue that consumes credit

**Problem.** `mock_perps_a/b` only report. Nothing shows that a venue can
apply credit safely in a real margin engine.

**Design.** New program `programs/mock_perps_engine` (localnet/testing only,
clearly labelled). Built twice, with features `instance-a` and `instance-b`
that select the `declare_id!` (two fresh ids, no keypairs committed) and
`--sbf-out-dir target/deploy-engine-{a,b}`. Each instance is one venue whose
`venue_authority` is its own `[b"clearinghouse"]` PDA. Pure margin logic
lives in a new crate `crates/perps_engine_math` (no Anchor, unit-tested).

| Account | Seeds | Contents |
| --- | --- | --- |
| `EngineConfig` | `engine` | admin, venue_id, venue_index, collateral mint, vault, `im_bps = 2,000`, `mm_bps = 1,000`, ch_core id |
| vault (SPL token account) | `vault` | user collateral, owned by the engine (the **venue** holds funds; Clearinghouse never does) |
| `MarginAccount` | `account`, user | collateral, ≤ 4 positions `{asset, is_long, size, entry_price}` |

| Instruction | Rule |
| --- | --- |
| `init_engine`, `set_acceptance` (CPI to `set_venue_acceptance`) | admin |
| `deposit(amount)` | SPL transfer into the vault |
| `withdraw(amount)` | equity − amount ≥ Σ IM **without credit** (`withdrawable_equity`) |
| `open_position(asset, is_long, notional)` (new risk) | update the position, then CPI `submit_position_snapshot`, then, if refresh accounts are passed, CPI `refresh_portfolio`. Then read `Portfolio` + `GlobalConfig` with full validation (owner = ch_core, PDA, discriminator, epoch, expiry, status, kind, view mask) and require equity ≥ Σ `effective_initial_margin(IM_i, MM_i, usable_credit_i)`. The credit is for the **resulting** book, because it is recomputed in the same instruction. No refresh accounts means credit 0 |
| `reduce_position(asset, notional)` | risk-reducing; no margin check; CPI snapshot (or close the snapshot at zero) |
| `liquidate(user, asset)` | permissionless when equity < Σ MM; MM only, credit never read; closes the position and its snapshot |

Prices come from ch_core's `MockPrice` accounts (owner and PDA checked), so
localnet only. A production venue would read Pyth. `open_position` with
refresh is above 200k CU, so tests set `SetComputeUnitLimit`; the measured
number goes into PERFORMANCE and ECONOMICS.

**Changes.** New program, new crate, new IDL. ch_core: none beyond A1–A5.
CI: two more SBF builds; program-id check extended.

**Tests (C2, LiteSVM).** (a) `hedged_user_opens_more_size_than_unhedged_with_same_collateral`
(with the measured extra size); (b) `single_leg_user_gets_zero_credit`;
(c) `closing_the_hedge_removes_the_benefit_in_the_same_instruction`;
(d) one test each for expired, wrong epoch, paused (no new credit), venue
deactivated, consent withdrawn and not allowlisted ⇒ zero relief;
(e) `liquidation_threshold_identical_with_and_without_credit`;
(f) `misreporting_venue_cannot_raise_another_venues_allowance` (ties to A1).

**Risks.** It is a reference, not an exchange: no funding, fees, order book
or ADL. The docs will say so.

---

## 9. Combined layout and migration

| Account | Today (bytes) | After (approx., measured after the change) | Cause |
| --- | ---: | ---: | --- |
| `GlobalConfig` | 1,051 | ≈ 1,613 | A1 allowlists (+512), new params A3/A5/B3 (+~50) |
| `Portfolio` | 738 | ≈ 867 | A1 `view_mask` (+64), A5b `registered_notional` (+64), A4/B3 `flags` (+1) |
| `PosSnapshot` | 156 | ≈ 180 | A5c window (+16), A5d mark (+8) |
| `MockPrice` (localnet) | 34 | 42 | B3 EMA |
| `RiskModel`, `PendingChange`, `VenueRegistration`, `UserConsent` | unchanged | unchanged | — |

Instruction signature changes: `initialize` (+3 role keys, and `ConfigParams`
grows), `update_params` (`ConfigParams`), `submit_position_snapshot`
(+`mark_price_micro`), `set_mock_price` (+EMA, localnet), new
`set_venue_acceptance`. New errors (appended): `RolesNotDistinct`,
`LegNotionalCapExceeded`, `VenueNotionalCapExceeded`, `NotionalGrowthTooFast`,
`MarkPriceDeviation`, `MarkPriceOracleMissing`. New invalidation reason
`ACCEPTANCE_REVOKED`. Venue API: `usable_credit` takes the acceptance mask.

**Migration.** Nothing is deployed beyond localnet, so there is no on-chain
state to migrate: localnet is re-initialized. Every changed account would be
incompatible with an old deployment, which MIGRATION.md will state. The
off-chain consumers to update in the same commits are the IDL, the TS types,
the testbench (fixtures, smoke, chain layer), the keeper (account lists, the
compute-unit limit), `services/risk` (params schema, backtest), the scripts,
`tests/ch_core.ts`, and the `ch_svm_tests` setup and helpers.

**Order of implementation** (one commit per item, each with its tests):
A2 → A3 → A4 → A1 → A5 → A7 → B3 → C1/C2, then A8 (stateful test covering
all of them) and A9 (mutations).

## 10. Off-chain work that needs no approval (proceeding now)

A8 test design, A9 mutation list, A10 audit package, B1 frontier sweep, B2
fetch scripts (no network), D1–D4 docs, E4 pitch, CI wiring. Results that
depend on approved on-chain items are filled in only after those items exist
and their tests have run.
