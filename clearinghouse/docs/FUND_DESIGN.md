# Default fund (`ch_fund`): decision and design

Status: **not implemented**, deliberately. `programs/ch_fund` is a
labelled stub with no instructions beyond a no-op `initialize`. **Nothing in
Clearinghouse covers losses.** See [`STATUS.md`](STATUS.md).

## Decision

A funded backstop is worth building only if every one of the following can be
enforced on-chain and tested end to end:

| Requirement | Why it is not met today |
| --- | --- |
| Contributions (who funds it, in what asset, lock-up) | No contributor model, no token custody design. |
| Eligible losses (what counts as a loss caused by honouring a credit) | A venue's loss on a liquidated account is a venue-internal fact. There is no on-chain, verifiable "loss attributable to Clearinghouse credit" today. |
| Claim verification | Claims would be venue self-reports. Without venue CPI integration (Stage D, still open) nothing binds a claim to an actual liquidation. |
| Per-event and aggregate payout limits | Definable, but meaningless without the two rows above. |
| Replay protection | Needs a claim id bound to a specific liquidation event. |
| Reservations against outstanding credit | The global outstanding-credit counter exists (below); a reservation model on top of it does not. |
| Insolvency behaviour (pro-rata haircuts, ordering, what venues are told) | Not designed. |

So implementing it now would produce something that *looks* like loss
coverage and is not. The economic design instead removes the need for loss
coverage: credit is **initial-margin relief only**, capped at `IM − MM` per
leg, so every venue keeps at least its own maintenance margin and its
liquidation engine is untouched ([`RISK_MODEL.md`](RISK_MODEL.md) §1).

## What *is* enforced without a fund

- **Global outstanding-credit cap** (`global_credit_cap`), plus per-venue and
  per-asset caps, against counters in `GlobalConfig` that are updated on every
  refresh and released exactly once on invalidation. Tested in
  `crates/ch_svm_tests/tests/portfolio.rs`.
- `ch_math::fund_cap()` remains as arithmetic only. It is not called
  on-chain and is not evidence of solvency.

## If a fund is built later

Minimum design, in order:

1. Venue CPI integration (so claims come from the venue program, not a key).
2. On-chain liquidation receipts from integrated venues: account, leg,
   shortfall, slot, the portfolio version and credit in force at that slot.
3. Eligibility rule: shortfall only where `usable_credit > 0` at the
   liquidation slot, capped at that credit.
4. Vault + contributor shares; payouts ≤ vault balance; per-event cap;
   aggregate cap per epoch; claim id = hash(receipt) stored to prevent replay.
5. `global_credit_cap ≤ k · vault balance` enforced at refresh time.
6. Insolvency: pro-rata payout across claims in an epoch; credits suspended
   (guardian epoch bump) while the vault is below its floor.
7. Tests: payouts ≤ reserves; no double claim; claims without a receipt
   rejected; insolvency path.
