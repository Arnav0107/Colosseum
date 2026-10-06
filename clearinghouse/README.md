# Clearinghouse Monorepo

Cross-venue decentralized portfolio risk netting and margin credit allocation protocol on Solana.

Clearinghouse aggregates user trading positions across decentralized perpetuals venues, computes global portfolio variance and netting, and issues verifiable margin credits to participating venues.

Venues retain custody of user funds at all times. Clearinghouse programs never take custody of trading collateral, maintaining a mutualized default fund (`ch_fund`) for waterfall solvency.

> **Risk Modeling Principle**: `rho` is the ASSET correlation. Direction comes from the sign of each leg (+ for long, - for short). Netting is calculated using pure integer portfolio variance math in `ch_math`.

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
│   ├── ch_core/         # Core netting state, snapshots, and margin credit distribution (Dev A)
│   ├── ch_fund/         # Default fund and waterfall engine (Dev B)
│   ├── mock_perps_a/    # Mock perpetuals exchange A (Dev B)
│   └── mock_perps_b/    # Mock perpetuals exchange B (Dev B)
├── crates/
│   ├── ch_client/       # Venue client integration crate (Dev B)
│   └── ch_math/         # Pure Rust mathematical netting engine (Dev A)
├── tests/               # Anchor TypeScript integration tests
├── services/
│   ├── keeper/          # Margin credit keeper bot (Dev C)
│   └── risk/            # Risk calculation and oracle ingestion service (Dev C)
└── dashboard/           # Protocol risk visualization dashboard (Dev C)
```

---

## Ownership Allocation

- **Dev A**: `programs/ch_core`, `crates/ch_math`
- **Dev B**: `programs/mock_perps_a`, `programs/mock_perps_b`, `crates/ch_client`, `programs/ch_fund`
- **Dev C**: `services/keeper`, `services/risk`, `dashboard/`

---

## Quickstart

### Prerequisites
- Rust 1.80+ / 1.89+
- Solana CLI 2.x+ / 3.x+
- Anchor CLI 0.30+ / 1.1.2+
- Node.js 20+

### Build & Test
```bash
# Run workspace Rust tests (including ch_math golden vectors)
cargo test --workspace

# Build on-chain Solana Anchor programs
anchor build
```
