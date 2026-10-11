//! Property and regression tests for the joint portfolio engine.
//! Zero external dependencies: a deterministic LCG drives the generators.

use ch_math::portfolio::*;
use ch_math::{combined_risk, credit_total, split_pro_rata, Leg};

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
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.range(0, i as u64) as usize;
            v.swap(i, j);
        }
    }
}

/// Symmetric correlation table for 8 assets, built from a seed.
struct Corr([[i64; 8]; 8]);
impl Corr {
    fn random(rng: &mut Lcg) -> Self {
        let mut m = [[0i64; 8]; 8];
        let mut upper = [[0i64; 8]; 8];
        for (i, row) in upper.iter_mut().enumerate() {
            for v in row.iter_mut().skip(i + 1) {
                // Keep it mildly correlated so random matrices are usually PSD.
                *v = rng.range(0, 600_000) as i64 - 100_000;
            }
        }
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = match i.cmp(&j) {
                    std::cmp::Ordering::Equal => 1_000_000,
                    std::cmp::Ordering::Less => upper[i][j],
                    std::cmp::Ordering::Greater => upper[j][i],
                };
            }
        }
        Corr(m)
    }
}

fn random_legs(rng: &mut Lcg, n: usize) -> Vec<PortfolioLeg> {
    let mut legs = Vec::new();
    let mut used = std::collections::HashSet::new();
    while legs.len() < n {
        let venue = rng.range(0, 7) as u8;
        let asset = rng.range(0, 7) as u8;
        if !used.insert((venue, asset)) {
            continue;
        }
        let notional = rng.range(1_000_000, 1_000_000_000_000);
        let im = rng.range(1, notional);
        let mm = rng.range(1, im);
        legs.push(PortfolioLeg {
            venue_index: venue,
            asset,
            is_long: rng.next().is_multiple_of(2),
            notional,
            initial_margin: im,
            maintenance_margin: mm,
        });
    }
    legs
}

fn random_params(rng: &mut Lcg) -> CreditParams {
    CreditParams {
        haircut_bps: rng.range(0, 10_000) as u16,
        max_credit_per_user: rng.range(0, 5_000_000_000_000),
        max_credit_bps_of_im: rng.range(0, 10_000) as u16,
        cap_at_im_minus_mm: rng.next().is_multiple_of(2),
    }
}

#[test]
fn prop_leg_order_never_changes_any_result() {
    let mut rng = Lcg(0x5EED_0001);
    for _ in 0..3_000 {
        let corr = Corr::random(&mut rng);
        let rho = |a: u8, b: u8| corr.0[a as usize][b as usize];
        let model = MarginAsRiskModel { rho: &rho };
        let n = rng.range(1, MAX_LEGS as u64) as usize;
        let legs = random_legs(&mut rng, n);
        let params = random_params(&mut rng);
        let base = compute_portfolio_credit(&legs, &model, &params);

        let mut idx: Vec<usize> = (0..n).collect();
        rng.shuffle(&mut idx);
        let permuted: Vec<_> = idx.iter().map(|&i| legs[i]).collect();
        let other = compute_portfolio_credit(&permuted, &model, &params);

        match (base, other) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a.portfolio_risk, b.portfolio_risk);
                assert_eq!(a.sum_standalone_risk, b.sum_standalone_risk);
                assert_eq!(a.total_before_leg_caps, b.total_before_leg_caps);
                for (k, &i) in idx.iter().enumerate() {
                    assert_eq!(
                        b.leg_credit[k], a.leg_credit[i],
                        "leg credit follows its leg"
                    );
                }
                assert_eq!(a.total(), b.total());
            }
            (Err(a), Err(b)) => assert_eq!(a, b),
            (a, b) => panic!("order changed success/failure: {a:?} vs {b:?}"),
        }
    }
}

#[test]
fn prop_credit_respects_every_cap_and_never_exceeds_hedge_benefit() {
    let mut rng = Lcg(0x5EED_0002);
    let mut checked = 0;
    for _ in 0..5_000 {
        let corr = Corr::random(&mut rng);
        let rho = |a: u8, b: u8| corr.0[a as usize][b as usize];
        let model = MarginAsRiskModel { rho: &rho };
        let n = rng.range(1, MAX_LEGS as u64) as usize;
        let legs = random_legs(&mut rng, n);
        let params = random_params(&mut rng);
        let Ok(r) = compute_portfolio_credit(&legs, &model, &params) else {
            continue;
        };
        checked += 1;
        let total = r.total();
        assert!(total <= r.total_before_leg_caps);
        assert!(total <= params.max_credit_per_user);
        for (i, leg) in legs.iter().enumerate() {
            let c = r.leg_credit[i];
            assert!(c <= leg.initial_margin);
            assert!(
                (c as u128) * 10_000
                    <= (leg.initial_margin as u128) * params.max_credit_bps_of_im as u128
            );
            if params.cap_at_im_minus_mm {
                assert!(
                    c <= leg.initial_margin - leg.maintenance_margin,
                    "credit below MM"
                );
            }
        }
        // Margin still held is at least the netted portfolio risk.
        let held = r.sum_initial_margin - total;
        assert!(
            held >= r.portfolio_risk.min(r.sum_initial_margin),
            "held {held} < netted risk {}",
            r.portfolio_risk
        );
        // Credit never exceeds what the hedge benefit justifies.
        assert!(
            total as u128
                <= (r.sum_initial_margin as u128).saturating_sub(r.portfolio_risk as u128)
        );
    }
    assert!(checked > 3_000, "too few valid samples: {checked}");
}

