# AGENTS.md — Clearinghouse Monorepo Rules & Guidelines

## 1. Architecture & Protocol Philosophy
Clearinghouse reads a user's perp positions across external DeFi perps venues, computes combined (netted) portfolio risk off-chain and on-chain, and publishes a verifiable **margin credit** that participating venues subtract from their local required collateral.

- **Non-Custodial Invariant**: Participating venues retain user custody. Clearinghouse programs NEVER hold or custody user trading funds. The only capital pooled on-chain is the default fund (`ch_fund`) for waterfall solvency.
- **Strict Integer Arithmetic**: On-chain programs and Solana crates MUST use integer math only (standard fixed-point `1e6`, e.g. 1 USD = 1_000_000 units). **Floating-point math (`f32`, `f64`) is strictly forbidden on-chain**.
- **Cross-Program Invocation (CPI)**: Venues interact with Clearinghouse via `ch_client` or CPI into `ch_core` account snapshots and credit attestations.

---

## 2. Team & Code Ownership
All contributors and AI agents must respect domain ownership:

| Area | Components | Owner | Description |
| :--- | :--- | :--- | :--- |
| **Core Architecture & Math** | `programs/ch_core`<br>`crates/ch_math` | **Dev A** | Netting protocol, account snapshots, consent PDA, correlation matrices, pure Rust portfolio variance math. |
| **Venues, SDK & Fund** | `programs/mock_perps_a`<br>`programs/mock_perps_b`<br>`crates/ch_client`<br>`programs/ch_fund` | **Dev B** | Venue integration library (`ch_client`), sample venues A/B, default fund capitalization & waterfall distribution. |
| **Off-Chain Services & UI** | `services/keeper`<br>`services/risk`<br>`dashboard/` | **Dev C** | Keeper pump bot, Pyth oracle correlation feed, real-time analytics dashboard. |

---

## 3. Directory Layout
```text
clearinghouse/
├── AGENTS.md            # Agent rules, architecture guidelines, and constraints
├── SPEC.md              # Technical specification, account schemas, instruction signatures
├── README.md            # Monorepo overview and developer quickstart
├── Anchor.toml          # Anchor workspace configuration
├── Cargo.toml           # Root workspace Cargo manifest
├── docs/                # Protocol documentation
├── programs/
│   ├── ch_core/         # Core state: consent, venue registry, snapshots, margin credits
│   ├── ch_fund/         # Default fund and liquidation waterfall
│   ├── mock_perps_a/    # Reference perpetuals venue A
│   └── mock_perps_b/    # Reference perpetuals venue B
├── crates/
│   ├── ch_client/       # Venue client crate: required_margin = own_margin - credit
│   └── ch_math/         # Pure Rust mathematical engine (no Anchor dependencies)
├── tests/               # Anchor TypeScript integration test suites
├── services/
│   ├── keeper/          # Keeper bot for publishing margin credit updates
│   └── risk/            # Risk modeling service: Pyth oracles, covariance matrix
└── dashboard/           # Protocol monitoring and risk visualization UI
```

---

## 4. Development Rules & Invariants

### 4.1 On-Chain Program Invariants (Rust / Anchor)
1. **No Floats**: Every calculation on-chain must be performed with fixed-point integers (1e6 scaling).
2. **Deterministic Computations**: Overflow checks must always be enabled. Use `checked_*` arithmetic or Anchor's default checked math.
3. **No Dynamic Allocations / Unbounded Loops**: Position snapshots and venue lists must have defined maximum capacities.
4. **Security**: Validate all account keys, signer flags, PDA seeds, and program ownership before performing state transitions.

### 4.2 Mathematical Crate (`crates/ch_math`)
1. **Zero Anchor Dependency**: `ch_math` must remain a pure Rust library compileable to both native and BPF targets without Anchor.
2. **Fixed-Point Precision**: Precision scaling constant is `PRECISION = 1_000_000` (1e6).

### 4.3 Off-Chain Services (`services/*`)
1. Implemented in TypeScript/Node.js.
2. Read-only oracle queries or signed transactions executed strictly through authorized keeper keypairs.
3. Environment variables (`.env`) must never be committed to git.

---

## 5. Verification Commands
- Check workspace tests: `cargo test --workspace`
- Build Solana programs: `anchor build`
- Run integration tests: `anchor test`
