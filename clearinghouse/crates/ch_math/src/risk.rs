//! Stressed portfolio risk model (Stage C). See docs/RISK_MODEL.md.
//!
//! * Standalone risk of a leg: `R_i = ceil(|notional_i| · shock_bps(asset) / 10_000)`,
//!   the loss of that leg alone under a one-horizon adverse move.
//! * Parametric portfolio risk with *direction-aware* correlation stress:
//!   for two legs on different assets, use `ρ − δ` when their exposures offset
//!   (a hedge) and `ρ + δ` when they point the same way, clamped to [−1, 1].
//!   Every cross term can only grow, so decorrelation hurts hedges and
//!   correlation spikes hurt same-direction books.
//! * Same-asset legs on different venues are not assumed to be perfectly
//!   correlated when they offset: `ρ = 1 − basis_decorr`. Same-direction
//!   same-asset legs use ρ = 1 (fully additive, the worst case).
//! * Deterministic scenarios, each a loss computed exactly from notionals:
//!   - market-wide: every asset moves by its own shock_bps in the same
//!     direction (down, then up). A book with no offsetting exposure loses
//!     the full sum of standalone risks here, so it never earns credit from
//!     mere diversification;
//!   - a single-asset idiosyncratic gap of ±gap_bps for every asset present;
//!   - a venue-basis gap of ±basis_bps on each leg whose asset is also held on
//!     another venue (that leg alone moves);
//!   - governance-defined shock vectors (historical replays or hypotheticals).
//! * `R_p = max(parametric, worst scenario loss)`.
//!
//! All arithmetic is integer; risk and losses round up.

use crate::isqrt_ceil;
use crate::portfolio::{PortfolioError, PortfolioLeg, RiskModel, MAX_ASSETS, MAX_LEGS};

/// Governance scenarios per risk model.
pub const MAX_SCENARIOS: usize = 8;
pub const PPM: i128 = 1_000_000;

/// Scenario provenance, so a replay is never mistaken for a forecast.
pub mod scenario_kind {
    pub const UNUSED: u8 = 0;
    /// Moves measured from a historical price series (record the source).
    pub const HISTORICAL_REPLAY: u8 = 1;
    /// A stress chosen by the risk authority.
    pub const HYPOTHETICAL: u8 = 2;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssetRisk {
    /// One-horizon adverse move used for standalone risk. 0 = asset unsupported.
    pub shock_bps: u16,
    /// Idiosyncratic gap: this asset moves alone by ±gap_bps.
    pub gap_bps: u16,
    /// Venue basis: one venue's mark for this asset moves alone by ±basis_bps.
    pub basis_bps: u16,
}

pub struct StressedRiskModel<'a> {
    pub assets: &'a [AssetRisk; MAX_ASSETS],
    /// Base asset correlation, scaled 1e6.
    pub rho: &'a dyn Fn(u8, u8) -> i64,
    /// δ, scaled 1e6.
    pub corr_stress_ppm: u32,
    /// Decorrelation of offsetting same-asset legs on different venues, scaled 1e6.
    pub basis_decorr_ppm: u32,
    /// Active governance scenarios: per-asset signed moves in bps.
    pub scenarios: &'a [[i16; MAX_ASSETS]],
}

fn asset(model: &StressedRiskModel, a: u8) -> Result<AssetRisk, PortfolioError> {
    let r = *model
        .assets
        .get(a as usize)
        .ok_or(PortfolioError::InvalidAsset)?;
    if r.shock_bps == 0 {
        return Err(PortfolioError::UnsupportedAsset);
    }
    Ok(r)
}

/// ceil(n · bps / 10_000) in u128.
fn bps_ceil(n: u64, bps: u16) -> u128 {
    let p = (n as u128) * (bps as u128);
    p.div_ceil(10_000)
}

/// Loss (≥ 0, rounded up) of a set of legs under per-leg moves in bps.
/// `move_of(i)` is the signed move of leg i's mark.
fn scenario_loss(
    legs: &[PortfolioLeg],
    move_of: &dyn Fn(usize) -> i16,
) -> Result<u64, PortfolioError> {
    // P&L in units of micro-USD · bps. |notional| < 2^64, |move| <= 2^15, so a
    // term fits in i128 with room for 8 legs.
    let mut pnl: i128 = 0;
    for (i, leg) in legs.iter().enumerate() {
        let m = move_of(i) as i128;
        let side: i128 = if leg.is_long { 1 } else { -1 };
        pnl = pnl
            .checked_add(side * (leg.notional as i128) * m)
            .ok_or(PortfolioError::Overflow)?;
    }
    if pnl >= 0 {
        return Ok(0);
    }
    let loss = (-pnl) as u128;
    u64::try_from(loss.div_ceil(10_000)).map_err(|_| PortfolioError::Overflow)
}

