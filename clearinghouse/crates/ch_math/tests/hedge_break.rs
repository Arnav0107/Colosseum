//! M3: hedge-break scenario sensitivity (HYPOTHETICAL stress).
//!
//!     cargo test -p ch_math --test hedge_break -- --nocapture
//!
//! prints the before/after tables quoted in docs/RISK_MODEL.md §3a and asserts
//! every number in them.

use ch_math::portfolio::*;
use ch_math::risk::*;

const SOL: u8 = 0;
const ETH: u8 = 1;
const USD: u64 = 1_000_000;

fn assets() -> [AssetRisk; MAX_ASSETS] {
    let mut a = [AssetRisk::default(); MAX_ASSETS];
    a[SOL as usize] = AssetRisk {
        shock_bps: 1_000,
        gap_bps: 1_000,
        basis_bps: 100,
    };
    a[ETH as usize] = AssetRisk {
        shock_bps: 800,
        gap_bps: 800,
        basis_bps: 100,
    };
    a
}

fn rho(a: u8, b: u8) -> i64 {
    if (a.min(b), a.max(b)) == (SOL, ETH) {
        800_000
    } else {
        0
    }
}

/// The current localnet / test parameters (`ch_svm_tests::setup::default_params`).
const PARAMS: CreditParams = CreditParams {
    haircut_bps: 2_000,
    max_credit_per_user: 100_000 * USD,
    max_credit_bps_of_im: 7_500,
    cap_at_im_minus_mm: true,
};

/// $50k notional, IM 20% ($10k), MM 10% ($5k).
fn leg(v: u8, a: u8, long: bool) -> PortfolioLeg {
    PortfolioLeg {
        venue_index: v,
        asset: a,
        is_long: long,
        notional: 50_000 * USD,
        initial_margin: 10_000 * USD,
        maintenance_margin: 5_000 * USD,
    }
}

struct Row {
    h: u16,
    portfolio_risk: u64,
    f_ppm: u32,
    total: u64,
    extra_size: u64,
}

/// Extra notional the freed margin carries at the venue's own IM rate:
/// Σ min(credit_i, IM_i − MM_i) · notional_i / IM_i (first order: the
/// credit itself would be recomputed after the new position is reported).
fn max_extra_size(legs: &[PortfolioLeg], c: &PortfolioCredit) -> u64 {
    legs.iter()
        .zip(c.leg_credit.iter())
        .map(|(l, &credit)| {
            let freed = credit.min(l.initial_margin - l.maintenance_margin) as u128;
            (freed * l.notional as u128 / l.initial_margin as u128) as u64
        })
        .sum()
}

fn row(legs: &[PortfolioLeg], h: u16) -> Row {
    let a = assets();
    let m = HedgeBreakModel {
        inner: StressedRiskModel {
            assets: &a,
            rho: &rho,
            corr_stress_ppm: 100_000,
            basis_decorr_ppm: 20_000,
            scenarios: &[],
        },
        hedge_break_bps: h,
    };
    let c = compute_portfolio_credit(legs, &m, &PARAMS).unwrap();
    Row {
        h,
        portfolio_risk: c.portfolio_risk,
        f_ppm: c.hedge_benefit_ppm,
        total: c.total(),
        extra_size: max_extra_size(legs, &c),
    }
}

fn usd(micro: u64) -> String {
    let d = micro / USD;
    let c = (micro % USD) / 10_000;
    let s = d.to_string();
    let mut g = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            g.push(',');
        }
        g.push(ch);
    }
    format!("${g}.{c:02}")
}

fn print(title: &str, rows: &[Row]) {
    println!("\n{title}\n| hedge_break_bps | R_p | f | total credit | max extra size |\n| ---: | ---: | ---: | ---: | ---: |");
    for r in rows {
        println!(
            "| {} | {} | {:.2}% | {} | {} |",
            r.h,
            usd(r.portfolio_risk),
            r.f_ppm as f64 / 10_000.0,
            usd(r.total),
            usd(r.extra_size)
        );
    }
}

