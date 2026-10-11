# Upgrade report: clearinghouse hardening

Branch `feat/clearinghouse-hardening`, from `origin/main` at `7c5f4c4`.
Reviewed findings F1–F14; working log with evidence: [`UPGRADE_LOG.md`](UPGRADE_LOG.md).
Current status (canonical): [`STATUS.md`](STATUS.md). Date: 2026-10-11.

**Bottom line:** every finding has a root-cause fix with a test that runs on the
compiled program, except two pieces of evidence that could not run on this
host: the validator-based integration tests and CI. The project moves from
**4/10 to 6/10**. It does **not** reach 9/10. The blockers are external and
design items (section E), not unfinished code.

## A. What changed

| Stage | Commits | Change |
| --- | --- | --- |
| A: evidence and hygiene | `bce171e` `1afd67e` `db5d6a4` `f20ecb4` `de65208` `6626454` `886d123` | `expectTxError` helper, and all 33 negative tests converted (12 could never fail); F8 registry fix with a LiteSVM harness on the compiled program; F12 verification against on-chain config; no default oracle; CI pinned to a toolchain that can build the program, with the program actually loaded; single required gate |
| B: portfolio | `53072fa` `f66f1a4` `01c5b54` | joint per-user `Portfolio`, inputs bound by version, atomic invalidation, risk epoch, stored baseline, exactly-once counters, per-user/venue/asset/global caps, permissionless refresh and invalidation |
| C: risk | `06c51f7` `8d95ac4` `c158089` `79da2e7` | stressed risk model (notional × shock, direction-aware correlation stress, market-wide/gap/basis/governance scenarios); credit ≤ IM − MM; TradingPowerOnly; stress suite including a Nov-2022 replay from primary data; Rust/TS parity vectors; fund decision (not built) |
| D: governance, auth, oracle | `288782c` `b402281` | roles (admin, risk authority, venue manager, guardian), bounds, step limits, timelock with credit-reducing fast path; CPI-only venue authentication; Pyth `PriceUpdateV2` parser pinned to real mainnet bytes; oracle evidence must postdate the baseline; account closure and reinit review |
| E: tooling, docs | `fdc36af` `9a051df` `f9eaf02` `d318bcf` `e6e9675` `5769324` | testbench rebuilt on the new API (on-chain inputs only, independent verification, a11y, 390 px, loopback-only); minimal idempotent keeper; measured performance; reviewed npm-audit gate; STATUS/SPEC/architecture/threat model/operations/venue integration/limitations |
| F: verification | `b9e9bec` + this report | full local gate; Rust and npm audits; re-review; audit gate made non-mutating |

Breaking changes for integrators: [`MIGRATION.md`](MIGRATION.md).

## B. Test results

### Passed (run locally on 2026-10-11, all in one gate run from clean SBF builds)

| Check | Result |
| --- | --- |
| `cargo fmt --check` (program workspace, LiteSVM workspace) | pass |
| `cargo clippy -D warnings`: workspace (`mock-oracle,dev-venue-signer`) and ch_core with `pyth,venue-cpi`, `mock-oracle,venue-cpi`, `pyth,dev-venue-signer`; LiteSVM crate | pass |
| ch_core fails to compile for 5 invalid feature sets | pass (all 5 fail as required) |
| Rust workspace tests | **62 passed, 0 failed** |
| ch_core unit tests with `pyth,venue-cpi` (parser vs real mainnet `PriceUpdateV2`) | **13 passed** |
| SBF builds: localnet, CPI, Pyth, **production (`pyth,venue-cpi`)**, two mock venues | pass |
| LiteSVM on the compiled programs: portfolio 28, governance 9, venue registry 8, risk model 6, venue CPI 5, Pyth 5, account lifecycle 4, perf 1 | **66 passed, 0 failed** |
| IDL and TS type regenerated = committed; program ids consistent | pass |
| Root `tsc` (integration tests type-check) | pass |
| Testbench: vitest | **46 passed** (15 parity vectors, 13 verification/rules, 11 render/a11y, 7 legacy math) |
| Testbench `tsc`, production build | pass (bundle-size and polyfill `eval` warnings, recorded, not suppressed) |
| Keeper tests (including two instances in parallel) | **8 passed** |
| `cargo audit`, program `Cargo.lock` | **0 vulnerabilities**; 1 unmaintained warning (`bincode` 1.3.3, via Solana crates) |
| `cargo audit`, LiteSVM test `Cargo.lock` | **0 vulnerabilities**; 5 unmaintained warnings (test-only dependencies) |
| npm production audit (testbench, keeper, root) | 6 advisories, **all reviewed as unreachable** and in the register; 0 unreviewed |
| actionlint on the workflow | pass |
| 390 px layout (fixture state, headless Chrome, 390 px iframe) | page `scrollWidth == clientWidth`, no element past the viewport |
| Mutation and negative-control checks | 4 program mutants killed; F8 tests fail against the v0 binary; a11y mutant fails 4 tests; audit gate exits 1 on an unreviewed advisory |

