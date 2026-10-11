//! Stress suite for the Stage C risk model. Each test states whether its
//! scenario is a HISTORICAL REPLAY (derived from recorded prices) or a
//! HYPOTHETICAL stress.

use ch_math::portfolio::*;
use ch_math::risk::*;

const SOL: u8 = 0;
const ETH: u8 = 1;
const BTC: u8 = 2;

fn base_assets() -> [AssetRisk; MAX_ASSETS] {
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
    a[BTC as usize] = AssetRisk {
        shock_bps: 600,
        gap_bps: 600,
        basis_bps: 100,
    };
    a
}

fn base_rho(a: u8, b: u8) -> i64 {
    match (a.min(b), a.max(b)) {
        (0, 1) => 800_000,
        (0, 2) => 700_000,
        (1, 2) => 800_000,
        _ => 0,
    }
}

fn params(haircut_bps: u16) -> CreditParams {
    CreditParams {
        haircut_bps,
        max_credit_per_user: u64::MAX,
        max_credit_bps_of_im: 10_000,
        cap_at_im_minus_mm: true,
    }
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

struct M {
    assets: [AssetRisk; MAX_ASSETS],
    rho: fn(u8, u8) -> i64,
    delta: u32,
    basis: u32,
    scenarios: Vec<[i16; MAX_ASSETS]>,
}

impl M {
    fn base() -> Self {
        M {
            assets: base_assets(),
            rho: base_rho,
            delta: 100_000,
            basis: 20_000,
            scenarios: vec![],
        }
    }
    fn credit(
        &self,
        legs: &[PortfolioLeg],
        haircut: u16,
    ) -> Result<PortfolioCredit, PortfolioError> {
        let rho = self.rho;
        let rho_fn = move |a: u8, b: u8| rho(a, b);
        let model = StressedRiskModel {
            assets: &self.assets,
            rho: &rho_fn,
            corr_stress_ppm: self.delta,
            basis_decorr_ppm: self.basis,
            scenarios: &self.scenarios,
        };
        compute_portfolio_credit(legs, &model, &params(haircut))
    }
    fn total(&self, legs: &[PortfolioLeg]) -> u64 {
        self.credit(legs, 2_000).unwrap().total()
    }
}

const N: u64 = 50_000_000_000; // $50k notional

fn hedge() -> [PortfolioLeg; 2] {
    [leg(0, SOL, true, N), leg(1, ETH, false, N)]
}

// ----------------------------------------------------------------- HYPOTHETICAL

#[test]
fn hypothetical_favourable_and_adverse_correlation_shifts() {
    // Small gaps so the parametric term is the binding one.
    let mut m = M::base();
    for a in [SOL, ETH] {
        m.assets[a as usize].gap_bps = 1;
    }
    let at = |rho: i64| {
        let mut mm = M {
            assets: m.assets,
            rho: base_rho,
            delta: m.delta,
            basis: m.basis,
            scenarios: vec![],
        };
        // A table of fixed correlations for the shift.
        mm.rho = match rho {
            500_000 => |_, _| 500_000,
            800_000 => |_, _| 800_000,
            950_000 => |_, _| 950_000,
            _ => unreachable!(),
        };
        mm.total(&hedge())
    };
    let adverse = at(500_000);
    let base = at(800_000);
    let favourable = at(950_000);
    assert!(
        adverse < base && base < favourable,
        "{adverse} {base} {favourable}"
    );
}

#[test]
fn hypothetical_sudden_decorrelation_never_increases_credit() {
    // The stress delta models a decorrelation; any delta >= 0 can only cut credit.
    let mut prev = u64::MAX;
    for delta in [0u32, 100_000, 300_000, 800_000, 2_000_000] {
        let mut m = M::base();
        m.delta = delta;
        for a in [SOL, ETH] {
            m.assets[a as usize].gap_bps = 1;
        }
        let c = m.total(&hedge());
        assert!(c <= prev);
        prev = c;
    }
    assert_eq!(
        prev, 0,
        "full decorrelation of a cross-asset hedge leaves no credit"
    );
}

#[test]
fn hypothetical_volatility_spike_on_one_leg_cuts_credit() {
    let m = M::base();
    let calm = m.total(&hedge());
    let mut spike = M::base();
    spike.assets[SOL as usize] = AssetRisk {
        shock_bps: 3_000,
        gap_bps: 3_000,
        basis_bps: 100,
    };
    assert!(spike.total(&hedge()) < calm);
}

#[test]
fn hypothetical_price_gaps_are_monotone() {
    let mut prev = u64::MAX;
    for gap in [1_000u16, 2_000, 4_000, 8_000, 10_000] {
        let mut m = M::base();
        m.assets[SOL as usize].gap_bps = gap;
        let c = m.total(&hedge());
        assert!(c <= prev);
        prev = c;
    }
}

#[test]
fn hypothetical_hedge_leg_liquidated_first_leaves_no_credit() {
    let m = M::base();
    assert!(m.total(&hedge()) > 0);
    // The short ETH hedge is liquidated on its venue; only the long remains.
    assert_eq!(m.total(&[leg(0, SOL, true, N)]), 0);
}

#[test]
fn hypothetical_same_direction_legs_across_many_venues_get_nothing() {
    let m = M::base();
    let legs: Vec<_> = (0..MAX_LEGS as u8).map(|v| leg(v, SOL, true, N)).collect();
    assert_eq!(m.total(&legs), 0);
    let mixed: Vec<_> = (0..MAX_LEGS as u8)
        .map(|v| leg(v, [SOL, ETH, BTC][(v % 3) as usize], true, N))
        .collect();
    // Found by this test: without a market-wide scenario, rho + delta < 1
    // across assets granted ~$2,591 of "diversification" credit to this
    // all-long book. The built-in market-wide move removes it.
    assert_eq!(
        m.total(&mixed),
        0,
        "all-long book across assets is not a hedge"
    );
}

#[test]
fn hypothetical_same_asset_cross_venue_hedge_is_capped_by_basis_and_im_minus_mm() {
    let m = M::base();
    let legs = [leg(0, SOL, true, N), leg(1, SOL, false, N)];
    let r = m.credit(&legs, 2_000).unwrap();
    assert!(r.portfolio_risk > 0, "basis stress: not a perfect hedge");
    for (i, l) in legs.iter().enumerate() {
        assert!(r.leg_credit[i] <= l.initial_margin - l.maintenance_margin);
    }
}

#[test]
fn hypothetical_extreme_but_valid_values_do_not_overflow() {
    let m = M::base();
    // $1B notional per leg on 8 legs.
    let big = 1_000_000_000_000_000u64;
    let legs: Vec<_> = (0..MAX_LEGS as u8)
        .map(|v| leg(v, [SOL, ETH][(v % 2) as usize], v % 2 == 0, big))
        .collect();
    let r = m.credit(&legs, 0).unwrap();
    assert!(r.total() <= r.sum_initial_margin);
    // Beyond representable: fails closed, never panics.
    let huge = [leg(0, SOL, true, u64::MAX), leg(1, ETH, false, u64::MAX)];
    assert_eq!(m.credit(&huge, 0).err(), Some(PortfolioError::Overflow));
}

#[test]
fn hypothetical_unsupported_asset_fails_closed() {
    let m = M::base();
    let legs = [leg(0, SOL, true, N), leg(1, 7, false, N)];
    assert_eq!(
        m.credit(&legs, 0).err(),
        Some(PortfolioError::UnsupportedAsset)
    );
}

// ------------------------------------------------------------------ properties

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

fn random_book(rng: &mut Lcg) -> Vec<PortfolioLeg> {
    let n = rng.range(1, MAX_LEGS as u64) as usize;
    let mut legs: Vec<PortfolioLeg> = Vec::new();
    while legs.len() < n {
        let v = rng.range(0, 7) as u8;
        let a = rng.range(0, 2) as u8;
        if legs.iter().any(|l| l.venue_index == v && l.asset == a) {
            continue;
        }
        let notional = rng.range(1_000_000, 10_000_000_000_000);
        let im = rng.range(notional / 50 + 1, notional);
        let mm = rng.range(1, im);
        legs.push(PortfolioLeg {
            venue_index: v,
            asset: a,
            is_long: rng.next() & 1 == 0,
            notional,
            initial_margin: im,
            maintenance_margin: mm,
        });
    }
    legs
}

#[test]
fn prop_every_stress_knob_is_monotone_and_order_never_matters() {
    let mut rng = Lcg(0xC0FFEE);
    for _ in 0..2_000 {
        let legs = random_book(&mut rng);
        let base = M::base();
        let c0 = base.total(&legs);

        let mut more_delta = M::base();
        more_delta.delta = 300_000;
        assert!(more_delta.total(&legs) <= c0, "delta");

        let mut more_gap = M::base();
        for a in [SOL, ETH, BTC] {
            more_gap.assets[a as usize].gap_bps *= 2;
        }
        assert!(more_gap.total(&legs) <= c0, "gap");

        let mut more_basis = M::base();
        more_basis.basis = 200_000;
        for a in [SOL, ETH, BTC] {
            more_basis.assets[a as usize].basis_bps = 1_000;
        }
        assert!(more_basis.total(&legs) <= c0, "basis");

        let mut with_scenario = M::base();
        let mut sc = [0i16; MAX_ASSETS];
        sc[SOL as usize] = rng.range(0, 20_000) as i16 - 10_000;
        sc[ETH as usize] = rng.range(0, 20_000) as i16 - 10_000;
        with_scenario.scenarios.push(sc);
        assert!(with_scenario.total(&legs) <= c0, "scenario");

        assert!(base.credit(&legs, 5_000).unwrap().total() <= c0, "haircut");

        let mut rev = legs.clone();
        rev.reverse();
        let a = base.credit(&legs, 2_000).unwrap();
        let b = base.credit(&rev, 2_000).unwrap();
        assert_eq!(a.portfolio_risk, b.portfolio_risk);
        assert_eq!(a.total(), b.total());

        // Credit never pushes a leg below its maintenance margin, and the
        // fraction of IM still held is at least R_p / S.
        for (i, l) in legs.iter().enumerate() {
            assert!(a.leg_credit[i] <= l.initial_margin - l.maintenance_margin);
        }
        let held = (a.sum_initial_margin - a.total()) as u128;
        assert!(
            held * a.sum_standalone_risk as u128
                >= a.portfolio_risk.min(a.sum_standalone_risk) as u128
                    * a.sum_initial_margin as u128
        );
    }
}

// ------------------------------------------------------------ HISTORICAL REPLAY

/// Closes from crates/ch_math/tests/data/binance_nov2022_daily.csv
/// (Binance spot daily klines, retrieved 2026-10-11).
fn closes() -> Vec<(String, f64, f64)> {
    let raw = include_str!("data/binance_nov2022_daily.csv");
    raw.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("date"))
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (
                f[0].to_string(),
                f[1].parse().unwrap(),
                f[3].parse().unwrap(),
            )
        })
        .collect()
}

