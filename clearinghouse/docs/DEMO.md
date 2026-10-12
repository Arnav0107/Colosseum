# Demo

> **Output status: not yet captured.** `npm run demo` has not completed
> anywhere yet. On the Windows development host it builds and then fails at
> validator start (output in §4). The CI job `demo` runs the same command and uploads its full output
> as the `demo-output` artifact. Paste that output into §4–§6 once it exists
> ([`WORKFLOW.md`](WORKFLOW.md) §2: no output here that was not produced by a
> run).

## 1. What the demo shows

A user is long SOL on venue A and short ETH on venue B, $50,000 each, with IM
$10,000 and MM $5,000 per leg. Clearinghouse computes one joint, stressed
risk number for both legs and publishes trading-power credit. Each venue can
then lower its **initial** margin to `max(IM − credit, MM)` for new
risk-increasing orders. When the hedge changes (here: a price move beyond
the guard), anyone can invalidate the credit, and each venue's requirement
goes back to IM. The maintenance margin, and so the liquidation threshold,
is the same in every column.

## 2. Prerequisites

| Tool | Version used by CI ([`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)) |
| --- | --- |
| Rust | 1.99.0 |
| Agave (`cargo-build-sbf`, `solana-test-validator`, `solana`) | v4.3.0 |
| Node / npm | 20.19.0 |

Build features: `mock-oracle,dev-venue-signer` (**localnet only**: admin-set
prices, keypair venue signer). Linux or macOS. On the Windows development host
`solana-test-validator` cannot run ([`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) #17).

## 3. Run it

```bash
cd clearinghouse
npm run demo
```

What `scripts/demo.sh` does, in order:

1. Checks the tools above and that nothing is already serving
   `127.0.0.1:8899`.
2. `cargo build-sbf` of `ch_core` with `mock-oracle,dev-venue-signer`.
3. Starts `solana-test-validator --reset` with `ch_core` **loaded at its
   `declare_id!` address** (`--bpf-program`). This is the deploy step. The
   program keypair is not in the repository, so `anchor deploy` would place
   it at a different address.
4. `npm --prefix testbench run smoke`: the same chain code the testbench UI
   uses. It initialises config, registers venues A and B, sets the risk
   parameters and correlation (ρ = 0.8), gives consent, sets mock prices,
   submits both snapshots and refreshes. It then checks the result with an
   independent recomputation, invalidates on a 6% price move, and prints the
   before/after table. The validator is stopped on exit.

Every check is an assertion; the first failure stops the run with a non-zero
exit code.

## 4. Scenario: hedged user gets trading-power credit

Runs so far, both on the Windows development host on 2026-10-12. Neither
reached the smoke.

Without the toolchain on `PATH`:

```
> bash scripts/demo.sh

demo: missing tool: cargo
```

With the toolchain (`source "$LOCALAPPDATA/xwin-bin/sbf-env.sh"`; Compiling
lines omitted). The build succeeds, and the validator cannot start on Windows
([`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) #17):

```
> clearinghouse@0.1.0 demo
> bash scripts/demo.sh

=== 1/4 build ch_core (SBF, mock-oracle + dev-venue-signer: LOCALNET ONLY) ===
[2026-10-11T23:05:48.791528900Z WARN  cargo_build_sbf] Package 'ch_core' has two crate types defined: cdylib and lib. This setting precludes link-time optimizations (LTO). Use cdylib for programs to be deployed and rlib for packages to be imported by other programs as libraries.
=== 2/4 start solana-test-validator with ch_core at 2XcTzg5kuZBxaY7FXGHHDbVCtsUxeoMtgJnNpXevsBbg ===

thread 'main' (31188) panicked at validator\src\bin\solana-test-validator.rs:130:86:
called `Result::unwrap()` on an `Err` value: Os { code: 1314, kind: Uncategorized, message: "A required privilege is not held by the client." }
stack backtrace:
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
Error: error sending request for url (http://127.0.0.1:8899/)
demo: validator did not start
```

_Pending: the smoke output from a run with the tools installed, including
`total credit …` (the smoke prints it next to the worked-example value
7,111,111,110 that the LiteSVM suite pins) and `independent verification:
match`._

## 5. Scenario: the hedge changes and credit is invalidated

_Pending: smoke lines "invalidation without a reason is rejected", "price
breach newer than the baseline invalidates", "invalidation released the
credit", "venues see no usable credit after invalidation"._

## 6. Before / after table

The smoke prints this table after the invalidation step. Each row is one leg
(and a total). The columns are:

| Column | Meaning |
| --- | --- |
| IM, no credit (before) | the venue's own initial margin |
| usable credit | `usable_credit` read from chain while the portfolio is Active (`testbench/src/model/verify.ts`, the `ch_client` rule) |
| IM with credit (after) | `max(IM − credit, MM)` |
| IM after hedge change | the same rule after invalidation (usable credit 0, so IM) |
| MM (all columns) | maintenance margin: unchanged by credit and by its loss |

_Pending: the table as printed by `npm run demo`._

## 6a. Before / after at a venue that applies the credit (C2, C3)

This part does not need a validator. It runs the compiled programs (ch_core
with production venue authentication, two instances of the reference venue
`programs/mock_perps_engine`) in LiteSVM:

```bash
cd clearinghouse
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,venue-cpi --sbf-out-dir target/deploy-cpi
cargo build-sbf --manifest-path programs/mock_perps_a/Cargo.toml
cargo build-sbf --manifest-path programs/mock_perps_b/Cargo.toml
cargo build-sbf --manifest-path programs/mock_perps_engine/Cargo.toml --features instance-a --sbf-out-dir target/deploy-engine-a
cargo build-sbf --manifest-path programs/mock_perps_engine/Cargo.toml --features instance-b --sbf-out-dir target/deploy-engine-b
cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test engine -- --nocapture --test-threads=1
```

Output of that run on the Windows development host, 2026-10-12 (lines
filtered with `grep -E "C2|test "`; the hedged test prints its `ok` on a
line of its own after the two `C2(a)` lines, so the filter drops it, and the
result line shows 6 passed):

```
test closing_the_hedge_removes_the_benefit_in_the_same_instruction ... ok
test hedged_user_opens_more_size_than_unhedged_with_same_collateral ... C2(a): open_position with in-instruction snapshot + refresh (2 legs): 132563 CU
C2(a): collateral $8,000 on venue A. max SOL long: unhedged $40000, hedged $59000 (+$19000 enabled by credit)
test liquidation_threshold_identical_with_and_without_credit ... ok
test misreporting_venue_cannot_raise_another_venues_allowance ... ok
test no_relief_when_credit_is_not_usable ... ok
test single_leg_user_gets_zero_credit ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.05s
```

Read as before/after, with the same $8,000 collateral on venue A (IM 20%,
MM 10%, SOL at $150):

| | Unhedged trader | Trader also short $50,000 ETH on venue B |
| --- | --- | --- |
| Largest SOL long venue A accepts | $40,000 | $59,000 |
| Why | IM 20% × $40,000 = $8,000 | venue A requires max(IM − usable credit, MM) |
| Liquidation price of a $30,000 long with $6,000 | below $133.33 | the same (test `liquidation_threshold_identical_with_and_without_credit`) |
| After the ETH short is closed | — | relief gone in the same instruction (test `closing_the_hedge_removes_the_benefit_in_the_same_instruction`) |

The testbench shows the same comparison in the panel "What the credit buys
at a venue" (`testbench/src/components/VenueRulePanel.tsx`). It computes it
in the browser with `testbench/src/model/venueRule.ts`, a TypeScript mirror
of the venue rule that uses the bit-identical engine port; the test
`testbench/src/model/venueRule.test.ts` checks it gives $40,000 and $59,000
too.

## 7. What the demo does not show

- No real venue applies the credit. The reference venue in §6a is a
  minimal margin engine for tests (no order book, funding or fees), not a
  production venue ([`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) #11).
- Mock oracle and keypair venue signer. The production configuration
  (`pyth,venue-cpi`) is built in CI but not run here.
- No loss coverage: `ch_fund` is a stub ([`FUND_DESIGN.md`](FUND_DESIGN.md)).
- Whether the credit is *safe*: see the backtest, which found a breach at a
  1-day horizon ([`BACKTEST.md`](BACKTEST.md)).
- Claim by claim: [`PITCH_VS_PROTOTYPE.md`](PITCH_VS_PROTOTYPE.md).
