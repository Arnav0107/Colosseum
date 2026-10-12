# A8 stateful invariant test and A9 mutation checks (design)

Status: **A8 implemented** (`crates/ch_svm_tests/tests/stateful.rs`); A9 below. Implementation follows the approved on-chain items in
[`CHANGES_FOR_APPROVAL.md`](CHANGES_FOR_APPROVAL.md), because several
invariants refer to them (allowlist, floor, hedge break, PSD fallback,
notional caps). The parts that exist today are implemented first and run
against the current program.

## A8. Seeded random-sequence test (`crates/ch_svm_tests/tests/stateful.rs`)

**Harness.** LiteSVM on the compiled CPI build (`mock-oracle,venue-cpi`), so
reports go through the real venue programs. World: 4 venues (indices 0–3),
3 users, 3 assets (SOL, ETH, BTC), mock prices. RNG: a small local xorshift64*
seeded from `u64` (no new dependency). Each step draws one action from the
table below with random arguments. **Invalid** variants are drawn
deliberately, for example a report from the wrong program, a refresh missing
a snapshot, an unjustified invalidation, a step-limit violation or an
over-cap notional. Every step's result is checked against a model of what
the program should do: an action that the model says must fail has to fail
with the exact error, and one that must succeed has to succeed.

| Action | Valid form | Invalid forms drawn |
| --- | --- | --- |
| submit snapshot (via venue CPI) | consented, active venue, `0 < MM ≤ IM ≤ N` | non-consented user, inactive venue, bad margins, over caps (A5), 9th leg |
| close snapshot | venue authority; user after revoking | user while still consented |
| consent change | random bitmap / inactive | — |
| refresh | all registered snapshots + oracles | missing / duplicate snapshot, wrong oracle, paused, stale price |
| invalidate | with a true reason | no reason (`InvalidationNotJustified`) |
| pause / unpause | guardian / admin | unpause by guardian |
| bump epoch | guardian | non-guardian |
| deactivate / activate venue | venue manager after the timelock | before the timelock |
| governance change | within bounds and step limits | out of bounds, too large a step |
| set venue acceptance (A1) | venue program by CPI | another program |
| warp clock | +1..4,000 slots (crosses TTL and staleness) | — |
| move price | ±0–15% | — |

**Invariants checked after every step**, by reading all accounts:

1. `global_outstanding_credit` = Σ `total_credit` over portfolios with
   `counted ∧ counted_epoch = risk_epoch`. The same holds per venue
   (`venue_outstanding[v]`) and per asset.
2. Every leg: `credit ≤ IM − MM` and `credit ≤ IM · max_credit_bps_of_im / 10,000`.
3. `usable_credit = 0` for every (user, venue, asset) whenever status ≠ Active,
   the epoch differs, `now > expires_slot`, the venue is inactive, or (A1) the
   leg's `view_mask ⊄` the venue's current allowlist.
4. (A1) No usable credit at venue `v` that a recomputation **without** the
   legs of venues `v` does not accept would not also give (checked with
   `ch_math` natively).
5. (A2) For every Active portfolio, `Σ credit ≤ (ΣIM − R_p)⁺`, with `R_p`
   recomputed natively.
6. No counter underflow: an action never fails with `CounterUnderflow`, and
   every counter stays ≤ its cap.
7. Totals: `Σ credit ≤ max_credit_per_user` per portfolio; per-venue,
   per-asset and global counters ≤ caps.
8. Credit kind is always `TradingPowerOnly`.

**As built.** World of 4 venues, 3 users, assets SOL/ETH/BTC on the localnet
build (dev-venue-signer; the CPI path is covered by `venue_cpi.rs`). The
actions are snapshot submit/close, consent changes, refresh, invalidate,
pause/unpause, epoch bump (and an invalid bump by a user), venue
deactivation/activation, haircut changes, allowlist changes (and an invalid
change by another venue's authority), warps and ±15% price moves. A refresh
that runs out of the default 200k CU is retried with a 1.4M ComputeBudget
limit, as the keeper does (A1 partial views). Invariants 1–3 and 5–8 are
checked as listed, plus the A2 floor. Invariant 4 is checked as "usable credit
only from allowlisted, active venues"; the native recomputation lives in
`ch_math/tests/view_props.rs`. Run 2026-10-12: 32 seeds × 300 steps, every
invariant held, 4,759 successful actions, 1,899 steps with usable credit
(the test requires more than 1,500, so the invariants are not vacuous); 291 s.

**Seeds and failure output.** A fixed list of 32 seeds × 300 steps runs in
CI. `CH_STATEFUL_SEEDS=…` and `CH_STATEFUL_STEPS=…` override them locally.
On failure the message contains the seed, the step number, the action
history of the last 20 steps and the violated invariant, plus the command
that reproduces it:
`CH_STATEFUL_SEEDS=<seed> cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test stateful`.

## A9. Mutation checks

Each mutation is a one-line source change, applied by hand (or by
`scripts/ci/mutate.sh <id>` once written), rebuilt and tested. A mutation
passes the check ("killed") only if at least one test fails. The results go
into `docs/TESTING_AND_CI.md` "Evidence that tests can fail", with the
failing test names. Existing 4 are listed there already.

| # | Mutation | Expected to fail |
| --- | --- | --- |
| M5 | A1: refresh ignores the allowlist (`view = all legs`) | `malicious_venue_fake_offset_gives_zero_to_non_allowlisting_venue`, stateful inv. 4 |
| M6 | A1: `usable_credit` skips the `view_mask` check | `unallowlisting_removes_credit_in_the_same_instruction` |
| M7 | A1: final credit uses `c_view` only (no `min` with joint) | `one_hedge_is_not_used_twice_across_views` |
| M8 | A2: floor removed | floor property test, vectors, stateful inv. 5 |
| M9 | A3: `compute_credit` builds `StressedRiskModel` (hedge break ignored) | `same_asset_basis_trade_credit_matches_engine_with_hedge_break`, vectors |
| M10 | A3: lowering `hedge_break_bps` classified credit-reducing | `hedge_break_decrease_waits_for_timelock` |
| M11 | A4: PSD check always returns true | `non_psd_correlation_gives_zero_credit_and_sets_flag`, `non_psd.rs` |
| M12 | A5a: leg notional cap check removed | `leg_notional_cap_rejects` |
| M13 | A5b: per-(user, venue) cap uses the new leg only | `venue_notional_cap_rejects_sum` |
| M14 | A5c: growth window never resets / never checked | growth tests |
| M15 | A7: distinct-roles check removed from `set_roles` | `prod_set_roles_rejects_shared_keys` |
| M16 | B3: throttle scale ignored | `throttle_scales_credit_linearly_between_thresholds` |
| M17 | `release()` skipped on refresh (counter drift) | stateful inv. 1 |
