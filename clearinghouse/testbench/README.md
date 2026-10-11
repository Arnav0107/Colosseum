# Clearinghouse test bench

A localnet console for `ch_core`. You set up venues, prices and positions;
compute a trader's portfolio credit; and check it against an independent
off-chain computation.

> **LOCALNET · MOCK ORACLE · UNVERIFIED VENUES.** The bench signs with fixed
> dev keys, the admin writes prices, and venues self-report with keypairs. It
> refuses non-loopback RPC URLs and the dev server binds to `127.0.0.1` only.
> Nothing shown here is a real credit decision.

Project status: [`../docs/STATUS.md`](../docs/STATUS.md).

## Run it

```bash
# 1. From clearinghouse/: build ch_core for localnet, start a validator with the
#    program at its declare_id! address, and sync the IDL + types into src/idl/.
./scripts/dev-localnet.sh

# 2. Start the UI.
cd testbench
npm ci
npm run dev        # http://127.0.0.1:5173
```

The script does **not** fund anything. Use step 1 in the UI ("Airdrop 10 SOL to
each dev key"). The smoke test below airdrops on its own.

After changing the program, regenerate and sync the IDL (the CI fails if the
committed copy differs):

```bash
anchor idl build -p ch_core -o target/idl/ch_core.json -t target/types/ch_core.ts \
  -- --features mock-oracle,dev-venue-signer
npm --prefix testbench run sync-idl
```

## What the page does

| Panel | On chain | Notes |
| --- | --- | --- |
| Connection | reads the Clock sysvar, checks the program is loaded | All ages and expiries use the **on-chain** clock, never the browser's. |
| Setup | `initialize`, `register_venue`, `set_asset_risk`, `update_correlations`, `update_user_consent` | Each step's state comes from chain, not local flags. Correlations move at most ±0.25 per update, so the bench walks there in steps. |
| Mock prices | `set_mock_price` | Stamped with the on-chain clock. Stale prices are flagged against `max_price_age_secs`. |
| Positions | `submit_position_snapshot`, `close_position_snapshot` | Inputs are validated with the program's rules (0 < MM ≤ IM ≤ notional). |
| Portfolio credit | `refresh_portfolio`, `invalidate_portfolio` | Per-leg breakdown: cap `min(IM·bps, IM−MM)`, credit, what the venue applies (`max(IM − credit, MM)`), usable now, baseline price. |
| Independent verification | none (pure) | `src/model/verify.ts` recomputes S, R_p, the hedge benefit and every leg's credit from on-chain inputs only. It uses the parity-tested engine (`src/portfolio.ts`) and protocol budgets, and reports MATCH, MISMATCH (listing each difference), or NOT COMPARABLE after an epoch or model change. |

## Code layout

```
src/
  chain/      bench.ts (transactions + decoding), pdas.ts, errors.ts
  model/      views.ts (plain account views), verify.ts, format.ts, rpc.ts
  components/ one file per panel, fields.tsx (labelled inputs), Banner.tsx
  portfolio.ts  TS port of ch_math (bit-identical; vectors.test.ts)
  math.ts       v0 pairwise math, kept as a reference for the F3 comparison
  idl/          generated IDL + TypeScript type (never edited by hand)
```

## Tests

```bash
npm test           # unit, parity-vector and server-rendered UI tests (no validator)
npx tsc --noEmit
npm run build
npm run smoke      # end to end against a running localnet (RPC_URL to override)
```

- `vectors.test.ts`: the Rust/TS parity vectors from `crates/ch_math`.
- `model/verify.test.ts`: MATCH / MISMATCH / stale handling, budgets, the
  venue-side rules, USD parsing and the loopback-only RPC rule.
- `ui.test.tsx`: the banner is present and comes before the content. Every input
  and select has a label, tables have column headers, the log is a polite live
  region, and the portfolio panel renders the breakdown and a MATCH verdict
  for the worked example.
- `scripts/smoke.e2e.ts`: drives the same `Bench` class the UI uses. It covers
  setup, two venue reports, refresh, a MATCH verdict, rejected unjustified
  invalidation, and invalidation by a newer 6% price move. CI is configured to run it against
  the built artifact (`.github/workflows/ci.yml`, anchor-integration job); it
  has not run yet, because no validator is available on the development host.

Accessibility: visible labels on every control, 44 px touch targets, visible
focus outlines, status text that does not rely on colour, and light/dark themes.
Tables scroll inside their own box, so the page has no horizontal scroll at 390 px.
