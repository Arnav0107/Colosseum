use anchor_lang::prelude::*;

pub use ch_math::portfolio::{MAX_ASSETS, MAX_LEGS};
pub use ch_math::risk::MAX_SCENARIOS;

pub const SEED_CONFIG: &[u8] = b"config";
pub const SEED_VENUE: &[u8] = b"venue";
pub const SEED_CONSENT: &[u8] = b"consent";
pub const SEED_SNAPSHOT: &[u8] = b"snapshot";
pub const SEED_PORTFOLIO: &[u8] = b"portfolio";
pub const SEED_RISK_MODEL: &[u8] = b"risk_model";
pub const SEED_MOCK_PRICE: &[u8] = b"mock_price";

/// Venue indices live in a u64 bitmap.
pub const MAX_VENUES: usize = 64;
/// Upper triangle (i < j) of the MAX_ASSETS x MAX_ASSETS correlation matrix.
pub const CORR_LEN: usize = MAX_ASSETS * (MAX_ASSETS - 1) / 2;
/// Credit lifetime ceiling (~20 minutes at 400 ms slots).
pub const MAX_CREDIT_TTL_SLOTS: u64 = 3_000;
pub const RHO_ONE: i32 = 1_000_000;

/// Risk-parameter bounds (floors keep a minimum of stress in the model).
pub const MIN_SHOCK_BPS: u16 = 100;
pub const MIN_BASIS_BPS: u16 = 10;
pub const MAX_MOVE_BPS: u16 = 10_000;
pub const MIN_CORR_STRESS_PPM: u32 = 50_000;
pub const MIN_BASIS_DECORR_PPM: u32 = 10_000;
pub const PPM_ONE: u32 = 1_000_000;
pub const DEFAULT_CORR_STRESS_PPM: u32 = 100_000;
pub const DEFAULT_BASIS_DECORR_PPM: u32 = 20_000;

/// Portfolio status values.
pub mod status {
    /// Never computed.
    pub const EMPTY: u8 = 0;
    /// Credit is usable by venues while the epoch matches and it has not expired.
    pub const ACTIVE: u8 = 1;
    /// An input changed or a guard tripped; no credit is usable.
    pub const INVALIDATED: u8 = 2;
}

