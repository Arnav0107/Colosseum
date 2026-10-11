# Clearinghouse: cross-venue trading-power credit

A trader who is long SOL on one perps venue and short ETH on another is
partly hedged, but each venue margins its leg as if it stood alone.
Clearinghouse computes the hedge once, across venues, and gives each venue a
**trading-power credit**: the hedged user can open more size with the same
collateral.

What it is **not**:

- **Credit is not withdrawable.** It never counts as equity or collateral.
- **Credit never changes liquidation thresholds.** It only lowers the
  *initial* margin for new risk, never below maintenance margin:
  `max(IM − credit, MM)`.
- **Clearinghouse does NOT move gains between venues.** Collateral stays at
  each venue; no program holds user funds.
- **Roadmap, not built:** default fund, loss waterfall, circuit breaker,
  unwind coordinator, fee model, governance voting.

## How it works

1. Each venue program reports the user's positions to `ch_core` by CPI.
2. Anyone can trigger a refresh. `ch_core` computes one joint, stressed risk
   number over every leg and derives per-leg credit, capped at `IM − MM`.
3. A venue reads the credit and applies `max(IM − credit, MM)` to new risk.
   Any position change invalidates it immediately.

## Status

Research prototype, localnet only: not audited, not deployed, not for real
funds. What is real, mock or roadmap:
[`clearinghouse/docs/STATUS.md`](clearinghouse/docs/STATUS.md).

## Quickstart (localnet)

Requires Rust 1.99, Agave (Solana CLI) 4.3, Anchor CLI 1.2.0 and Node 20.

```bash
./scripts/dev-localnet.sh        # build ch_core (mock oracle, dev venue keys), start a validator, sync the IDL
cd clearinghouse/testbench
npm ci && npm run dev            # http://127.0.0.1:5173 (fund the dev keys from the UI)
```

Tests and every other command: [`clearinghouse/docs/TESTING_AND_CI.md`](clearinghouse/docs/TESTING_AND_CI.md).