fn move_bps(from: f64, to: f64) -> i16 {
    // Truncate toward zero in magnitude terms is not conservative for a loss;
    // round away from zero so the replayed loss is never understated.
    let x = (to / from - 1.0) * 10_000.0;
    (if x < 0.0 { x.floor() } else { x.ceil() }) as i16
}

/// HISTORICAL REPLAY, multi-day cumulative: close 2022-11-02 -> close 2022-11-15.
/// Not a single margin horizon; it is the move a hedge carried through the
/// FTX collapse if nobody re-margined. Press reports (Reuters via Inquirer,
/// 15 Nov 2022) gave SOL -53.8% / ETH ~-20%; the primary series gives
/// SOL -53.51% / ETH -17.46%.
pub const NOV2022_CUMULATIVE: (i16, i16) = (-5_351, -1_747);
/// HISTORICAL REPLAY, single day: close 2022-11-08 -> close 2022-11-09.
pub const NOV2022_WORST_DAY: (i16, i16) = (-4_225, -1_739);

#[test]
fn replay_constants_are_derived_from_the_recorded_series() {
    let c = closes();
    let get = |d: &str| c.iter().find(|r| r.0 == d).unwrap().clone();
    let (_, s2, e2) = get("2022-11-02");
    let (_, s15, e15) = get("2022-11-15");
    assert_eq!((move_bps(s2, s15), move_bps(e2, e15)), NOV2022_CUMULATIVE);
    let (_, s8, e8) = get("2022-11-08");
    let (_, s9, e9) = get("2022-11-09");
    assert_eq!((move_bps(s8, s9), move_bps(e8, e9)), NOV2022_WORST_DAY);
}

