# SPEC.md — Clearinghouse Specification

## 1. Executive Summary
Clearinghouse provides cross-venue portfolio netting and margin credit allocation across Solana DeFi perpetuals protocols. Custody remains decentralized at each venue; Clearinghouse performs credit calculation and mutualized risk backstopping.

All on-chain monetary and risk numbers represent fixed-point values scaled by `1e6` (`PRECISION = 1_000_000`), corresponding to micro-USD (e.g. $10,000.00 = `10_000_000_000`).

---

## 2. Mathematical Reference & Golden Vectors

### 2.1 Netting Formulation
Given a set of portfolio legs where each leg $i$ has asset identifier $asset_i$ and signed required margin $r_i$ ($+r$ for long positions, $-r$ for short positions), and asset correlation matrix $\rho$:

> **Rule**: $\rho$ is the ASSET correlation. Direction comes from the sign of each leg.

The netted portfolio risk is:
$$\sigma_{\text{net}} = \text{isqrt\_ceil}\left( \sum_i \sum_j \frac{r_i \cdot r_j \cdot \rho(asset_i, asset_j)}{1\,000\,000} \right)$$
where $\rho(i, i) \equiv 1\,000\,000$.

Total allowable credit with haircut in basis points ($haircut\_bps$):
$$\text{Margin Credit} = \left\lfloor \frac{(\sum_i |r_i| - \sigma_{\text{net}}) \cdot (10\,000 - haircut\_bps)}{10\,000} \right\rfloor$$

### 2.2 Golden Test Vectors (micro-USD)
Legs $[+10\,000\,000\,000 \text{ on asset 0 (long SOL)}, -10\,000\,000\,000 \text{ on asset 1 (short ETH)}]$:
- $\rho(0,1) = +800\,000 \rightarrow \text{combined\_risk} = \mathbf{6\,324\,555\,321}$
- $\text{credit\_total}(\text{sum\_required} = 20\,000\,000\,000, \text{haircut} = 0) \rightarrow \mathbf{13\,675\,444\,679}$
- $\text{split\_pro\_rata}(13\,675\,444\,679, [10\,000\,000\,000, 10\,000\,000\,000]) \rightarrow [\mathbf{6\,837\,722\,339}, \mathbf{6\,837\,722\,340}]$
- $\rho(0,1) = 0 \rightarrow \mathbf{14\,142\,135\,624}$
- $\rho(0,1) = +1\,000\,000 \rightarrow \mathbf{0}$
- $\rho(0,1) = -1\,000\,000 \rightarrow \mathbf{20\,000\,000\,000}$

Same-direction legs $[+10\,000\,000\,000, +10\,000\,000\,000]$:
- $\rho(0,1) = +800\,000 \rightarrow \text{combined\_risk} = \mathbf{18\,973\,665\,962}$ ($\text{credit\_total} = \mathbf{1\,026\,334\,038}$)

Auxiliary Risk Formulas:
- $\text{fund\_cap}(100\,000\,000\,000, 200) \rightarrow \mathbf{5\,000\,000\,000\,000}$
- $\text{liquidation\_drop\_bps}(10\,000\,000\,000, 50\,000\,000\,000, 8\,000) \rightarrow \mathbf{1\,600}$
- $\text{liquidation\_drop\_bps}(7\,000\,000\,000, 50\,000\,000\,000, 8\,000) \rightarrow \mathbf{1\,120}$
- $\text{liquidation\_drop\_bps}(3\,162\,277\,661, 50\,000\,000\,000, 8\,000) \rightarrow \mathbf{505}$ (~5.05%)
- $\text{price\_move\_bps}(100\,000\,000, 110\,000\,000) \rightarrow \mathbf{1\,000}$
- $\text{price\_move\_bps}(0, 1) \rightarrow \mathbf{u64::MAX}$

---

## 3. Account Specifications (`ch_core`)

### 3.1 `GlobalConfig`
Main governance and parameter state for the Clearinghouse protocol.
```rust
pub struct GlobalConfig {
    pub admin: Pubkey,
    pub proposed_admin: Pubkey,
    pub keeper_authority: Pubkey,
    pub default_fund_program: Pubkey,
    pub haircut_bps: u16,
    pub max_credit_bps_of_required: u16,
    pub credit_ttl_slots: u64,
    pub snapshot_max_age_slots: u64,
    pub max_venues: u8,
    pub max_price_age_secs: u64,
    pub max_conf_bps: u16,
    pub max_move_bps: u16,
    pub corr_max_age_slots: u64,
    pub paused: bool,
    pub used_venues_bitmap: u64,
    pub bump: u8,
    pub reserved: [u8; 64],
}
```

### 3.2 `VenueRegistration`
Authorized venue whitelist entry.
```rust
pub struct VenueRegistration {
    pub venue_id: [u8; 32],
    pub venue_program_id: Pubkey,
    pub venue_authority: Pubkey,
    pub venue_index: u8,
    pub is_active: bool,
    pub weight_bps: u16,
    pub bump: u8,
}
```

