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
//! * `R_p = max(parametric, worst scenario loss)`, or, when the base
//!   correlation sub-matrix over the assets present is not positive definite
//!   (A4, crate::psd), `R_p = max(S, worst scenario loss)` (no hedge credit).
//!
//! All arithmetic is integer; risk and losses round up.

use crate::isqrt_ceil;
use crate::portfolio::{PortfolioError, PortfolioLeg, RiskModel, MAX_ASSETS, MAX_LEGS};
use crate::psd::corr_is_positive_definite;

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
    /// A4: is the base correlation sub-matrix over the distinct assets of
    /// `legs` positive definite (crate::psd)? Assets are sorted first, so the
    /// answer never depends on leg order. Callers bound `legs` to MAX_LEGS.
    pub fn correlation_is_positive_definite(&self, legs: &[PortfolioLeg]) -> bool {
        let mut assets = [0u8; MAX_LEGS];
        let mut n = 0;
        for l in legs.iter().take(MAX_LEGS) {
            if !assets[..n].contains(&l.asset) {
                assets[n] = l.asset;
                n += 1;
            }
        }
        assets[..n].sort_unstable();
        corr_is_positive_definite(&assets[..n], self.rho)
    }

    /// Σ standalone risk (the ρ-free fallback for the parametric term).
    fn sum_standalone(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError> {
        let mut s = 0u64;
        for l in legs {
            s = s
                .checked_add(self.standalone_risk(l)?)
                .ok_or(PortfolioError::Overflow)?;
        }
        Ok(s)
    }

    /// Parametric term, or (A4) Σ standalone risk when the correlation
    /// sub-matrix is not positive definite. The bool reports the fallback.
    fn parametric_or_fallback(&self, legs: &[PortfolioLeg]) -> Result<(u64, bool), PortfolioError> {
        if legs.len() > MAX_LEGS {
            return Err(PortfolioError::TooManyLegs);
        }
        if self.correlation_is_positive_definite(legs) {
            Ok((self.parametric_risk(legs)?, false))
        } else {
            Ok((self.sum_standalone(legs)?, true))
        }
    }

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

/// Hedge-break scenario (M3): every leg moves adversely at once (longs down,
/// shorts up) by `shock_bps(asset) · hedge_break_bps / 10_000`.
///
/// At `hedge_break_bps = 10_000` the loss equals Σ standalone risk exactly,
/// so the hedge benefit is 0 for *every* portfolio and no credit is ever
/// granted. The parameter is therefore a fraction, bounded to
/// `[HEDGE_BREAK_MIN_BPS, HEDGE_BREAK_MAX_BPS]` when enabled; 0 disables it.
/// See docs/RISK_MODEL.md (sensitivity table).
pub const HEDGE_BREAK_MAX_BPS: u16 = 10_000;
/// Lowest value governance may set once the scenario is enabled.
pub const HEDGE_BREAK_MIN_BPS: u16 = 2_500;
/// Recommended default (half the shock on every leg at once): scripts and
/// fixtures use it. On-chain it is `ConfigParams::hedge_break_bps` (A3),
/// governed; calibration on the committed data suggests ~3,046, so 5,000 is
/// the more conservative choice (docs/BACKTEST.md, B1).
pub const HEDGE_BREAK_DEFAULT_BPS: u16 = 5_000;

/// 0 (disabled) or within `[HEDGE_BREAK_MIN_BPS, HEDGE_BREAK_MAX_BPS]`.
pub fn hedge_break_bps_is_valid(bps: u16) -> bool {
    bps == 0 || (HEDGE_BREAK_MIN_BPS..=HEDGE_BREAK_MAX_BPS).contains(&bps)
}

impl StressedRiskModel<'_> {
    /// Loss when every leg moves adversely by `ceil(shock · h / 10_000)` bps.
    /// Each leg's loss is rounded up separately, so at `h = 10_000` the loss
    /// is exactly Σ standalone risk. Values above `HEDGE_BREAK_MAX_BPS` are
    /// treated as the maximum.
    pub fn hedge_break_loss(
        &self,
        legs: &[PortfolioLeg],
        hedge_break_bps: u16,
    ) -> Result<u64, PortfolioError> {
        let h = hedge_break_bps.min(HEDGE_BREAK_MAX_BPS) as u32;
        let mut loss = 0u64;
        for leg in legs {
            let shock = asset(self, leg.asset)?.shock_bps as u32;
            // ≤ 10_000, so it fits the i16 move range used elsewhere.
            let m = (shock * h).div_ceil(10_000) as u16;
            let l =
                u64::try_from(bps_ceil(leg.notional, m)).map_err(|_| PortfolioError::Overflow)?;
            loss = loss.checked_add(l).ok_or(PortfolioError::Overflow)?;
        }
        Ok(loss)
    }
}

/// `StressedRiskModel` plus the hedge-break scenario:
/// `R_p = max(parametric, built-in and governance scenarios, hedge break)`.
pub struct HedgeBreakModel<'a> {
    pub inner: StressedRiskModel<'a>,
    pub hedge_break_bps: u16,
}