fn replay_vector(moves: (i16, i16)) -> [i16; MAX_ASSETS] {
    let mut v = [0i16; MAX_ASSETS];
    v[SOL as usize] = moves.0;
    v[ETH as usize] = moves.1;
    v
}

#[test]
fn replay_nov2022_long_sol_short_eth_hedge() {
    let legs = hedge();
    // Realized loss over the cumulative window, equal $50k notionals:
    // 53.51% on the long minus 17.47% gained on the short.
    let realized = (N as i128 * (5_351 - 1_747) as i128 / 10_000) as u64;
    assert_eq!(realized, 18_020_000_000);

    // v0 (margin as risk, rho 0.8, haircut 20%) freed $10,940 of $20,000 IM:
    // margin held ($9,060) was well below the realized loss.
    let rho = |_: u8, _: u8| 800_000i64;
    let v0 = compute_portfolio_credit(
        &legs,
        &MarginAsRiskModel { rho: &rho },
        &CreditParams {
            haircut_bps: 2_000,
            max_credit_per_user: u64::MAX,
            max_credit_bps_of_im: 10_000,
            cap_at_im_minus_mm: false,
        },
    )
    .unwrap();
    assert_eq!(v0.total_before_leg_caps, 10_940_355_743);
    assert!(v0.sum_initial_margin - v0.total() < realized);

    // Stressed model without the replay: some credit, capped at IM - MM.
    let m = M::base();
    let without = m.credit(&legs, 2_000).unwrap();
    assert!(without.total() > 0 && without.total() < v0.total());

    // With either replay in the governance scenario set, the replay loss
    // exceeds the sum of standalone risks and the hedge earns nothing.
    for moves in [NOV2022_CUMULATIVE, NOV2022_WORST_DAY] {
        let mut m = M::base();
        m.scenarios.push(replay_vector(moves));
        let r = m.credit(&legs, 2_000).unwrap();
        assert!(r.portfolio_risk >= r.sum_standalone_risk);
        assert_eq!(r.total(), 0);
    }
}