### Failed

None of the final runs failed. Failures found and fixed on the way are in
the commit messages and `UPGRADE_LOG.md`. Examples:
- a non-compiling intermediate commit (`06c51f7`, fixed in `8d95ac4`);
- a model flaw that gave an all-long book credit;
- an oracle-evidence replay hole;
- a keeper fee loop on stale inputs;
- a page overflow at 390 px;
- an ignored audit register;
- an audit gate that rewrote `yarn.lock`.

### Blocked (could not run on this host)

| Check | Why | Where it is wired |
| --- | --- | --- |
| `anchor test` (`tests/ch_core.ts`, 22 cases, rewritten for the portfolio API with exact-error negative tests) | `solana-test-validator` needs symlink privilege (Windows os error 1314) | CI `anchor-integration` |
| Testbench smoke on a validator | same | CI `anchor-integration` |
| Live UI against a validator | same | manual (`scripts/dev-localnet.sh`) |
| Yanked-crate check in `cargo audit` | crates.io index update failed (no git helper on this host) | CI `rustsec/audit-check` |

### Not run

| Check | Reason |
| --- | --- |
| GitHub Actions on this branch | not pushed (see H) |
| Pyth against the live receiver program on a cluster | no devnet or mainnet actions allowed; recorded mainnet bytes used instead |
| Keeper adapter against a validator | no validator on this host |
| External audit | out of scope |

## C. Findings F1–F14: closure

"Closed" = root cause removed, regression test passes locally on the compiled
program or the pure engine. "Closed, execution blocked" = fixed and test
written, but the test needs a validator.