#[test]
fn prop_single_leg_and_same_direction_same_asset_give_zero() {
    let mut rng = Lcg(0x5EED_0003);
    let rho = |_: u8, _: u8| 0i64;
    let model = MarginAsRiskModel { rho: &rho };
    for _ in 0..1_000 {
        let legs = random_legs(&mut rng, 1);
        let params = random_params(&mut rng);
        let r = compute_portfolio_credit(&legs, &model, &params).unwrap();
        assert_eq!(r.total(), 0);

        // The same asset, same direction, on several venues: no hedge at all.
        let mut many = Vec::new();
        for v in 0..4u8 {
            let mut l = legs[0];
            l.venue_index = v;
            many.push(l);
        }
        let r = compute_portfolio_credit(&many, &model, &params).unwrap();
        assert_eq!(r.total(), 0);
    }
}

#[test]
fn cap_groups_never_increases_and_enforces_headroom() {
    let mut rng = Lcg(0x5EED_0004);
    for _ in 0..2_000 {
        let n = rng.range(1, 8) as usize;
        let credits: Vec<u64> = (0..n).map(|_| rng.range(0, 1_000_000_000)).collect();
        let groups: Vec<usize> = (0..n).map(|_| rng.range(0, 3) as usize).collect();
        let headroom: Vec<u64> = (0..4).map(|_| rng.range(0, 2_000_000_000)).collect();
        let mut out = credits.clone();
        cap_groups(&mut out, &|i| groups[i], &headroom);
        for (g, &h) in headroom.iter().enumerate() {
            let s: u64 = (0..n).filter(|&i| groups[i] == g).map(|i| out[i]).sum();
            assert!(s <= h);
        }
        for i in 0..n {
            assert!(out[i] <= credits[i]);
        }
    }
}

// -----------------------------------------------------------------------------
// F3 regression: the original pairwise scheme vs the joint engine.
// -----------------------------------------------------------------------------

/// The v0 on-chain pipeline for one pair (compute_credit before this branch):
/// credit_total(haircut) ∧ max_credit_per_user, split_pro_rata, per-leg cap.
fn v0_pair(ra: i64, rb: i64, rho: i64) -> (u64, u64) {
    let legs = [
        Leg {
            asset: 0,
            signed_required_margin: ra,
        },
        Leg {
            asset: 1,
            signed_required_margin: rb,
        },
    ];
    let c = combined_risk(&legs, &|_, _| rho).unwrap();
    let (ua, ub) = (ra.unsigned_abs(), rb.unsigned_abs());
    let t = credit_total(ua + ub, c, 2_000).min(100_000_000_000);
    let s = split_pro_rata(t, [ua, ub]);
    (s[0].min(ua * 7_500 / 10_000), s[1].min(ub * 7_500 / 10_000))
}

#[test]
fn f3_pairwise_overcredit_is_reproduced_and_joint_engine_removes_it() {
    // One long SOL leg (venue 0) hedged by n equal short ETH legs on venues 1..=n.
    let r = 10_000_000_000i64;
    let rho_v = 800_000i64;
    let rho = |_: u8, _: u8| rho_v;
    let model = MarginAsRiskModel { rho: &rho };
    let params = CreditParams {
        haircut_bps: 2_000,
        max_credit_per_user: u64::MAX,
        max_credit_bps_of_im: 10_000,
        cap_at_im_minus_mm: false,
    };
    // (n, pairwise over-credit in tenths of a percent, as measured in the review)
    for (n, expected_over_permille) in [(2u8, 237i64), (3, 590), (5, 1335)] {
        // Pairwise: credit_A is overwritten by each pair, each B accumulates.
        let mut pairwise = 0u64;
        let mut last_a = 0;
        for _ in 0..n {
            let (a, b) = v0_pair(r, -r, rho_v);
            pairwise += b;
            last_a = a;
        }
        pairwise += last_a;

        let mut legs = vec![PortfolioLeg {
            venue_index: 0,
            asset: 0,
            is_long: true,
            notional: 5 * r as u64,
            initial_margin: r as u64,
            maintenance_margin: r as u64 / 2,
        }];
        for v in 1..=n {
            legs.push(PortfolioLeg {
                venue_index: v,
                asset: 1,
                is_long: false,
                notional: 5 * r as u64,
                initial_margin: r as u64,
                maintenance_margin: r as u64 / 2,
            });
        }
        let joint = compute_portfolio_credit(&legs, &model, &params).unwrap();
        let jt = joint.total_before_leg_caps;

        // Rounded half-up, as the review reported (+23.7% / +59.0% / +133.5%).
        let over_permille = (((pairwise as i128 - jt as i128) * 2000 / jt as i128 + 1) / 2) as i64;
        assert_eq!(over_permille, expected_over_permille, "n = {n}");

        // The joint engine holds at least the netted risk; the pairwise scheme
        // did not once n >= 3.
        let sum_im = (n as u64 + 1) * r as u64;
        assert!(sum_im - joint.total() >= joint.portfolio_risk);
        if n >= 3 {
            assert!(sum_im - pairwise < joint.portfolio_risk);
        }
    }
}
