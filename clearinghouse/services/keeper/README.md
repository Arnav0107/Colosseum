# Keeper (liveness only)

Keeps portfolio credit fresh and stops stale credit from counting.

It holds **no role**. `refresh_portfolio` and `invalidate_portfolio` are
permissionless and deterministic, so a keeper cannot create credit that any
other caller could not, and if every keeper stops, no credit becomes unsafe:
it expires (`credit_ttl_slots` ≤ 3,000 slots) and venues stop accepting it.

## What it does each tick

From on-chain state only (`src/decide.ts`):

| Portfolio | Action |
| --- | --- |
| Active, current epoch, not near expiry | nothing |
| Not active, epoch changed, or expiring within `KEEPER_MARGIN_SLOTS` | `refresh_portfolio` |
| Refresh fails because inputs are stale (snapshot, price, venue, consent…) | `invalidate_portfolio` so stale credit stops counting, then back off |
| Paused | never refresh; release expired, stale or invalid credit that still counts |

**Order within a tick** (`orderCandidates` in `src/decide.ts`), applied before
the `KEEPER_MAX_ACTIONS` budget: releases (`invalidate_portfolio`) first, then
refreshes by `expiresSlot` ascending, ties broken by portfolio address. Work
over the budget waits for the next tick, still in that order, so the credit
closest to lapsing always goes first.

## Running two instances

**Sharded (recommended):** start each instance with `KEEPER_INSTANCE=0/2`,
`KEEPER_INSTANCE=1/2`. Each portfolio has one owner (FNV-1a of its address
mod count), which acts at once. Another instance acts only once it has seen
the same portfolio, at the same on-chain version, still needing action for
`KEEPER_TAKEOVER_TICKS` ticks (default 2), i.e. when the owner looks dead.
Keep `takeover ticks × interval` well below `KEEPER_MARGIN_SLOTS`.
Tested: two concurrent sharded instances over 80 ticks make no double attempt
on any (portfolio, version) and see no failure at all; with one instance dead,
the other takes over its portfolios before any credit lapses.

A portfolio whose inputs are stale is retried by its owner after backoff, and
by the other instance after the takeover delay. These are later retries, not
concurrent duplicates; the second one fails as "inputs not ready" and backs off.

**Unsharded:** also safe, and tested (`src/keeper.test.ts`, against a fake
chain that applies the program's rules):

- Both instances choose the same actions from the same slot.
- A duplicate refresh is a second identical computation: the program releases
  the previous share before counting the new one, so counters never
  double-count.
- A duplicate invalidation fails with `InvalidationNotJustified`, which the
  keeper treats as "already done" (`benignRaces` in the tick stats).
- After a failure the keeper backs off that portfolio until its on-chain
  `version` changes or `backoffTicks` pass, so stale inputs don't cost a
  failed fee every tick.

The cost of a second instance is duplicate fees, not correctness.

## Run

```bash
npm ci && npm run build
KEEPER_RPC_URL=http://127.0.0.1:8899 \
KEEPER_KEYPAIR=/path/to/fee-payer.json \
KEEPER_ORACLES=mock \
npm start
```

| Variable | Meaning |
| --- | --- |
| `KEEPER_RPC_URL` | RPC endpoint. Logged as origin only, because provider URLs often carry API keys. |
| `KEEPER_KEYPAIR` | Fee payer's secret key file. Never logged; needs no role. |
| `KEEPER_ORACLES` | `mock` (localnet `MockPrice` PDAs), or a JSON file `{"<asset id>": "<Pyth PriceUpdateV2 account>"}`. Unknown assets fail closed. |
| `KEEPER_IDL` | Optional; default `../../testbench/src/idl/ch_core.json` (checked against the build in CI). |
| `KEEPER_INTERVAL_MS`, `KEEPER_MARGIN_SLOTS`, `KEEPER_MAX_ACTIONS` | Optional: 10000 / 300 / 50. |
| `KEEPER_INSTANCE`, `KEEPER_TAKEOVER_TICKS` | Optional: `"<index>/<count>"` for sharded instances; takeover delay (default 2). |

## Monitoring

Output is one JSON object per line: `start`, `tick` (counts: `attempted`, `ok`,
`benignRaces`, `inputsNotReady`, `unexpected`, `backedOff`, `deferredToOwner`,
`overBudget`, `durationMs`),
`action` (portfolio, action, reason, outcome, signature or error), `error`,
`stop` (cumulative metrics). `Keeper` also takes `KeeperHooks` for embedding.

Suggested alerts (see `docs/OPERATIONS.md`):
- no `tick` line for 3 intervals;
- `unexpected > 0`;
- `inputsNotReady` growing (a venue or oracle has stopped updating);
- a falling fee-payer balance.

## Tests

```bash
npm test     # tsc + node --test: decisions, ordering, N = 3 × cap starvation, sharded and unsharded two-instance runs, takeover, paused mode, backoff, hooks
```

Not covered here: the Anchor adapter (`src/chain.ts`) against a live validator.
Its instructions and account lists match the testbench smoke, which is wired
into CI but has not yet run against a validator.