impl StressedRiskModel<'_> {
    fn stressed_rho(&self, a: &PortfolioLeg, b: &PortfolioLeg, same_sign: bool) -> i128 {
        if a.asset == b.asset {
            return if same_sign {
                PPM
            } else {
                PPM - (self.basis_decorr_ppm as i128).min(PPM)
            };
        }
        let base = ((self.rho)(a.asset, b.asset) as i128).clamp(-PPM, PPM);
        let d = (self.corr_stress_ppm as i128).min(2 * PPM);
        let r = if same_sign { base + d } else { base - d };
        r.clamp(-PPM, PPM)
    }

    /// Parametric risk with direction-aware stressed correlations.
    pub fn parametric_risk(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError> {
        let mut x = [0i128; MAX_LEGS];
        if legs.len() > MAX_LEGS {
            return Err(PortfolioError::TooManyLegs);
        }
        for (xi, leg) in x.iter_mut().zip(legs) {
            let r = self.standalone_risk(leg)? as i128;
            *xi = if leg.is_long { r } else { -r };
        }
        let mut cov: i128 = 0;
        for i in 0..legs.len() {
            for j in 0..legs.len() {
                let rho = if i == j {
                    PPM
                } else {
                    self.stressed_rho(&legs[i], &legs[j], (x[i] >= 0) == (x[j] >= 0))
                };
                let term = x[i]
                    .checked_mul(x[j])
                    .and_then(|v| v.checked_mul(rho))
                    .ok_or(PortfolioError::Overflow)?;
                cov = cov.checked_add(term).ok_or(PortfolioError::Overflow)?;
            }
        }
        if cov < 0 {
            return Err(PortfolioError::NegativeVariance);
        }
        let var = (cov as u128).div_ceil(PPM as u128);
        u64::try_from(isqrt_ceil(var)).map_err(|_| PortfolioError::Overflow)
    }

    /// Worst loss over built-in gap/basis scenarios and governance scenarios.
    pub fn scenario_risk(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError> {
        let mut worst = 0u64;

        // Market-wide move: every asset by its own shock, same direction.
        for dir in [-1i16, 1] {
            let mut shocks = [0i16; MAX_LEGS];
            for (s, l) in shocks.iter_mut().zip(legs) {
                *s = dir * asset(self, l.asset)?.shock_bps as i16;
            }
            worst = worst.max(scenario_loss(legs, &|i| shocks[i])?);
        }

        // Single-asset idiosyncratic gaps.
        for a in legs.iter().map(|l| l.asset) {
            let g = asset(self, a)?.gap_bps as i16;
            for s in [g, -g] {
                let l = scenario_loss(legs, &|i| if legs[i].asset == a { s } else { 0 })?;
                worst = worst.max(l);
            }
        }

        // Venue basis: one leg's mark moves alone when its asset is held elsewhere too.
        for (k, leg) in legs.iter().enumerate() {
            if !legs
                .iter()
                .enumerate()
                .any(|(j, o)| j != k && o.asset == leg.asset)
            {
                continue;
            }
            let b = asset(self, leg.asset)?.basis_bps as i16;
            for s in [b, -b] {
                let l = scenario_loss(legs, &|i| if i == k { s } else { 0 })?;
                worst = worst.max(l);
            }
        }

        // Governance shock vectors.
        for sc in self.scenarios {
            let l = scenario_loss(legs, &|i| sc[legs[i].asset as usize])?;
            worst = worst.max(l);
        }
        Ok(worst)
    }
}

impl RiskModel for StressedRiskModel<'_> {
    fn standalone_risk(&self, leg: &PortfolioLeg) -> Result<u64, PortfolioError> {
        let r = asset(self, leg.asset)?;
        u64::try_from(bps_ceil(leg.notional, r.shock_bps)).map_err(|_| PortfolioError::Overflow)
    }

    fn portfolio_risk(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError> {
        let p = self.parametric_risk(legs)?;
        let s = self.scenario_risk(legs)?;
        Ok(p.max(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{compute_portfolio_credit, CreditParams};

    fn assets() -> [AssetRisk; MAX_ASSETS] {
        let mut a = [AssetRisk::default(); MAX_ASSETS];
        a[0] = AssetRisk {
            shock_bps: 1_000,
            gap_bps: 1_000,
            basis_bps: 100,
        };
        a[1] = AssetRisk {
            shock_bps: 800,
            gap_bps: 800,
            basis_bps: 100,
        };
        a
    }

    fn leg(v: u8, a: u8, long: bool, notional: u64) -> PortfolioLeg {
        PortfolioLeg {
            venue_index: v,
            asset: a,
            is_long: long,
            notional,
            initial_margin: notional / 5,
            maintenance_margin: notional / 10,
        }
    }

    #[test]
    fn standalone_risk_rounds_up() {
        let a = assets();
        let m = StressedRiskModel {
            assets: &a,
            rho: &|_, _| 0,
            corr_stress_ppm: 0,
            basis_decorr_ppm: 0,
            scenarios: &[],
        };
        assert_eq!(m.standalone_risk(&leg(0, 0, true, 50_000)).unwrap(), 5_000);
        assert_eq!(m.standalone_risk(&leg(0, 0, true, 1)).unwrap(), 1);
        assert_eq!(
            m.standalone_risk(&leg(0, 5, true, 1)),
            Err(PortfolioError::UnsupportedAsset)
        );
    }

    #[test]
    fn worked_example_from_risk_model_doc() {
        // Long SOL $50k (shock 10%), short ETH $50k (shock 8%), rho 0.8, delta 0.1.
        let a = assets();
        let rho = |_: u8, _: u8| 800_000i64;
        let m = StressedRiskModel {
            assets: &a,
            rho: &rho,
            corr_stress_ppm: 100_000,
            basis_decorr_ppm: 20_000,
            scenarios: &[],
        };
        let n = 50_000_000_000u64;
        let legs = [leg(0, 0, true, n), leg(1, 1, false, n)];
        // x = (5e9, -4e9); rho' = 0.7; V = 25 + 16 - 2*0.7*20 = 13 (e18).
        assert_eq!(m.parametric_risk(&legs).unwrap(), 3_605_551_276);
        // Worst gap: SOL alone -10% => 5e9.
        assert_eq!(m.scenario_risk(&legs).unwrap(), 5_000_000_000);
        let r = compute_portfolio_credit(
            &legs,
            &m,
            &CreditParams {
                haircut_bps: 2_000,
                max_credit_per_user: u64::MAX,
                max_credit_bps_of_im: 10_000,
                cap_at_im_minus_mm: true,
            },
        )
        .unwrap();
        assert_eq!(r.sum_standalone_risk, 9_000_000_000);
        assert_eq!(r.portfolio_risk, 5_000_000_000);
        // floor(20e9 * 4e9 / 9e9) = 8_888_888_888; * 0.8 = 7_111_111_110.
        assert_eq!(r.total_before_leg_caps, 7_111_111_110);
    }

    #[test]
    fn decorrelation_hurts_hedges_and_correlation_spikes_hurt_same_direction() {
        let a = assets();
        let rho = |_: u8, _: u8| 800_000i64;
        let model = |d: u32| StressedRiskModel {
            assets: &a,
            rho: &rho,
            corr_stress_ppm: d,
            basis_decorr_ppm: 0,
            scenarios: &[],
        };
        let n = 50_000_000_000u64;
        let hedge = [leg(0, 0, true, n), leg(1, 1, false, n)];
        let same = [leg(0, 0, true, n), leg(1, 1, true, n)];
        let mut prev_h = 0;
        let mut prev_s = 0;
        for d in [0u32, 50_000, 100_000, 300_000, 1_000_000] {
            let h = model(d).parametric_risk(&hedge).unwrap();
            let s = model(d).parametric_risk(&same).unwrap();
            assert!(h >= prev_h && s >= prev_s, "stress must never reduce risk");
            prev_h = h;
            prev_s = s;
        }
    }

    #[test]
    fn single_leg_is_never_credited_even_with_large_gaps() {
        let mut a = assets();
        a[0].gap_bps = 5_000;
        let m = StressedRiskModel {
            assets: &a,
            rho: &|_, _| 0,
            corr_stress_ppm: 100_000,
            basis_decorr_ppm: 20_000,
            scenarios: &[],
        };
        let legs = [leg(0, 0, true, 1_000_000_000)];
        let r = compute_portfolio_credit(
            &legs,
            &m,
            &CreditParams {
                haircut_bps: 0,
                max_credit_per_user: u64::MAX,
                max_credit_bps_of_im: 10_000,
                cap_at_im_minus_mm: true,
            },
        )
        .unwrap();
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn same_asset_offsetting_legs_are_not_a_perfect_hedge() {
        let a = assets();
        let m = StressedRiskModel {
            assets: &a,
            rho: &|_, _| 0,
            corr_stress_ppm: 0,
            basis_decorr_ppm: 20_000,
            scenarios: &[],
        };
        let n = 50_000_000_000u64;
        let legs = [leg(0, 0, true, n), leg(1, 0, false, n)];
        // Parametric: sqrt(2 * 0.02) * 5e9 = 1e9; basis scenario: 1% of 50e9 = 5e8.
        assert_eq!(m.parametric_risk(&legs).unwrap(), 1_000_000_000);
        assert_eq!(m.scenario_risk(&legs).unwrap(), 500_000_000);
        assert!(m.portfolio_risk(&legs).unwrap() > 0);
    }

    #[test]
    fn governance_scenario_drives_risk_when_worse() {
        let a = assets();
        let mut sc = [0i16; MAX_ASSETS];
        sc[0] = -5_000; // SOL -50%
        sc[1] = -2_000; // ETH -20%
        let scen = [sc];
        let m = StressedRiskModel {
            assets: &a,
            rho: &|_, _| 800_000,
            corr_stress_ppm: 100_000,
            basis_decorr_ppm: 20_000,
            scenarios: &scen,
        };
        let n = 50_000_000_000u64;
        let legs = [leg(0, 0, true, n), leg(1, 1, false, n)];
        // Long SOL loses 25e9, short ETH gains 10e9: net 15e9.
        assert_eq!(m.scenario_risk(&legs).unwrap(), 15_000_000_000);
    }
}