const HS: [u16; 9] = [0, 2_500, 5_000, 5_556, 6_000, 7_000, 7_500, 9_000, 10_000];

#[test]
fn worked_example_sensitivity_long_sol_short_eth() {
    let legs = [leg(0, SOL, true), leg(1, ETH, false)];
    let rows: Vec<Row> = HS.iter().map(|&h| row(&legs, h)).collect();
    print(
        "Worked example: long SOL $50k (venue 0) / short ETH $50k (venue 1)",
        &rows,
    );
    let total = |h: u16| rows.iter().find(|r| r.h == h).unwrap().total;
    // Below ~55.6% the SOL gap scenario ($5,000) still dominates: unchanged.
    for h in [0, 2_500, 5_000] {
        assert_eq!(total(h), 7_111_111_110, "h = {h}");
    }
    // 7500: loss = 0.75 × $9,000 = $6,750 → f = 25% → floor(20k·2.25/9)·0.8 = $4,000.
    assert_eq!(total(7_500), 4_000_000_000);
    // Full shock: loss = S, f = 0, no credit at all.
    assert_eq!(total(10_000), 0);
    assert_eq!(rows.last().unwrap().f_ppm, 0);
    // Credit never increases as h grows.
    assert!(rows.windows(2).all(|w| w[1].total <= w[0].total));
    // Max extra size at h = 0: $3,555.555555 freed per leg at a 20% IM rate.
    assert_eq!(rows[0].extra_size, 35_555_555_550);
}

#[test]
fn same_asset_basis_trade_sensitivity() {
    // Long SOL on venue 0, short SOL on venue 1: hedge break moves each venue's
    // mark against its leg, which no basis scenario of 1% comes close to.
    let legs = [leg(0, SOL, true), leg(1, SOL, false)];
    let rows: Vec<Row> = HS.iter().map(|&h| row(&legs, h)).collect();
    print(
        "Basis trade: long SOL $50k (venue 0) / short SOL $50k (venue 1)",
        &rows,
    );
    let total = |h: u16| rows.iter().find(|r| r.h == h).unwrap().total;
    // h = 0: parametric sqrt(2·0.02)·$5k = $1,000 dominates; f = 90% → leg caps (IM − MM) bind.
    assert_eq!(total(0), 10_000_000_000);
    // 5000: loss = 2 × 5% × $50k = $5,000 → f = 50% → $8,000.
    assert_eq!(total(5_000), 8_000_000_000);
    assert_eq!(total(10_000), 0);
    assert!(rows.windows(2).all(|w| w[1].total <= w[0].total));
}

#[test]
fn hedge_break_bounds_credit_for_any_correlation() {
    // Whatever ρ says, R_p ≥ hedge-break loss ≥ S·h/10_000, so
    // credit ≤ ΣIM · (1 − h/10_000) · (1 − haircut). Checked against ρ = −1
    // (the most hedge-flattering correlation for a long/short pair).
    let a = assets();
    let legs = [leg(0, SOL, true), leg(1, ETH, false)];
    for h in [2_500u16, 5_000, 7_500] {
        let m = HedgeBreakModel {
            inner: StressedRiskModel {
                assets: &a,
                rho: &|_, _| -1_000_000,
                corr_stress_ppm: 50_000,
                basis_decorr_ppm: 10_000,
                scenarios: &[],
            },
            hedge_break_bps: h,
        };
        let c = compute_portfolio_credit(&legs, &m, &PARAMS).unwrap();
        let sum_im = 20_000 * USD as u128;
        let bound =
            sum_im * (10_000 - h as u128) / 10_000 * (10_000 - PARAMS.haircut_bps as u128) / 10_000;
        assert!(
            c.total() as u128 <= bound,
            "h = {h}: {} > {bound}",
            c.total()
        );
    }
}
