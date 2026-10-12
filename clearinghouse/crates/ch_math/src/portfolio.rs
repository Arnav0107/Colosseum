//! Portfolio-level credit engine: one joint computation over *all* legs of
//! one user, replacing the pairwise scheme that double-counted hedges across
//! three or more venues (F3).
//!
//! Every function here is a pure, deterministic function of its inputs and
//! is independent of leg order. Rounding is always conservative: risk is
//! rounded up (by the risk model), credit is rounded down.
//!
//! Credit formula (see docs/RISK_MODEL.md):
//!
//! ```text
//!   S   = Σ standalone_risk(leg_i)           (≥ 0)
//!   R_p = portfolio_risk(legs)               (≥ 0)
//!   raw = floor( ΣIM · (S − R_p)⁺ / S )      (hedge benefit applied to IM)
//!   total = min( floor(raw · (10_000 − haircut_bps) / 10_000), max_credit_per_user,
//!                (ΣIM − R_p)⁺ )                        (floor, A2)
//!   share_i = floor( total · IM_i / ΣIM )    (pro rata, remainder dropped)
//!   credit_i = min( share_i, leg_cap_i )
//! ```
//!
//! With `standalone_risk = IM` and `portfolio_risk = combined_risk(signed IM)`
//! (the [`MarginAsRiskModel`], i.e. the v0 semantics) `raw = ΣIM − R_p`, so the
//! original golden vectors are reproduced exactly.

use crate::{combined_risk, Leg, MathError};

/// Upper bound on legs per portfolio. See docs/PERFORMANCE.md for the
/// derivation (transaction size without lookup tables: each leg needs one
/// snapshot and one oracle account in `refresh_portfolio`).
pub const MAX_LEGS: usize = 8;

/// Number of distinct assets the risk model can describe.
pub const MAX_ASSETS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortfolioLeg {
    pub venue_index: u8,
    pub asset: u8,
    pub is_long: bool,
    /// Absolute position notional, micro-USD.
    pub notional: u64,
    /// Venue's initial-margin requirement for this position, micro-USD.
    pub initial_margin: u64,
    /// Venue's maintenance-margin requirement for this position, micro-USD.
    pub maintenance_margin: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortfolioError {
    TooManyLegs,
    DuplicateLeg,
    InvalidAsset,
    /// The risk model has no parameters for this asset (fail closed).
    UnsupportedAsset,
    ZeroNotional,
    /// Violates 0 < maintenance_margin <= initial_margin <= notional.
    InvalidMargin,
    NegativeVariance,
    Overflow,
}

impl From<MathError> for PortfolioError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::NegativeVariance => PortfolioError::NegativeVariance,
            MathError::Overflow => PortfolioError::Overflow,
        }
    }
}

/// A risk model maps legs to standalone risks and a joint portfolio risk.
/// Implementations must be order-independent and round risk up.
pub trait RiskModel {
    fn standalone_risk(&self, leg: &PortfolioLeg) -> Result<u64, PortfolioError>;
    fn portfolio_risk(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError>;
    /// Portfolio risk, and whether it came from the ρ-free fallback because
    /// the correlation sub-matrix is not positive definite (A4). Models
    /// without a correlation check report `false`.
    fn portfolio_risk_report(&self, legs: &[PortfolioLeg]) -> Result<(u64, bool), PortfolioError> {
        Ok((self.portfolio_risk(legs)?, false))
    }
}

/// v0 semantics: required (initial) margin is used as the risk measure and
/// netted with the asset correlation matrix. Kept so the original golden
/// vectors still pin the arithmetic. Same-asset legs on different venues are
/// treated as perfectly correlated (ρ = 1).
pub struct MarginAsRiskModel<'a> {
    pub rho: &'a dyn Fn(u8, u8) -> i64,
}

