# Operations

There is **no deployment** of this branch beyond localnet. This document says
what a responsible deployment would require and how to operate it. Items
marked *required before mainnet* are not done.

## 1. Build and deploy

| Step | Command / rule |
| --- | --- |
| Build | `cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features pyth,venue-cpi` (any other feature set is for tests). CI builds exactly this as `target/deploy-prod/ch_core.so`. |
| Verify | Reproducible build (`anchor build --verifiable` or solana-verify), and compare the hash with the CI artifact. *Required before mainnet; not done.* |
| Program id | `declare_id!`, `Anchor.toml` and the IDL must agree (`scripts/ci/check-program-ids.cjs`). The program keypair is not in the repository and never should be. |
| Upgrade authority | A multisig with its own timelock, at least as long as `timelock_slots`. Otherwise the upgrade key bypasses every on-chain bound. *Required before mainnet.* |
| Initialize | `initialize(params)` with production values (bounds in SPEC §3). All roles start as the admin; call `set_roles` immediately. |

## 2. Keys and roles

| Role | Holder | Notes |
| --- | --- | --- |
| admin | multisig (cold) | sets roles, unpauses, transfers admin (two-step) |
| risk_authority | multisig of the risk team | every credit-increasing change waits `timelock_slots` |
| venue_manager | multisig | registration is permanent per id/index; activation waits for the timelock |
| guardian | hot, monitored, low threshold | can only remove credit: pause, deactivate a venue, cancel a queued change, bump the epoch |
| keeper fee payer | hot, low balance | no role at all; top up its balance and alert on it |

## 3. Routine changes (risk authority)

1. Propose (`update_params`, `set_asset_risk`, `update_correlations`, …).
   Credit-reducing changes apply immediately, the rest queue. Check the
   transaction logs for `ChangeQueued` and its `eta_slot`.
2. Announce the queued change. Observers have `timelock_slots` (≥ ~1 day) to
   object, and the guardian can `cancel_pending_change`.
3. After the ETA, `apply_pending_change`. It is re-validated against the state at
   that time, and the epoch bump invalidates all credit; keepers refresh it.
4. Only one change can be pending at a time. Large moves must be staged:
   correlation ±0.25 per change, shocks ×2 or ×½, haircut −10 points, caps ×2.

Adding a venue: `register_venue` (with the venue program's PDA as authority),
wait for the timelock, then `activate_venue`. Adding an asset: `set_asset_risk`
(queued), then `set_oracle_feed` (queued), then correlations (queued, stepped).

## 4. Incidents

| Situation | Immediate action (guardian) | Then |
| --- | --- | --- |
| Suspected bad parameter or model already applied | `bump_risk_epoch` (all credit void) and `pause` (no new credit) | the risk authority proposes a credit-reducing fix (applies at once); admin unpauses |
| Bad change queued | `cancel_pending_change` | investigate the proposer key |
| Venue misreporting or compromised venue program | `deactivate_venue` (immediate, bumps the epoch) | the venue manager decides whether to reactivate (timelocked) |
| Oracle outage or bad prices | none needed: refresh fails closed on stale or wide prices, and credit expires within the TTL | `pause` if prices are fresh but wrong; clear the feed (`set_oracle_feed` with zeros, applies at once) |
| Keeper down | none: credit expires and venues stop applying it | restart; any party can refresh meanwhile |
| Risk key compromised | `cancel_pending_change`; `pause` | admin `set_roles` to a new risk key. Changes already applied stayed within the bounds |

Remember: removing credit is always safe for users (credit ≤ IM − MM, never
collateral), so the guardian should act early.

## 5. Monitoring

| Signal | Source | Alert when |
| --- | --- | --- |
| Keeper heartbeat | keeper `tick` JSON lines | no tick for 3 intervals |
| Keeper errors | `tick.unexpected`, `error` lines | > 0 |
| Stale inputs | `tick.inputsNotReady`, `backedOff` | sustained growth: a venue or oracle has stopped updating |
| Keeper balance | fee payer lamports | below ~2 days of fees ([PERFORMANCE](PERFORMANCE.md)) |
| Queued governance change | `ChangeQueued` events | always: page a human to review before the ETA |
| Epoch bumps, pauses, venue lifecycle, roles | `EmergencyEpochBump`, `PausedUpdated`, `VenueRegistered`, `VenueActiveUpdated`, `RolesUpdated`, `AdminUpdated` | always |
| Outstanding credit | `GlobalConfig.global_outstanding_credit` vs `global_credit_cap`; per venue / asset | > 80% of a cap |
| Refresh failures by reason | failed refresh logs (`SnapshotStale`, `PriceStale`, …) | rate change |

## 6. Keeper

See [`services/keeper/README.md`](../services/keeper/README.md). Running
two instances is safe; it costs duplicate fees, not correctness. Use
`KEEPER_ORACLES` with the Pyth `PriceUpdateV2` account per asset (a sponsored
feed account or a self-posted update). Unknown assets fail closed.
