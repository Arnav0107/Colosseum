# Clearinghouse Test Bench

Operator test console and verification bench for the Clearinghouse protocol on Solana localnet.

## Quick Start

### 1. Start Localnet & Deploy

Start the localnet Solana test validator, fund test accounts, build and deploy the `ch_core` program, and initialize default fixtures using the automated startup script:

```bash
# From workspace root
./scripts/dev-localnet.sh
```

The script sets up a validator running at `http://127.0.0.1:8899` with preloaded test keypairs, builds `ch_core`, runs Anchor deploy, and synchronizes the generated IDL into `testbench/src/idl/ch_core.json`.

### 2. Manual IDL Sync

If you modify the Anchor program or re-run `anchor build`, sync the latest IDL to the test bench:

```bash
cd testbench
npm run sync-idl
```

### 3. Run Test Bench Dev Server

```bash
cd testbench
npm install
npm run dev
```

Open [http://localhost:5173](http://localhost:5173) in your browser.

> **Security Note:** The test bench strictly restricts RPC connection endpoints to `localhost` and `127.0.0.1`. Connections to public or non-local RPC endpoints will be rejected.

### 4. Run Math Verification Tests

The test bench includes pure TypeScript BigInt unit tests matching on-chain `ch_math` golden vectors:

```bash
cd testbench
npm test
```

### 5. Run Headless Smoke Test

Verify the end-to-end on-chain lifecycle against a running localnet validator:

```bash
cd testbench
npm run smoke
```

The script funds test keypairs, initializes the protocol, registers venues, records consent, updates correlations ($\rho = 0.8$), sets oracle prices, submits position snapshots, computes margin credit, verifies the [6837722339, 6837722340] golden vectors against `src/math.ts`, trips the price move safety guard with a 10% price move, and asserts credit revocation to 0.

## Implemented (Real) vs Mock / Stubbed / Future

| Component / Subsystem | Status | Description |
| --- | --- | --- |
| `programs/ch_core` | **Real (Implemented)** | On-chain portfolio netting, venue registration, user consent bitmap, position snapshots, paired margin credit calculation (`compute_credit`), paired revokes (`revoke_credit`, `revoke_if_unsafe`, `revoke_if_basis_gone`), snapshot invalidation, and two-step admin controls. |
| `crates/ch_math` | **Real (Implemented)** | Pure integer portfolio variance arithmetic (`integer_sqrt`, `combined_risk`, `credit_total`, `split_pro_rata`), validated against golden vectors and property tests. |
| `testbench` | **Real (Implemented)** | Localnet operator console with BigInt math engine, setup lifecycle management, paired credit revoke controls, live price countdowns, snapshot age, and credit TTL tracking. |
| `smoke.ts` | **Real (Implemented)** | Headless integration test covering 7 distinct lifecycle, hedging, and revocation flows on localnet. |
| Mock Price Oracle | **Mock (Localnet Only)** | On-chain `MockPrice` account PDA with manual price, confidence, and timestamp updates for deterministic localnet testing. |
| Pyth Oracle | **Stubbed (Future)** | Returns `OracleNotConfigured` until production Pyth price update accounts are integrated. |
| Default Fund (`ch_fund`) | **Stub / Unbuilt** | Mutualized default fund and liquidation waterfall engine skeleton. |
| Automated Keeper Bot | **Unbuilt / Future** | Keeper operations are executed via testbench controls or the smoke test script. |

## Features

- **Global Config & Matrix Setup**: Initialize global clearinghouse parameters, emergency pause status, and set symmetrical N×N correlation matrices.
- **Venue & Asset Registry**: Register trading venues, configure venue authorities, and register assets with maximum allowable haircuts.
- **Account State & Position Snapshots**: Post signed mock positions for Venue A and Venue B with customizable notional and required margins.
- **Oracle Controls (Mock Backend)**: Inspect and publish mock oracle asset prices and publish timestamps.
- **Credit Computation & Paired Revocation**: Execute `compute_credit`, verify pro-rata split allocations, test paired `revoke_credit`, `revoke_if_unsafe`, and `revoke_if_basis_gone`.
- **Golden Vector Match Calculator**: Client-side integer arithmetic calculator that reproduces exact on-chain margin offsets and credit allocations.
