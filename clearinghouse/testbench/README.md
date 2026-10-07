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

## Features

- **Global Config & Matrix Setup**: Initialize global clearinghouse parameters, emergency pause status, and set symmetrical N×N correlation matrices.
- **Venue & Asset Registry**: Register trading venues, configure venue authorities, and register assets with maximum allowable haircuts.
- **Account State & Position Snapshots**: Post signed mock positions for Venue A and Venue B with customizable notional and required margins.
- **Oracle Controls (Mock Backend)**: Inspect and publish mock oracle asset prices and publish timestamps.
- **Credit Computation & Revocation**: Execute `compute_credit`, verify pro-rata split allocations, test `revoke_credit`, and evaluate price safety guards via `revoke_if_unsafe`.
- **Golden Vector Match Calculator**: Client-side integer arithmetic calculator that reproduces exact on-chain margin offsets and credit allocations.
