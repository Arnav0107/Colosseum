# Workflow rules (standing)

These rules apply to all work on this branch. Later instructions refer to
them by section.

## 1. Git

- Work only on branch `feat/venue-proof-and-evidence`. Never commit to,
  merge into, rebase or reset `main`. Never force-push. Never use `--no-verify`.
- Commit identity (repo-local): `Arnav0107 <arnavbansal.edu@gmail.com>`.
- Before every commit: `git branch --show-current` must not print `main`.
- One commit per completed step, not one per phase. Message format:

  ```
  <type>(<scope>): <summary> [finding IDs]

  Change: …
  Verified by: <exact commands> → <result>
  Not verified: …
  ```

- Before each commit, check `git diff --cached --stat`. Never commit keys,
  `.env`, `target/`, `node_modules/` or `test-ledger/`.
- Do not push, deploy, or touch real keys or clusters unless an instruction
  explicitly says so.

## 2. Evidence

- Never claim a test passed unless it ran in the current session.
- If a tool is missing (solana, anchor, cargo build-sbf, validator), say so,
  write the test anyway, and give the exact command to run it.
- Never delete or weaken an existing test or assertion to get green.
- Every new number in a doc (credit amounts, CU costs, backtest results) must
  come from a command that was run. Cite the command next to the number.

## 3. Invariants that must never weaken

1. Credit is `TradingPowerOnly` and not withdrawable.
2. Per-leg credit ≤ `IM − MM`.
3. Credit never changes the liquidation threshold (maintenance margin).
4. The program never holds user funds.
5. `mock-oracle` and `dev-venue-signer` are never default features.

## 4. Approval gate

Before any change to on-chain account layout, PDA seeds, instruction
signatures or `ch_core` semantics: post the plan (what changes, why,
migration impact, tests) and **wait for "approved"**.

No approval is needed for changes confined to `ch_math`, `ch_client`, the
keeper, `services/`, `scripts/`, docs, tests, and new crates.

## 5. Engine parity

The TypeScript port of `ch_math` (`testbench/src/portfolio.ts`) stays
bit-identical to Rust via the shared vectors in
`crates/ch_math/tests/vectors/`. Whenever engine behaviour changes, update the
vectors and **both** test suites (`crates/ch_math/tests/vectors.rs`,
`testbench/src/vectors.test.ts`) in the same commit.
Regenerate with `CH_UPDATE_VECTORS=1 cargo test -p ch_math --test vectors`.

## 6. Docs

`STATUS`, `SPEC`, `KNOWN_LIMITATIONS`, `THREAT_MODEL` and `RISK_MODEL` are
updated in the same commit as the behaviour they describe. They must never
claim more than the code does.