impl RiskModel for MarginAsRiskModel<'_> {
    fn standalone_risk(&self, leg: &PortfolioLeg) -> Result<u64, PortfolioError> {
        Ok(leg.initial_margin)
    }

    fn portfolio_risk(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError> {
        let mut signed = [Leg {
            asset: 0,
            signed_required_margin: 0,
        }; MAX_LEGS];
        if legs.len() > MAX_LEGS {
            return Err(PortfolioError::TooManyLegs);
        }
        for (slot, leg) in signed.iter_mut().zip(legs) {
            let m = i64::try_from(leg.initial_margin).map_err(|_| PortfolioError::Overflow)?;
            *slot = Leg {
                asset: leg.asset,
                signed_required_margin: if leg.is_long { m } else { -m },
            };
        }
        Ok(combined_risk(&signed[..legs.len()], self.rho)?)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreditParams {
    pub haircut_bps: u16,
    /// Cap on a portfolio's total credit.
    pub max_credit_per_user: u64,
    /// Per-leg cap as a fraction of the leg's initial margin.
    pub max_credit_bps_of_im: u16,
    /// When true, each leg's credit is also capped at `IM − MM`, so a credit
    /// can never push a venue's requirement below its own maintenance margin.
    pub cap_at_im_minus_mm: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortfolioCredit {
    pub sum_standalone_risk: u64,
    pub portfolio_risk: u64,
    /// Display only: floor((S − R_p)⁺ · 1e6 / S).
    pub hedge_benefit_ppm: u32,
    pub sum_initial_margin: u64,
    /// Total after haircut, the per-user cap and the floor (ΣIM − R_p)⁺,
    /// before per-leg caps.
    pub total_before_leg_caps: u64,
    /// Final per-leg credits, aligned with the input legs.
    pub leg_credit: [u64; MAX_LEGS],
    pub num_legs: usize,
    /// A4: the correlation sub-matrix was not positive definite, so R_p is
    /// the ρ-free max(S, scenarios) and credit is 0.
    pub psd_fallback: bool,
}

impl PortfolioCredit {
    pub fn total(&self) -> u64 {
        self.leg_credit[..self.num_legs]
            .iter()
            .fold(0u64, |a, c| a.saturating_add(*c))
    }
}

/// Validate a leg set: bounded size, no duplicate (venue, asset), supported
/// asset, non-zero notional, and 0 < MM ≤ IM ≤ notional.
pub fn validate_legs(legs: &[PortfolioLeg]) -> Result<(), PortfolioError> {
    if legs.len() > MAX_LEGS {
        return Err(PortfolioError::TooManyLegs);
    }
    for (i, a) in legs.iter().enumerate() {
        if a.asset as usize >= MAX_ASSETS {
            return Err(PortfolioError::InvalidAsset);
        }
        if a.notional == 0 {
            return Err(PortfolioError::ZeroNotional);
        }
        if a.maintenance_margin == 0
            || a.maintenance_margin > a.initial_margin
            || a.initial_margin > a.notional
        {
            return Err(PortfolioError::InvalidMargin);
        }
        for b in &legs[i + 1..] {
            if a.venue_index == b.venue_index && a.asset == b.asset {
                return Err(PortfolioError::DuplicateLeg);
            }
        }
    }
    Ok(())
}

/// floor((S − R)⁺ · 1e6 / S); 0 when S == 0.
pub fn hedge_benefit_ppm(sum_standalone: u64, portfolio_risk: u64) -> u32 {
    if sum_standalone == 0 || portfolio_risk >= sum_standalone {
        return 0;
    }
    let num = ((sum_standalone - portfolio_risk) as u128) * 1_000_000;
    // < 1e6 because R > 0 or the difference is < S.
    (num / sum_standalone as u128) as u32
}

/// floor(sum_im · (S − R)⁺ / S), then floor(· (10_000 − haircut) / 10_000).
/// Two floors, each rounding credit down.
pub fn hedged_credit(
    sum_im: u64,
    sum_standalone: u64,
    portfolio_risk: u64,
    haircut_bps: u16,
) -> u64 {
    if sum_standalone == 0 || portfolio_risk >= sum_standalone || haircut_bps >= 10_000 {
        return 0;
    }
    let benefit = (sum_standalone - portfolio_risk) as u128;
    // sum_im, benefit < 2^64 so the product fits in u128.
    let raw = (sum_im as u128) * benefit / (sum_standalone as u128);
    // raw <= sum_im < 2^64.
    let after_haircut = raw * (10_000 - haircut_bps as u128) / 10_000;
    after_haircut as u64
}

/// The joint portfolio computation. Pure, deterministic, leg-order independent.
pub fn compute_portfolio_credit(
    legs: &[PortfolioLeg],
    model: &dyn RiskModel,
    params: &CreditParams,
) -> Result<PortfolioCredit, PortfolioError> {
    validate_legs(legs)?;

    let mut sum_standalone = 0u64;
    let mut sum_im = 0u64;
    for leg in legs {
        sum_standalone = sum_standalone
            .checked_add(model.standalone_risk(leg)?)
            .ok_or(PortfolioError::Overflow)?;
        sum_im = sum_im
            .checked_add(leg.initial_margin)
            .ok_or(PortfolioError::Overflow)?;
    }
    let (portfolio_risk, psd_fallback) = model.portfolio_risk_report(legs)?;

    // Floor (A2): once the model believes the book can lose all of ΣIM, no
    // credit. Post-credit requirement ΣIM − credit is then never below R_p
    // (when ΣIM ≥ R_p), whatever the ratio R_p / S.
    let total = hedged_credit(sum_im, sum_standalone, portfolio_risk, params.haircut_bps)
        .min(params.max_credit_per_user)
        .min(sum_im.saturating_sub(portfolio_risk));

    let mut leg_credit = [0u64; MAX_LEGS];
    if total > 0 && sum_im > 0 {
        for (out, leg) in leg_credit.iter_mut().zip(legs) {
            // share <= total because IM_i <= ΣIM.
            let share = ((total as u128) * (leg.initial_margin as u128) / (sum_im as u128)) as u64;
            let mut cap = ((leg.initial_margin as u128) * (params.max_credit_bps_of_im as u128)
                / 10_000) as u64;
            if params.cap_at_im_minus_mm {
                cap = cap.min(leg.initial_margin - leg.maintenance_margin);
            }
            *out = share.min(cap);
        }
    }

    Ok(PortfolioCredit {
        sum_standalone_risk: sum_standalone,
        portfolio_risk,
        hedge_benefit_ppm: hedge_benefit_ppm(sum_standalone, portfolio_risk),
        sum_initial_margin: sum_im,
        total_before_leg_caps: total,
        leg_credit,
        num_legs: legs.len(),
        psd_fallback,
    })
}

/// Scale credits down group by group so that each group's sum is at most its
/// headroom: for a group with sum S > H, credit_i = floor(credit_i · H / S).
/// Order-independent; never increases any credit.
pub fn cap_groups(credits: &mut [u64], group_of: &dyn Fn(usize) -> usize, headroom: &[u64]) {
    let mut sums = [0u128; 64];
    let n_groups = headroom.len().min(sums.len());
    for (i, c) in credits.iter().enumerate() {
        let g = group_of(i);
        if g < n_groups {
            sums[g] += *c as u128;
        }
    }
    for (i, c) in credits.iter_mut().enumerate() {
        let g = group_of(i);
        if g >= n_groups {
            // Unknown group: fail safe.
            *c = 0;
            continue;
        }
        let s = sums[g];
        let h = headroom[g] as u128;
        if s > h {
            *c = ((*c as u128) * h / s) as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leg(venue: u8, asset: u8, long: bool, im: u64) -> PortfolioLeg {
        PortfolioLeg {
            venue_index: venue,
            asset,
            is_long: long,
            notional: im * 5,
            initial_margin: im,
            maintenance_margin: im / 2,
        }
    }

    const P0: CreditParams = CreditParams {
        haircut_bps: 0,
        max_credit_per_user: u64::MAX,
        max_credit_bps_of_im: 10_000,
        cap_at_im_minus_mm: false,
    };

    #[test]
    fn golden_hedged_pair_matches_v0_credit_total() {
        let rho = |_: u8, _: u8| 800_000i64;
        let m = MarginAsRiskModel { rho: &rho };
        let legs = [
            leg(0, 0, true, 10_000_000_000),
            leg(1, 1, false, 10_000_000_000),
        ];
        let r = compute_portfolio_credit(&legs, &m, &P0).unwrap();
        assert_eq!(r.portfolio_risk, 6_324_555_321);
        assert_eq!(r.total_before_leg_caps, 13_675_444_679);
        // Remainder is dropped (order-independent), not given to the last leg.
        assert_eq!(&r.leg_credit[..2], &[6_837_722_339, 6_837_722_339]);

        let p = CreditParams {
            haircut_bps: 2_000,
            max_credit_per_user: 100_000_000_000,
            max_credit_bps_of_im: 7_500,
            cap_at_im_minus_mm: false,
        };
        let r = compute_portfolio_credit(&legs, &m, &p).unwrap();
        assert_eq!(r.total_before_leg_caps, 10_940_355_743);
        assert_eq!(&r.leg_credit[..2], &[5_470_177_871, 5_470_177_871]);
    }

    #[test]
    fn single_leg_gets_exactly_zero() {
        let rho = |_: u8, _: u8| 0i64;
        let m = MarginAsRiskModel { rho: &rho };
        let r = compute_portfolio_credit(&[leg(0, 0, true, 7_000_000)], &m, &P0).unwrap();
        assert_eq!(r.total(), 0);
        assert_eq!(r.hedge_benefit_ppm, 0);
    }

    #[test]
    fn empty_portfolio_is_zero() {
        let rho = |_: u8, _: u8| 0i64;
        let m = MarginAsRiskModel { rho: &rho };
        let r = compute_portfolio_credit(&[], &m, &P0).unwrap();
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn validation_rejects_bad_legs() {
        let ok = leg(0, 0, true, 1_000);
        assert_eq!(validate_legs(&[ok, ok]), Err(PortfolioError::DuplicateLeg));
        let mut z = ok;
        z.notional = 0;
        assert_eq!(validate_legs(&[z]), Err(PortfolioError::ZeroNotional));
        let mut m = ok;
        m.maintenance_margin = m.initial_margin + 1;
        assert_eq!(validate_legs(&[m]), Err(PortfolioError::InvalidMargin));
        let mut m0 = ok;
        m0.maintenance_margin = 0;
        assert_eq!(validate_legs(&[m0]), Err(PortfolioError::InvalidMargin));
        let mut big = ok;
        big.initial_margin = big.notional + 1;
        assert_eq!(validate_legs(&[big]), Err(PortfolioError::InvalidMargin));
        let mut a = ok;
        a.asset = MAX_ASSETS as u8;
        assert_eq!(validate_legs(&[a]), Err(PortfolioError::InvalidAsset));
        let many: Vec<_> = (0..=MAX_LEGS as u8)
            .map(|v| leg(v, 0, true, 1_000))
            .collect();
        assert_eq!(validate_legs(&many), Err(PortfolioError::TooManyLegs));
        // Same asset on two venues and two assets on one venue are both fine.
        assert!(validate_legs(&[leg(0, 0, true, 1_000), leg(1, 0, false, 1_000)]).is_ok());
        assert!(validate_legs(&[leg(0, 0, true, 1_000), leg(0, 1, false, 1_000)]).is_ok());
    }

    #[test]
    fn im_minus_mm_cap_binds() {
        let rho = |_: u8, _: u8| 1_000_000i64;
        let m = MarginAsRiskModel { rho: &rho };
        let mut a = leg(0, 0, true, 10_000);
        let mut b = leg(1, 1, false, 10_000);
        a.maintenance_margin = 9_000;
        b.maintenance_margin = 6_000;
        let p = CreditParams {
            cap_at_im_minus_mm: true,
            ..P0
        };
        let r = compute_portfolio_credit(&[a, b], &m, &p).unwrap();
        assert_eq!(r.total_before_leg_caps, 20_000);
        assert_eq!(&r.leg_credit[..2], &[1_000, 4_000]);
    }

    #[test]
    fn cap_groups_scales_only_over_budget_groups() {
        let mut c = [100, 300, 50];
        let groups = [0usize, 0, 1];
        cap_groups(&mut c, &|i| groups[i], &[200, 1_000]);
        assert_eq!(c, [50, 150, 50]);
        let mut c = [10, 10];
        cap_groups(&mut c, &|_| 5, &[100]);
        assert_eq!(c, [0, 0], "unknown group fails safe");
    }

    #[test]
    fn hedged_credit_never_exceeds_benefit() {
        assert_eq!(hedged_credit(100, 100, 40, 0), 60);
        assert_eq!(hedged_credit(100, 100, 40, 5_000), 30);
        assert_eq!(hedged_credit(100, 100, 100, 0), 0);
        assert_eq!(hedged_credit(100, 0, 0, 0), 0);
        assert_eq!(hedged_credit(u64::MAX, u64::MAX, 0, 0), u64::MAX);
        assert_eq!(hedged_credit(100, 100, 0, 10_000), 0);
    }
}
