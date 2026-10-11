# AGENTS.md — Clearinghouse Monorepo Rules & Guidelines

## 1. Architecture & Protocol Philosophy
Clearinghouse reads a user's perp positions across external DeFi perps venues, computes one joint, stressed portfolio risk number on-chain, and publishes **trading-power credit**: venues may lower the user's *initial* margin for risk-increasing actions by it, never below maintenance margin (`max(IM − credit, MM)`). Credit is never collateral and never withdrawable. Status: `docs/STATUS.md` (canonical).

- **Non-Custodial Invariant**: Participating venues retain user custody. Clearinghouse programs NEVER hold or custody user trading funds. `ch_fund` is a stub; no capital is pooled (docs/FUND_DESIGN.md).
- **Fail Closed**: production defaults must fail closed. Mocks live only behind explicit features (`mock-oracle`, `dev-venue-signer`); `ch_core` does not compile without an explicit feature choice.
- **Strict Integer Arithmetic**: On-chain programs and Solana crates MUST use integer math only (standard fixed-point `1e6`, e.g. 1 USD = 1_000_000 units). **Floating-point math (`f32`, `f64`) is strictly forbidden on-chain**.
- **Signed Legs & Correlation**: rho is the ASSET correlation. Direction comes from the sign of each leg (+ for long, - for short).
- **Cross-Program Invocation (CPI)**: Venues report by CPI into `ch_core::submit_position_snapshot`, signed by their `[b"clearinghouse"]` PDA, and apply credit with `ch_client` (docs/VENUE_INTEGRATION.md).

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
Status of each item: `docs/STATUS.md` (single source).
```text
clearinghouse/
├── AGENTS.md            # Agent rules, architecture guidelines, and constraints
├── SPEC.md              # Technical specification, account schemas, instruction signatures
├── README.md            # Monorepo overview and developer quickstart
├── Anchor.toml          # Anchor workspace configuration
├── Cargo.toml           # Root workspace Cargo manifest
├── docs/                # Protocol documentation
├── programs/
│   ├── ch_core/         # Protocol: governance, venues, consent, snapshots, portfolio credit
│   ├── ch_fund/         # Default fund program
│   ├── mock_perps_a/    # Example venue: reports by CPI
│   └── mock_perps_b/    # Example venue: reports by CPI
├── crates/
│   ├── ch_client/       # Venue rules: usable_credit, max(IM − credit, MM)
│   └── ch_math/         # Pure Rust mathematical engine (no Anchor dependencies)
├── tests/               # Anchor TypeScript integration test suites
├── services/
│   ├── keeper/          # Role-less keeper (refresh / release)
│   └── risk/            # Risk service
└── dashboard/           # Dashboard
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
2. Services hold no protocol role. The keeper only pays for permissionless instructions; never log keys or full RPC URLs.
3. Environment variables (`.env`) must never be committed to git.

---

## 5. Trust Assumptions & Operational Invariants
See `docs/THREAT_MODEL.md`. In short: venues report honestly (and rely on each other's reports); governance acts within on-chain bounds with a timelock for credit-increasing changes; Pyth prices with Full verification; nobody needs to be trusted to compute credit (refresh and invalidation are permissionless).

---

## 6. Implemented (Real) vs Mock / Stubbed / Future
Canonical table: `docs/STATUS.md`. Do not duplicate it here.

---

## 7. Verification Commands
Full list: `docs/TESTING_AND_CI.md`. Most used:
- `cargo test --workspace --features ch_core/mock-oracle,ch_core/dev-venue-signer`
- `cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,dev-venue-signer` then `cargo test --manifest-path crates/ch_svm_tests/Cargo.toml`
- `anchor test --validator legacy --skip-build --skip-deploy` (needs a validator)
- `cd testbench && npm test` (and `npm run smoke` against a running localnet)

