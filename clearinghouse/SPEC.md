# SPEC.md — Clearinghouse Specification

## 1. Executive Summary
Clearinghouse provides cross-venue portfolio netting and margin credit allocation across Solana DeFi perpetuals protocols. Custody remains decentralized at each venue; Clearinghouse performs credit calculation and mutualized risk backstopping.

All on-chain monetary and risk numbers represent fixed-point values scaled by `1e6` (`PRECISION = 1_000_000`), corresponding to micro-USD (e.g. $10,000.00 = `10_000_000_000`).

---

## 2. Mathematical Reference & Golden Vectors

### 2.1 Netting Formulation
Given two positions with unhedged margin requirements $a$ and $b$, and asset correlation $\rho \in [-1.0, 1.0]$:
$$\sigma_{\text{net}} = \sqrt{a^2 + b^2 + 2 \rho a b}$$
$$\text{Margin Credit} = (a + b) - \sigma_{\text{net}}$$

### 2.2 Golden Test Vectors
- **Input Position A ($a$)**: $10,000.00 (`10_000_000_000` fixed-point 1e6 units, or base `10_000`)
- **Input Position B ($b$)**: $10,000.00 (`10_000_000_000` fixed-point 1e6 units, or base `10_000`)
- **Correlation ($\rho$)**: `-0.8` (`-800_000` in 1e6 fixed point)

**Results**:
- **Combined Net Risk ($\sigma_{\text{net}}$)**:
  $$\sqrt{10000^2 + 10000^2 + 2(-0.8)(10000)(10000)} = \sqrt{200000000 - 160000000} = \sqrt{40000000} \approx 6,324.555 \rightarrow \mathbf{6,325}$$
- **Total Margin Credit**:
  $$20,000 - 6,325 = \mathbf{13,675}$$
- **Credit Allocation per App (50/50 split)**:
  $$13,675 / 2 = \mathbf{6,837.50}$$
- **Liquidation SOL Drop Scenarios**:
  - Collateral $10,000.00 $\rightarrow$ **16.0%** SOL drop tolerance
  - Collateral $7,000.00 $\rightarrow$ **11.2%** SOL drop tolerance
  - Collateral $3,162.50 $\rightarrow$ **5.06%** SOL drop tolerance

---

## 3. Account Specifications (`ch_core`)

### 3.1 `GlobalConfig`
Main governance and parameter state for the Clearinghouse protocol.
```rust
pub struct GlobalConfig {
    pub admin: Pubkey,
    pub keeper_authority: Pubkey,
    pub default_fund_program: Pubkey,
    pub max_venues: u8,
    pub bump: u8,
    pub reserved: [u8; 64], // TODO: Define future governance parameters
}
```

### 3.2 `VenueRegistration`
Authorized venue whitelist entry.
```rust
pub struct VenueRegistration {
    pub venue_id: [u8; 32],
    pub venue_program_id: Pubkey,
    pub is_active: bool,
    pub weight_bps: u16,    // TODO: Dynamic risk weights per venue
    pub bump: u8,
}
```

### 3.3 `UserConsent`
Explicit PDA signed by the user delegating permission for their snapshots to be netted.
```rust
pub struct UserConsent {
    pub user: Pubkey,
    pub is_active: bool,
    pub authorized_venues_bitmap: u64, // Bitmask of registered venues
    pub bump: u8,
    // TODO: Expiration timestamp or revocability flags
}
```

### 3.4 `PosSnapshot`
Timestamped position snapshot reported or read from a venue.
```rust
pub struct PosSnapshot {
    pub user: Pubkey,
    pub venue_id: [u8; 32],
    pub notional_value: u64, // Scaled 1e6
    pub is_long: bool,
    pub maintenance_margin: u64,
    pub slot: u64,
    pub timestamp: i64,
    // TODO: Mark fields for multi-asset basket breakdowns
}
```

### 3.5 `MarginCredit`
Current published margin credit for a specific user and venue.
```rust
pub struct MarginCredit {
    pub user: Pubkey,
    pub venue_id: [u8; 32],
    pub credit_amount: u64,  // Scaled 1e6
    pub valid_until_slot: u64,
    pub update_epoch: u64,
    pub bump: u8,
    // TODO: Slash conditions if credit revoked
}
```

### 3.6 `CorrelationMatrix`
Stores pairwise asset correlation parameters signed by keeper/risk oracle.
```rust
pub struct CorrelationMatrix {
    pub oracle_authority: Pubkey,
    pub updated_at: i64,
    pub correlations: [[i64; 8]; 8], // Scaled 1e6 fixed point [-1e6, +1e6]
    // TODO: Dynamic asset lookup table
}
```

---

## 4. Program Instruction Signatures

### 4.1 `ch_core`
- `initialize(ctx: Context<Initialize>, params: InitParams) -> Result<()>`
- `register_venue(ctx: Context<RegisterVenue>, venue_id: [u8; 32], venue_program: Pubkey) -> Result<()>`
- `update_user_consent(ctx: Context<UpdateConsent>, enable: bool) -> Result<()>`
- `submit_position_snapshot(ctx: Context<SubmitSnapshot>, snapshot_data: PosSnapshotData) -> Result<()>`
- `publish_margin_credit(ctx: Context<PublishCredit>, user: Pubkey, venue_id: [u8; 32], credit: u64) -> Result<()>`

### 4.2 `ch_fund`
- `initialize_fund(ctx: Context<InitFund>) -> Result<()>`
- `deposit_guarantee(ctx: Context<DepositGuarantee>, amount: u64) -> Result<()>`
- `process_waterfall_draw(ctx: Context<ProcessWaterfall>, deficit_amount: u64) -> Result<()>`

### 4.3 `mock_perps_a` & `mock_perps_b`
- `initialize_market(ctx: Context<InitMarket>) -> Result<()>`
- `open_position(ctx: Context<OpenPosition>, size: u64, is_long: bool) -> Result<()>`
- `apply_margin_credit(ctx: Context<ApplyCredit>, credit_amount: u64) -> Result<()>`
- `liquidate_position(ctx: Context<Liquidate>) -> Result<()>`

---

## 5. Client Library API (`crates/ch_client`)
```rust
pub fn calculate_effective_margin(
    own_required_margin: u64,
    approved_margin_credit: u64,
) -> u64 {
    // required_margin = own_margin - credit
    own_required_margin.saturating_sub(approved_margin_credit)
}
```

---

## 6. Pure Math Engine (`crates/ch_math`)
```rust
pub fn combined_risk(a: u64, b: u64, rho_scaled: i64) -> u64;
```
Inputs scaled by $10^6$. Output is the netted risk requirement scaled by $10^6$.