| ID | Finding (short) | Root cause | Fix | Evidence | Status | Residual |
| --- | --- | --- | --- | --- | --- | --- |
| F1 | Credit presented as collateral; a hedged user stays liquidatable; fund is a stub | credit from IM only, unbounded by MM or any resource | trading-power-only credit capped at IM − MM per leg; venue applies `max(IM − credit, MM)`; stressed risk model; global/venue/asset caps; fund decision documented | `ch_client` tests (F1 cost 2); engine caps; stress suite; LiteSVM caps | **Closed** (by redesign) | no loss coverage (KNOWN_LIMITATIONS #2) |
| F2 | Credit survives input changes | credit not bound to inputs | portfolio versions, atomic invalidation in every write, risk epoch | LiteSVM invalidation tests; 2 mutants killed | **Closed** | — |
| F3 | Pairwise over-credit (+23.7/+59.0/+133.5%) | each pair netted separately | one joint computation per user | `portfolio_props.rs` regression; LiteSVM | **Closed** | — |
| F4 | Seeds omit asset; 8×8 matrix | seeds and sizes hard-coded | seeds include asset; 32 assets; zero-copy model | LiteSVM; account sizes | **Closed** | `MAX_LEGS = 8` (bounded, measured) |
| F5 | Venue can reset the guard baseline | baseline on the mutable snapshot | baseline price, confidence and publish time in the portfolio; evidence must postdate it | `f5_…`, `oracle_evidence_…` (mutant killed), `pyth_oracle.rs` | **Closed** | refresher picks the baseline within the age window |
| F6 | Any single-leg move revokes, binary | per-leg guard | revocation is non-liquidating by construction; the guard is a recompute trigger | engine and `ch_client` tests | **Closed** | credit can vanish in a crash, by design |
| F7 | Keeper or admin sets risk freely, no timelock | one unbounded role | roles, bounds, step limits, timelock, guardian cancel | `governance.rs` unit (5) + LiteSVM (9) | **Closed** | judgement within bounds |
| F8 | Venue index collision, no deactivation | uniqueness read the wrong account | `init`-only, index-unique, lifecycle with timelock | 8 tests; 4 fail on the v0 binary | **Closed** | — |
| F9 | Mock oracle is the default; Pyth path a stub | default feature, unimplemented backend | no default; compile errors; Pyth parser, Full verification, feed per asset | CI must-fail checks; 2 parser tests on real bytes; 5 LiteSVM tests on the Pyth build | **Closed** | not run against the live receiver |
| F10 | Venue hot key can write for all users | keypair authority | `venue-cpi`: the authority must be the venue program's PDA | `venue_cpi.rs` (5) | **Closed** | venues still self-report (KNOWN_LIMITATIONS #1) |
| F11 | 12 vacuous negative tests; CI could not build or deploy | ad-hoc try/catch; old toolchain | `expectTxError` (7 self-tests); all 33 converted; tests type-check; CI pinned, program loaded, single gate; audit gates | helper self-tests; `tsc`; actionlint | **Closed, execution blocked** | `anchor test` and CI have not run yet |
| F12 | Testbench MISMATCH on the happy path; browser clock | UI defaults instead of on-chain inputs | testbench reads all inputs on chain, including the Clock sysvar; independent verifier incl. budgets | 13 verification tests; render tests; smoke (blocked) | **Closed** (smoke blocked) | — |
| F13 | SPEC/README wrong (args, instructions, claims) | docs written apart from the code | SPEC rewritten from the code; STATUS canonical; full doc set | claim-by-claim check; link check | **Closed** | — |
| F14 | Keeper-gated credit; keeper is a stub | computation needed a privileged keeper | permissionless deterministic refresh; minimal role-less keeper | LiteSVM; keeper tests (8) | **Closed** | liveness needs someone to pay fees |

New findings during the work (all fixed):
- F11b TS 4.9 could not parse `@types/node`.
- F11c tests never type-checked.
- F11d no program keypair, so deploys went to the wrong address.
- An all-long book got credit (model gap).
- A Borsh stack overflow.
- Oracle-evidence replay.
- A keeper fee loop.
- A 390 px overflow.
- The audit register was ignored by `*.json`.
- The audit gate mutated `yarn.lock`.
- The testbench's IM rule diverged from `ch_client`.

One new **open** design finding: cross-venue trust (C/E).

## D. Ratings (honest, 1–10)

| Dimension | Before | After | Why not higher |
| --- | ---: | ---: | --- |
| Economic safety | 3 | 6 | venues rely on each other's reports; no loss coverage; parameters are judgement |
| Security / access control | 3 | 7 | unaudited; upgrade authority and key custody not set up |
| Correctness | 4 | 7 | strong local evidence; integration tests not executed |
| Test evidence | 3 | 7 | CI has not run; no validator runs |
| Documentation | 4 | 8 | matches the code; no external review of the docs |
| Operational readiness | 2 | 4 | no deployment, monitoring designed but not built, keeper minimal |
| **Overall** | **4** | **6** | |

## E. Blockers to 9/10

1. **CI green on this branch**, including `anchor test` and the testbench smoke
   on a validator. This needs a push (H) and possibly CI fixes. Nothing here
   has proven the workflow works end to end.
2. **Cross-venue trust.** Design a per-venue acceptance set, or credit scoped
   to the reporting venues, so a venue applies credit only from hedges on
   venues it trusts. This is the largest open risk.
3. **External audit** of `ch_core` and `ch_math`.
4. **Loss coverage decision** for real deployments (fund or venue settlement),
   or a documented commitment that credit stays trading power only.
5. **Devnet deployment** with a verifiable build, a multisig upgrade authority,
   separate role keys, a real venue integration and the Pyth receiver.

## F. Next steps (in order)

1. Push the branch (commands in H) and get CI green, fixing whatever the
   first runs expose (most likely the `anchor-integration` job).
2. Design and implement venue acceptance sets (blocker 2). Add
   LiteSVM tests for a misreporting venue.
3. Deploy to devnet with `pyth,venue-cpi`, multisig upgrade authority,
   distinct role keys, and run the keeper against real Pyth sponsored feeds.
4. Integrate one real venue (or a fuller mock with a margin engine) that
   enforces `max(IM − credit, MM)` end to end.
5. Move the TS side to a current Solana/Anchor client and drop the accepted
   npm advisories.
6. Commission an external audit.

## G. Readiness verdict

**Not ready for mainnet or real funds. Ready for continued development and
external review on localnet/devnet.** Credit can no longer by itself
liquidate anyone, cannot be minted by a privileged key, and is invalidated
whenever its inputs change. But venues must trust each other's reports,
nothing covers losses, nothing is audited, and the validator-based tests and
CI have not yet run.

## H. Git summary

- Branch: `feat/clearinghouse-hardening` (never committed to `main`; no
  merge, rebase or reset of `main`; no force-push).
- 24 commits on top of `origin/main`, including the one that adds this report (`git log --oneline origin/main..HEAD`),
  each with root cause, change, verification and blocked items in its message.
  `06c51f7` alone does not compile `ch_core`. The next commit fixes it, so
  bisect across the pair.
- Identity: repo-local `Arnav0107 <arnavbansal.edu@gmail.com>`.
- Nothing secret is committed: no keypairs, `.env`, `target/`,
  `node_modules/`, `dist/` or ledger. The `*.json` ignore rule keeps
  keypairs out, with explicit exceptions for the IDL, vectors and audit register.
- **Not pushed.** The instructions allow a push only from the GitHub account
  `Arnav0107`, confirmed with `gh auth status`. `gh` is not installed on
  this host, so the account cannot be confirmed. To push, run:

```bash
gh auth status                      # must show: Logged in to github.com account Arnav0107
cd C:/Hackathon/Colosseum/Colosseum
git branch --show-current           # must print: feat/clearinghouse-hardening
git push -u origin feat/clearinghouse-hardening
```

No pull request has been opened.