/// Index of the pair (a, b), a != b, in the upper-triangle correlation array.
pub fn corr_index(a: u8, b: u8) -> Option<usize> {
    let (i, j) = if a < b {
        (a as usize, b as usize)
    } else {
        (b as usize, a as usize)
    };
    if i == j || j >= MAX_ASSETS {
        return None;
    }
    // Rows 0..i contribute (n-1) + (n-2) + ... + (n-i) entries.
    Some(i * (2 * MAX_ASSETS - i - 1) / 2 + (j - i - 1))
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct ConfigParams {
    /// Highest venue index + 1 that may be registered (<= 64).
    pub max_venues: u8,
    /// Safety haircut applied to the computed hedge benefit.
    pub haircut_bps: u16,
    /// Credit lifetime in slots (1..=MAX_CREDIT_TTL_SLOTS).
    pub credit_ttl_slots: u64,
    /// A snapshot older than this cannot back credit.
    pub snapshot_max_age_slots: u64,
    /// Cap on one portfolio's total credit (micro-USD).
    pub max_credit_per_user: u64,
    /// Per-leg cap as a fraction of the leg's initial margin.
    pub max_credit_bps_of_im: u16,
    /// Cap on outstanding credit across all users, per venue (micro-USD).
    pub max_credit_per_venue: u64,
    /// Cap on outstanding credit across all users, per asset (micro-USD).
    pub max_credit_per_asset: u64,
    /// Protocol-wide cap on outstanding credit (micro-USD).
    pub global_credit_cap: u64,
    /// The risk model must have been updated within this many slots.
    pub risk_model_max_age_slots: u64,
    pub max_price_age_secs: i64,
    pub max_conf_bps: u16,
    /// Price move vs the portfolio's stored baseline that allows permissionless invalidation.
    pub max_move_bps: u16,
}

#[account]
#[derive(InitSpace)]
pub struct GlobalConfig {
    pub admin: Pubkey,
    pub proposed_admin: Pubkey,
    /// Bumped by every parameter, risk-model and venue-lifecycle change and by
    /// emergency invalidation. A portfolio is usable only while its epoch matches.
    pub risk_epoch: u64,
    pub params: ConfigParams,
    /// Indices ever assigned. Never cleared: an index is not reusable.
    pub used_venues_bitmap: u64,
    /// Indices of currently active venues (subset of `used_venues_bitmap`).
    pub active_venues_bitmap: u64,
    /// Outstanding credit counted in the current epoch. Reset on every epoch bump.
    pub global_outstanding_credit: u64,
    pub venue_outstanding: [u64; MAX_VENUES],
    pub asset_outstanding: [u64; MAX_ASSETS],
    pub paused: bool,
    pub bump: u8,
}

impl GlobalConfig {
    /// O(1) global invalidation: every portfolio computed in an older epoch
    /// becomes unusable, and its share of the counters is dropped here (it is
    /// never released again, see `Portfolio::release`).
    pub fn bump_epoch(&mut self) -> Result<u64> {
        self.risk_epoch = self
            .risk_epoch
            .checked_add(1)
            .ok_or(crate::ClearinghouseError::MathOverflow)?;
        self.global_outstanding_credit = 0;
        self.venue_outstanding = [0; MAX_VENUES];
        self.asset_outstanding = [0; MAX_ASSETS];
        Ok(self.risk_epoch)
    }

    pub fn venue_is_active(&self, venue_index: u8) -> bool {
        (venue_index as usize) < MAX_VENUES
            && self.active_venues_bitmap & (1u64 << venue_index) != 0
    }
}

/// Per-asset stress parameters as stored. `shock_bps == 0` means unsupported:
/// refresh fails closed for any leg in that asset.
#[zero_copy]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AssetRiskParams {
    pub shock_bps: u16,
    pub gap_bps: u16,
    pub basis_bps: u16,
}

/// Instruction argument form of `AssetRiskParams` (Borsh).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssetRiskArgs {
    pub shock_bps: u16,
    pub gap_bps: u16,
    pub basis_bps: u16,
}

impl From<AssetRiskArgs> for AssetRiskParams {
    fn from(a: AssetRiskArgs) -> Self {
        Self {
            shock_bps: a.shock_bps,
            gap_bps: a.gap_bps,
            basis_bps: a.basis_bps,
        }
    }
}

/// Zero-copy: at ~2.7 KiB a Borsh copy of this account does not fit in one
/// 4 KiB SBF stack frame, so it is read and written in place.
#[account(zero_copy)]
pub struct RiskModel {
    /// Incremented on every change.
    pub version: u64,
    pub updated_slot: u64,
    /// Upper-triangle asset correlations, scaled 1e6, in [-1e6, 1e6].
    pub corr: [i32; CORR_LEN],
    pub assets: [AssetRiskParams; MAX_ASSETS],
    /// Correlation stress delta, scaled 1e6.
    pub corr_stress_ppm: u32,
    /// Decorrelation of offsetting same-asset legs on different venues, 1e6.
    pub basis_decorr_ppm: u32,
    /// `ch_math::risk::scenario_kind` per slot (0 = unused).
    pub scenario_kind: [u8; MAX_SCENARIOS],
    /// Per-asset signed moves in bps; an unused slot is all zeros (no loss).
    pub scenarios: [[i16; MAX_ASSETS]; MAX_SCENARIOS],
    pub bump: u8,
    pub padding: [u8; 7],
}

pub const RISK_MODEL_SPACE: usize = 8 + core::mem::size_of::<RiskModel>();

