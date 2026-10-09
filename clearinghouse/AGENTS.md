# AGENTS.md — Clearinghouse Monorepo Rules & Guidelines

## 1. Architecture & Protocol Philosophy
Clearinghouse reads a user's perp positions across external DeFi perps venues, computes combined (netted) portfolio risk off-chain and on-chain, and publishes a verifiable **margin credit** that participating venues subtract from their local required collateral.

- **Non-Custodial Invariant**: Participating venues retain user custody. Clearinghouse programs NEVER hold or custody user trading funds. The only capital pooled on-chain is the default fund (`ch_fund`) for waterfall solvency.
- **Strict Integer Arithmetic**: On-chain programs and Solana crates MUST use integer math only (standard fixed-point `1e6`, e.g. 1 USD = 1_000_000 units). **Floating-point math (`f32`, `f64`) is strictly forbidden on-chain**.
- **Signed Legs & Correlation**: rho is the ASSET correlation. Direction comes from the sign of each leg (+ for long, - for short).
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

## 5. Trust Assumptions & Operational Invariants
1. **Keeper Controls Correlations**: Pairwise asset correlation matrix values are directly reported and maintained on-chain by the authorized `keeper_authority`. On-chain netting algorithms trust these values within `corr_max_age_slots`.
2. **Venues Self-Report Snapshots**: Position snapshots are submitted on-chain by each registered `venue_authority`. Venues are responsible for reporting accurate notional values and margin requirements.
3. **Mock Oracle Only on Localnet**: The `mock-oracle` feature is strictly for localnet testing. Production builds must compile `--no-default-features --features pyth`.

---

## 6. Implemented (Real) vs Mock / Stubbed / Future

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

## 7. Verification Commands
- Check workspace tests: `cargo test --workspace`
- Build Solana programs: `anchor build -p ch_core`
- Run integration tests: `anchor test` or `yarn run ts-mocha -p ./tsconfig.json -t 1000000 'tests/ch_core.ts'`
- Run headless smoke tests: `cd testbench && npm run smoke`