### 3.3 `UserConsent`
Explicit PDA signed by the user delegating permission for their snapshots to be netted.
```rust
pub struct UserConsent {
    pub user: Pubkey,
    pub is_active: bool,
    pub authorized_venues_bitmap: u64,
    pub bump: u8,
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
    pub required_margin: u64,
    pub slot: u64,
    pub timestamp: i64,
    pub asset_id: u8,
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
}
```

### 3.6 `CorrelationMatrix`
Stores pairwise asset correlation parameters signed by keeper/risk oracle.

> **Trust Assumption**: The keeper controls correlations. Pairwise asset correlation matrix values are directly reported and maintained on-chain by the authorized keeper (`keeper_authority`). On-chain risk netting algorithms trust these values within the configured `corr_max_age_slots` staleness threshold.

```rust
pub struct CorrelationMatrix {
    pub oracle_authority: Pubkey,
    pub updated_at: i64,
    pub updated_slot: u64,
    pub correlations: [[i64; 8]; 8], // Scaled 1e6 fixed point [-1e6, +1e6]
    pub bump: u8,
}
```

---

## 4. Program Instruction Signatures

### 4.1 `ch_core`
- `initialize(ctx: Context<Initialize>, params: InitConfigParams) -> Result<()>`
- `register_venue(ctx: Context<RegisterVenue>, venue_id: [u8; 32], venue_program_id: Pubkey, venue_authority: Pubkey, venue_index: u8, weight_bps: u16) -> Result<()>`
- `update_user_consent(ctx: Context<UpdateConsent>, authorized_venues_bitmap: u64, is_active: bool) -> Result<()>`
- `submit_position_snapshot(ctx: Context<SubmitSnapshot>, notional_value: u64, is_long: bool, required_margin: u64, asset_id: u8) -> Result<()>`
- `invalidate_snapshot(ctx: Context<InvalidateSnapshot>) -> Result<()>`
- `compute_credit(ctx: Context<ComputeCredit>) -> Result<()>`
- `revoke_credit(ctx: Context<RevokeCredit>) -> Result<()>`
- `revoke_if_unsafe(ctx: Context<RevokeIfUnsafe>) -> Result<()>`
- `revoke_if_basis_gone(ctx: Context<RevokeIfBasisGone>) -> Result<()>`
- `update_correlations(ctx: Context<UpdateCorrelations>, matrix: [[i64; 8]; 8]) -> Result<()>`
- `update_params(ctx: Context<UpdateParams>, params: InitConfigParams) -> Result<()>`
- `set_keeper(ctx: Context<SetKeeper>, new_keeper: Pubkey) -> Result<()>`
- `propose_admin(ctx: Context<ProposeAdmin>, new_admin: Pubkey) -> Result<()>`
- `accept_admin(ctx: Context<AcceptAdmin>) -> Result<()>`
- `set_paused(ctx: Context<SetPaused>, paused: bool) -> Result<()>`

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
pub fn effective_required_margin(
    own_required: u64,
    credit_amount: u64,
    valid_until_slot: u64,
    now_slot: u64,
) -> u64;
```
Returns `own_required.saturating_sub(credit_amount)` if `now_slot <= valid_until_slot`, else `own_required`.

---

## 6. Pure Math Engine (`crates/ch_math`)
```rust
pub struct Leg {
    pub asset: u8,
    pub signed_required_margin: i64, // + long, - short
}

pub enum MathError {
    NegativeVariance,
    Overflow,
}

pub fn isqrt_floor(n: u128) -> u128;
pub fn isqrt_ceil(n: u128) -> u128;
pub fn combined_risk(legs: &[Leg], rho: &dyn Fn(u8, u8) -> i64) -> Result<u64, MathError>;
pub fn credit_total(sum_required: u64, combined: u64, haircut_bps: u16) -> u64;
pub fn split_pro_rata<const N: usize>(total: u64, required: [u64; N]) -> [u64; N];
pub fn fund_cap(fund_balance: u64, min_ratio_bps: u16) -> u64;
pub fn liquidation_drop_bps(collateral: u64, position: u64, trigger_bps: u16) -> u64;
pub fn price_move_bps(old_price: u64, new_price: u64) -> u64;
```
All inputs and outputs scaled by $10^6$ (micro-USD) or basis points where specified.

---

## 7. Trust Assumptions & Risk Invariants

1. **Keeper Controls Correlations**: Pairwise asset correlation matrix values are directly reported and maintained on-chain by `keeper_authority`. Netting calculations trust these values within `corr_max_age_slots`.
2. **Venues Self-Report Snapshots**: Position snapshots are signed and submitted on-chain by the registered `venue_authority`. Venues are responsible for reporting accurate notional values and margin requirements.
3. **Mock Oracle Only on Localnet**: The `mock-oracle` feature allows manual price manipulation and is strictly for localnet testing. Production builds must compile `--no-default-features --features pyth`.

---

## 8. Implemented vs Stubbed Status

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