impl RiskModel {
    pub fn rho(&self, a: u8, b: u8) -> i64 {
        if a == b {
            return RHO_ONE as i64;
        }
        match corr_index(a, b) {
            Some(k) => self.corr[k] as i64,
            // Unreachable for validated assets; 0 is the neutral value and the
            // engine rejects out-of-range assets before calling this.
            None => 0,
        }
    }
}

#[account]
#[derive(InitSpace)]
pub struct VenueRegistration {
    pub venue_id: [u8; 32],
    pub venue_program_id: Pubkey,
    pub venue_authority: Pubkey,
    pub venue_index: u8,
    pub is_active: bool,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct UserConsent {
    pub user: Pubkey,
    pub is_active: bool,
    pub authorized_venues_bitmap: u64,
    pub bump: u8,
}

impl UserConsent {
    pub fn allows(&self, venue_index: u8) -> bool {
        self.is_active
            && (venue_index as usize) < MAX_VENUES
            && self.authorized_venues_bitmap & (1u64 << venue_index) != 0
    }
}

/// One position of one user on one venue in one asset, as reported by the venue.
/// Seeds: [SEED_SNAPSHOT, user, venue_id, [asset_id]].
#[account]
#[derive(InitSpace)]
pub struct PosSnapshot {
    pub user: Pubkey,
    pub venue_id: [u8; 32],
    pub venue_index: u8,
    pub asset_id: u8,
    /// Monotonic: 1 on creation, +1 on every write.
    pub version: u64,
    pub is_long: bool,
    pub notional: u64,
    pub initial_margin: u64,
    pub maintenance_margin: u64,
    pub slot: u64,
    pub timestamp: i64,
    /// Who paid rent; receives it back when the snapshot is closed.
    pub rent_payer: Pubkey,
    pub bump: u8,
}

#[derive(
    AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace,
)]
pub struct LegKey {
    pub venue_index: u8,
    pub asset_id: u8,
}

/// The inputs a leg's credit was computed from, plus the credit itself.
#[derive(
    AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq, InitSpace,
)]
pub struct LegState {
    pub venue_index: u8,
    pub asset_id: u8,
    pub is_long: bool,
    pub snapshot_version: u64,
    pub snapshot_slot: u64,
    pub notional: u64,
    pub initial_margin: u64,
    pub maintenance_margin: u64,
    /// Oracle price and confidence at computation time (the guard baseline, F5).
    pub baseline_price: u64,
    pub baseline_conf: u64,
    pub credit: u64,
}

/// One per user. Seeds: [SEED_PORTFOLIO, user].
#[account]
#[derive(InitSpace)]
pub struct Portfolio {
    pub user: Pubkey,
    pub status: u8,
    /// Credit semantics flag; always CREDIT_KIND_TRADING_POWER_ONLY.
    pub credit_kind: u8,
    /// Incremented on every computation and every invalidation.
    pub version: u64,
    /// Config epoch / risk-model version the credit was computed under.
    pub risk_epoch: u64,
    pub risk_model_version: u64,
    pub computed_slot: u64,
    pub expires_slot: u64,
    pub total_credit: u64,
    /// True while `legs[*].credit` / `total_credit` are included in the
    /// config's outstanding counters for epoch `counted_epoch`.
    pub counted: bool,
    pub counted_epoch: u64,
    pub sum_standalone_risk: u64,
    pub portfolio_risk: u64,
    pub hedge_benefit_ppm: u32,
    /// Every (venue, asset) for which a snapshot account exists. A refresh
    /// must include all of them.
    pub num_registered: u8,
    pub registered: [LegKey; MAX_LEGS],
    /// Legs as of the last computation.
    pub num_legs: u8,
    pub legs: [LegState; MAX_LEGS],
    pub bump: u8,
}

/// The only credit kind: lowers a venue's initial-margin requirement for
/// risk-increasing actions. It is not collateral, not withdrawable equity, and
/// never changes a liquidation threshold.
pub const CREDIT_KIND_TRADING_POWER_ONLY: u8 = 1;

