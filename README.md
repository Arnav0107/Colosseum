# Clearinghouse

Cross-venue margin netting for DeFi perpetuals on Solana.
Reads user positions across decentralized trading venues, computes netted portfolio variance, and publishes verifiable margin credits that venues subtract from their local required collateral.
Participating venues retain custody of user funds at all times; Clearinghouse programs never hold or custody user trading assets.

---

## Quickstart

### 1. Start Localnet & Deploy Programs
Run the localnet orchestrator script from the repository root:
```bash
./scripts/dev-localnet.sh
```
This builds the on-chain Anchor program (`ch_core`), syncs the IDL, starts a local `solana-test-validator`, funds test accounts, and deploys the programs.

### 2. Launch Operator Testbench
```bash
cd clearinghouse/testbench
npm install
npm run dev
```
Open `http://localhost:5173` in your browser to interact with the localnet operator console, configure parameters, simulate positions, and test paired margin credit computations and revocations.

---

## Status Table

| Subsystem / Feature | Status | Description |
| :--- | :--- | :--- |
| **Netting Math** | **Working** | Pure integer portfolio variance arithmetic (`crates/ch_math`) validated against golden vectors and property tests. |
| **Price Guard** | **Working** | On-chain staleness, confidence width, and price-movement breach checks (`programs/ch_core/src/oracle.rs`). |
| **Credit Computation** | **Working** | Paired margin credit evaluation (`compute_credit`) with consent verification, distinct venue enforcement, and correlation checks. |
| **Paired Revoke** | **Working** | Atomic pair revocation via `revoke_credit`, `revoke_if_unsafe`, and permissionless `revoke_if_basis_gone`. |
| **Testbench** | **Working** | Localnet operator UI with BigInt math engine, deterministic dev key derivation, setup persistence, and headless smoke test suite. |
| **Mutualized Default Fund (`ch_fund`)** | **Not yet built** | Default fund liquidation waterfall engine skeleton; unbuilt in current scope. |
| **Mock Venues (`mock_perps_a/b`)** | **Not yet built** | Reference perpetuals venue contracts; positions are currently submitted via authorized venue keys. |
| **Automated Keeper (`services/keeper`)** | **Not yet built** | Off-chain automated keeper bots; keeper instructions run interactively via testbench or scripts. |
| **Risk Service (`services/risk`)** | **Not yet built** | Off-chain continuous covariance matrix service; matrices are published via keeper commands. |
| **Risk Dashboard (`dashboard/`)** | **Not yet built** | Standalone production risk dashboard; testbench serves as the interactive operator interface. |
| **Pyth Backend** | **Not yet built** | Production Pyth pull oracle integration stub; currently compiles cleanly and fails closed with `OracleNotConfigured`. |

---

## Documentation

For technical details, architectural invariants, and specifications:
- [Clearinghouse Technical Specification](clearinghouse/SPEC.md) — Mathematical reference, account layouts, and error schemas.
- [Clearinghouse Subsystem README](clearinghouse/README.md) — Monorepo setup, oracle modes, and developer workflow.
