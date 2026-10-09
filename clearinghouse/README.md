# Clearinghouse Monorepo

Cross-venue decentralized portfolio risk netting and margin credit allocation protocol on Solana.

Clearinghouse aggregates user trading positions across decentralized perpetuals venues, computes global portfolio variance and netting, and issues verifiable margin credits to participating venues.

Venues retain custody of user funds at all times. Clearinghouse programs never take custody of trading collateral. (The mutualized default fund `ch_fund` is currently a stub).

> **Risk Modeling Principle**: `rho` is the ASSET correlation. Direction comes from the sign of each leg (+ for long, - for short). Netting is calculated using pure integer portfolio variance math in `ch_math`.

---

## Implemented (Real) vs Mock / Stubbed / Future

| Component / Subsystem | Status | Description |
| --- | --- | --- |
| `programs/ch_core` | **Real (Implemented)** | On-chain portfolio netting, venue registration, user consent bitmap, position snapshots, paired margin credit calculation (`compute_credit`), paired revokes (`revoke_credit`, `revoke_if_unsafe`, `revoke_if_basis_gone`), snapshot invalidation, and two-step admin controls. |
| `crates/ch_math` | **Real (Implemented)** | Pure integer portfolio variance arithmetic (`integer_sqrt`, `combined_risk`, `credit_total`, `split_pro_rata`), validated against golden vectors and property tests. |
| `testbench` | **Real (Implemented)** | Localnet operator console with BigInt math engine, setup lifecycle management, paired credit revoke controls, live price countdowns, snapshot age, and credit TTL tracking. |
| `tests/ch_core.ts` & `smoke.ts` | **Real (Implemented)** | Comprehensive integration and headless smoke tests validating all happy paths, boundary trips, and error branches on localnet. |
| Price Oracle (`mock-oracle`) | **Mock (Localnet Only)** | On-chain `MockPrice` account PDA with manual price, confidence, and timestamp updates for deterministic localnet testing. |
| Price Oracle (`pyth`) | **Stubbed (Future)** | Feature flag compiles cleanly with Pyth SDK but returns `OracleNotConfigured` until production Pyth feed accounts are wired. |
| `programs/ch_fund` | **Stub / Unbuilt** | Mutualized default fund and liquidation waterfall engine skeleton; not used in current netting flow. |
| `programs/mock_perps_a/b` | **Stub / Unbuilt** | Example mock perpetuals exchanges; position snapshots are currently submitted directly via `venue_authority` keypairs. |
| `services/keeper` & `services/risk` | **Unbuilt / Future** | Off-chain automated keeper bots and risk services; keeper operations are run interactively via the testbench or smoke test script. |
| `dashboard/` | **Unbuilt / Future** | Standalone production risk dashboard; testbench serves as the interactive operator interface. |

---

## Workspace Structure

```text
clearinghouse/
├── AGENTS.md            # Agent guidelines and ownership boundaries
├── SPEC.md              # Technical specification, account structs, and test vectors
├── README.md            # Monorepo documentation
├── Anchor.toml          # Anchor project configuration
├── Cargo.toml           # Cargo workspace definition
├── docs/                # Protocol architecture documentation
├── programs/
│   ├── ch_core/         # Core netting state, snapshots, and margin credit distribution (Active)
│   ├── ch_fund/         # Default fund skeleton (Stub)
│   ├── mock_perps_a/    # Mock perpetuals exchange A (Stub)
│   └── mock_perps_b/    # Mock perpetuals exchange B (Stub)
├── crates/
│   ├── ch_client/       # Venue client integration crate (Stub)
│   └── ch_math/         # Pure Rust mathematical netting engine (Active)
├── tests/               # Anchor TypeScript integration tests
├── testbench/           # Localnet operator console & smoke test suite (Active)
├── services/            # Placeholder service directories (Unbuilt)
└── dashboard/           # Placeholder dashboard directory (Unbuilt)
```

---

## Quickstart

### Prerequisites
- Rust 1.80+
- Solana CLI 2.x+
- Anchor CLI 0.30+
- Node.js 20+

### Build & Test
```bash
# Run workspace Rust tests (including ch_math golden vectors and oracle unit tests)
cargo test --workspace

# Build on-chain Solana Anchor program (default: mock-oracle for localnet/tests)
anchor build -p ch_core
```

### Oracle Backends
The `ch_core` program supports two mutual-exclusive price backends:
- `mock-oracle` (DEFAULT): For localnet and integration tests. Includes `MockPrice` account and `set_mock_price` instruction.
- `pyth`: Target architecture for production. Compiles with the Pyth SDK but returns `OracleNotConfigured` in the current build.

> **CRITICAL**: The mock oracle must never ship to mainnet. Production builds must be compiled without the mock oracle:
```bash
cargo build-sbf --no-default-features --features pyth
```