impl Portfolio {
    pub fn registered_keys(&self) -> &[LegKey] {
        &self.registered[..self.num_registered as usize]
    }

    pub fn is_registered(&self, key: LegKey) -> bool {
        self.registered_keys().contains(&key)
    }

    pub fn register(&mut self, key: LegKey) -> Result<()> {
        if self.is_registered(key) {
            return Ok(());
        }
        let n = self.num_registered as usize;
        require!(n < MAX_LEGS, crate::ClearinghouseError::PortfolioFull);
        self.registered[n] = key;
        self.num_registered += 1;
        Ok(())
    }

    pub fn unregister(&mut self, key: LegKey) {
        let n = self.num_registered as usize;
        if let Some(pos) = self.registered[..n].iter().position(|k| *k == key) {
            self.registered[pos] = self.registered[n - 1];
            self.registered[n - 1] = LegKey::default();
            self.num_registered -= 1;
        }
    }

    /// Make every credit in this portfolio unusable. Does not touch the
    /// global counters (callers that hold the config mutably call `release`).
    pub fn mark_invalid(&mut self) {
        if self.status == status::ACTIVE {
            self.status = status::INVALIDATED;
        }
        self.version = self.version.saturating_add(1);
    }

    /// Remove this portfolio's share from the outstanding-credit counters,
    /// exactly once. A share counted in an older epoch was already dropped
    /// by `GlobalConfig::bump_epoch`.
    pub fn release(&mut self, config: &mut GlobalConfig) -> Result<()> {
        if self.counted {
            if self.counted_epoch == config.risk_epoch {
                let err = crate::ClearinghouseError::CounterUnderflow;
                config.global_outstanding_credit = config
                    .global_outstanding_credit
                    .checked_sub(self.total_credit)
                    .ok_or(err)?;
                for leg in &self.legs[..self.num_legs as usize] {
                    let v = &mut config.venue_outstanding[leg.venue_index as usize];
                    *v = v.checked_sub(leg.credit).ok_or(err)?;
                    let a = &mut config.asset_outstanding[leg.asset_id as usize];
                    *a = a.checked_sub(leg.credit).ok_or(err)?;
                }
            }
            self.counted = false;
        }
        self.total_credit = 0;
        for leg in self.legs.iter_mut() {
            leg.credit = 0;
        }
        Ok(())
    }

    /// The venue-side validity rule, in one place: credit is usable only if
    /// the portfolio is Active, was computed in the current epoch, has not
    /// expired, and has a leg for exactly this (venue, asset).
    pub fn usable_credit(
        &self,
        config_epoch: u64,
        now_slot: u64,
        venue_index: u8,
        asset_id: u8,
    ) -> u64 {
        if self.status != status::ACTIVE
            || self.credit_kind != CREDIT_KIND_TRADING_POWER_ONLY
            || self.risk_epoch != config_epoch
            || now_slot > self.expires_slot
        {
            return 0;
        }
        self.legs[..self.num_legs as usize]
            .iter()
            .find(|l| l.venue_index == venue_index && l.asset_id == asset_id)
            .map(|l| l.credit)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corr_index_is_a_bijection_onto_the_upper_triangle() {
        let mut seen = vec![false; CORR_LEN];
        for a in 0..MAX_ASSETS as u8 {
            for b in 0..MAX_ASSETS as u8 {
                let k = corr_index(a, b);
                if a == b {
                    assert_eq!(k, None);
                    continue;
                }
                let k = k.unwrap();
                assert_eq!(Some(k), corr_index(b, a), "symmetric");
                if a < b {
                    assert!(!seen[k], "duplicate index");
                    seen[k] = true;
                }
            }
        }
        assert!(seen.iter().all(|s| *s));
        assert_eq!(corr_index(0, MAX_ASSETS as u8), None);
    }
}
