# Pitch vs prototype

One row per claim of the original idea document, checked against the code on
this branch. The idea PDF itself is not in the repository; the claims below
are the ones listed for review. Overall status:
[`STATUS.md`](STATUS.md).

Status meanings:
- **Implemented**: in the code and tested.
- **Mock**: present only as a stand-in for tests or localnet.
- **Roadmap**: not built.
- **Dropped**: removed on purpose.

Test paths are relative to `clearinghouse/`. LiteSVM tests run on the
compiled program: `cargo test --manifest-path crates/ch_svm_tests/Cargo.toml`.

| Claim | Status | Proof | Note |
| --- | --- | --- | --- |
| Venue adapters | **Mock** | `programs/mock_perps_a/src/lib.rs` (`report_position`); `crates/ch_svm_tests/tests/venue_cpi.rs`: `cpi_from_the_registered_venue_program_is_accepted_and_credit_flows`, `registration_requires_the_venue_programs_pda`, `another_program_cannot_report_for_a_venue_it_is_not_registered_as` | The CPI reporting path and the venue-side rules (`crates/ch_client`) are real. The two venues are mocks with no positions, collateral or liquidation. No real perps venue is integrated. |
| Position oracle | **Implemented** (venue-attested) | `programs/ch_core/src/instructions/snapshot.rs`; `crates/ch_svm_tests/tests/portfolio.rs`: `snapshot_validation_rejects_contradictory_inputs`, `snapshot_versions_are_monotonic`, `f2_shrink_to_near_zero_invalidates_in_the_same_instruction` | Positions are what the registered venue program reports by CPI. Nothing independently verifies them. Each venue relies only on the venues it allowlists (A1, `crates/ch_svm_tests/tests/allowlist.rs`; [`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) #1). |
| Netting engine | **Implemented** | `crates/ch_math/src/portfolio.rs`, `crates/ch_math/src/risk.rs`; `crates/ch_math/tests/portfolio_props.rs`: `f3_pairwise_overcredit_is_reproduced_and_joint_engine_removes_it`; `crates/ch_math/tests/risk_stress.rs`: `replay_nov2022_long_sol_short_eth_hedge`; `crates/ch_svm_tests/tests/portfolio.rs`: `hedged_pair_credit_matches_engine_and_is_counted_once`, `f3_three_hedge_venues_are_netted_jointly` | One joint computation per user over all legs, using a stressed model (shocks, correlation stress, scenarios), not margin-as-risk ([`RISK_MODEL.md`](RISK_MODEL.md)). |
| Withdrawable credit | **Dropped** (by design) | `crates/ch_client/src/lib.rs`: `withdrawable_equity`; test `f1_cost2_credit_cannot_prevent_or_cause_liquidation` (asserts `withdrawable_equity(equity, im) == 0` with credit present) | Without moving collateral between venues, withdrawing against a hedge on another venue would let users take out funds that nothing backs, so it was unsafe. |
| Wrong-liquidation fix | **Dropped** (by design) | `crates/ch_client/src/lib.rs` tests `f1_cost2_credit_cannot_prevent_or_cause_liquidation`, `invalidation_never_changes_the_liquidation_threshold` | Without collateral transfer, lowering a venue's liquidation threshold because of a hedge elsewhere leaves that venue under-collateralized when the hedge disappears, so it was unsafe. Credit never touches maintenance margin. |
| Unwind coordinator | **Roadmap** | none (no code) | No cross-venue unwind or liquidation coordination exists. |
| Default fund + waterfall + circuit breaker | **Roadmap** | `programs/ch_fund/src/lib.rs` (stub); decision in [`FUND_DESIGN.md`](FUND_DESIGN.md) | No funded backstop and no loss waterfall. The guardian `pause` and price-move invalidation exist (`crates/ch_svm_tests/tests/portfolio.rs`: `pause_blocks_credit_creation_but_not_removal`, `price_breach_vs_stored_baseline_allows_invalidation`), but they only remove credit; they are not a market circuit breaker. |
| Fee on freed margin | **Roadmap** | none (no code) | No fee is charged or accounted anywhere. |
| Governance voting | **Roadmap** | none for voting; role-based governance in `programs/ch_core/src/governance.rs`, `crates/ch_svm_tests/tests/governance.rs`: `credit_increasing_change_waits_for_the_timelock` | Parameters are set by role keys (meant to be multisigs) within on-chain bounds and a timelock. There is no token or voting mechanism. |
