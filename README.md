# Clearinghouse

Cross-venue portfolio margining for Solana perpetuals venues. Venues report a
user's positions, and `ch_core` computes one joint, stressed risk number over all
of them. The result is **trading-power credit**: an amount by which a venue may
lower the user's *initial* margin for new risk, never below maintenance margin.
Venues keep custody; no program holds user funds.

> **Status: research prototype, localnet only. Not audited, not deployed, not
> for real funds.** See [`clearinghouse/docs/STATUS.md`](clearinghouse/docs/STATUS.md).

## Quickstart (localnet)

Requirements: Rust 1.99, Agave (Solana CLI) 4.3, Anchor CLI 1.2.0, Node 20.

```bash
./scripts/dev-localnet.sh            # builds ch_core (mock oracle, dev venue keys), starts a validator, syncs the IDL
cd clearinghouse/testbench
npm ci && npm run dev                # http://127.0.0.1:5173; fund the dev keys from the UI
```

The validator script does not fund any accounts; the testbench's first step does.

## Documentation

All in [`clearinghouse/`](clearinghouse/README.md):
- [STATUS](clearinghouse/docs/STATUS.md): what is real, mock or stub, and what has been verified.
- [SPEC](clearinghouse/SPEC.md): accounts, instructions, math.
- [RISK_MODEL](clearinghouse/docs/RISK_MODEL.md), [THREAT_MODEL](clearinghouse/docs/THREAT_MODEL.md), [KNOWN_LIMITATIONS](clearinghouse/docs/KNOWN_LIMITATIONS.md).
- [VENUE_INTEGRATION](clearinghouse/docs/VENUE_INTEGRATION.md), [OPERATIONS](clearinghouse/docs/OPERATIONS.md), [TESTING_AND_CI](clearinghouse/docs/TESTING_AND_CI.md), [PERFORMANCE](clearinghouse/docs/PERFORMANCE.md).
- [UPGRADE_REPORT](clearinghouse/docs/UPGRADE_REPORT.md): review findings F1–F14 and how each was closed.