impl HedgeBreakModel<'_> {
    /// Worst scenario loss including the hedge break.
    pub fn scenario_risk(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError> {
        let s = self.inner.scenario_risk(legs)?;
        Ok(s.max(self.inner.hedge_break_loss(legs, self.hedge_break_bps)?))
    }
}

impl RiskModel for HedgeBreakModel<'_> {
    fn standalone_risk(&self, leg: &PortfolioLeg) -> Result<u64, PortfolioError> {
        self.inner.standalone_risk(leg)
    }

    fn portfolio_risk(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError> {
        Ok(self.portfolio_risk_report(legs)?.0)
    }

    fn portfolio_risk_report(&self, legs: &[PortfolioLeg]) -> Result<(u64, bool), PortfolioError> {
        let (p, fallback) = self.inner.parametric_or_fallback(legs)?;
        Ok((p.max(self.scenario_risk(legs)?), fallback))
    }
}

impl RiskModel for StressedRiskModel<'_> {
    fn standalone_risk(&self, leg: &PortfolioLeg) -> Result<u64, PortfolioError> {
        let r = asset(self, leg.asset)?;
        u64::try_from(bps_ceil(leg.notional, r.shock_bps)).map_err(|_| PortfolioError::Overflow)
    }

    fn portfolio_risk(&self, legs: &[PortfolioLeg]) -> Result<u64, PortfolioError> {
        Ok(self.portfolio_risk_report(legs)?.0)
    }

    /// `R_p = max(parametric, scenarios)`; if the correlation sub-matrix is
    /// not positive definite (A4), `R_p = max(S, scenarios) ≥ S`, so the
    /// hedge benefit, and with it the credit, is 0.
    fn portfolio_risk_report(&self, legs: &[PortfolioLeg]) -> Result<(u64, bool), PortfolioError> {
        let (p, fallback) = self.parametric_or_fallback(legs)?;
        let s = self.scenario_risk(legs)?;
        Ok((p.max(s), fallback))
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
    fn hedge_break_at_full_shock_equals_sum_of_standalone_risk_so_credit_is_zero() {
        let a = assets();
        let inner = StressedRiskModel {
            assets: &a,
            rho: &|_, _| 800_000,
            corr_stress_ppm: 100_000,
            basis_decorr_ppm: 20_000,
            scenarios: &[],
        };
        // Uneven, non-round notionals: per-leg rounding must still give exactly S.
        let legs = [
            leg(0, 0, true, 49_999_999_999),
            leg(1, 1, false, 37_123_456_789),
        ];
        let s: u64 = legs.iter().map(|l| inner.standalone_risk(l).unwrap()).sum();
        assert_eq!(inner.hedge_break_loss(&legs, 10_000).unwrap(), s);
        assert_eq!(
            inner.hedge_break_loss(&legs, u16::MAX).unwrap(),
            s,
            "capped at max"
        );
        assert_eq!(inner.hedge_break_loss(&legs, 0).unwrap(), 0, "0 disables");
        let m = HedgeBreakModel {
            inner,
            hedge_break_bps: 10_000,
        };
        let r = compute_portfolio_credit(
            &legs,
            &m,
            &CreditParams {
                haircut_bps: 0,
                max_credit_per_user: u64::MAX,
                max_credit_bps_of_im: 10_000,
                cap_at_im_minus_mm: false,
            },
        )
        .unwrap();
        assert_eq!((r.hedge_benefit_ppm, r.total()), (0, 0));
    }

    #[test]
    fn hedge_break_moves_round_up_and_disabled_model_is_unchanged() {
        let a = assets();
        let make = || StressedRiskModel {
            assets: &a,
            rho: &|_, _| 800_000,
            corr_stress_ppm: 100_000,
            basis_decorr_ppm: 20_000,
            scenarios: &[],
        };
        let n = 50_000_000_000u64;
        let legs = [leg(0, 0, true, n), leg(1, 1, false, n)];
        // SOL 1000 bps × 3333/10000 = 333.3 → 334 bps; ETH 800 × 0.3333 = 266.64 → 267.
        assert_eq!(
            make().hedge_break_loss(&legs, 3_333).unwrap(),
            n * 334 / 10_000 + n * 267 / 10_000
        );
        let plain = make();
        let wrapped = HedgeBreakModel {
            inner: make(),
            hedge_break_bps: 0,
        };
        assert_eq!(
            plain.portfolio_risk(&legs).unwrap(),
            wrapped.portfolio_risk(&legs).unwrap()
        );
        assert!(hedge_break_bps_is_valid(0));
        assert!(hedge_break_bps_is_valid(HEDGE_BREAK_MIN_BPS));
        assert!(hedge_break_bps_is_valid(HEDGE_BREAK_MAX_BPS));
        assert!(!hedge_break_bps_is_valid(HEDGE_BREAK_MIN_BPS - 1));
        assert!(!hedge_break_bps_is_valid(HEDGE_BREAK_MAX_BPS + 1));
        assert!(hedge_break_bps_is_valid(HEDGE_BREAK_DEFAULT_BPS));
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
