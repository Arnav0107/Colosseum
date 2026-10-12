//! A1: counterparty allowlists (ch_math::view). Properties on random books
//! and random allowlists; no external dependencies.
//!
//!     cargo test -p ch_math --test view_props -- --nocapture

use ch_math::portfolio::*;
use ch_math::risk::*;
use ch_math::view::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

fn assets() -> [AssetRisk; MAX_ASSETS] {
    let mut a = [AssetRisk::default(); MAX_ASSETS];
    for (i, x) in a.iter_mut().take(4).enumerate() {
        let s = [1_000u16, 800, 600, 900][i];
        *x = AssetRisk {
            shock_bps: s,
            gap_bps: s,
            basis_bps: 100,
        };
    }
    a
}

fn rho(a: u8, b: u8) -> i64 {
    if a == b {
        1_000_000
    } else {
        [
            [0, 800_000, 700_000, 600_000],
            [800_000, 0, 750_000, 500_000],
            [700_000, 750_000, 0, 400_000],
            [600_000, 500_000, 400_000, 0],
        ][a as usize][b as usize]
    }
}

const P: CreditParams = CreditParams {
    haircut_bps: 2_000,
    max_credit_per_user: 100_000_000_000,
    max_credit_bps_of_im: 7_500,
    cap_at_im_minus_mm: true,
};

fn leg(v: u8, a: u8, long: bool, n: u64) -> PortfolioLeg {
    PortfolioLeg {
        venue_index: v,
        asset: a,
        is_long: long,
        notional: n,
        initial_margin: n / 5,
        maintenance_margin: n / 10,
    }
}

fn model(a: &[AssetRisk; MAX_ASSETS]) -> HedgeBreakModel<'_> {
    HedgeBreakModel {
        inner: StressedRiskModel {
            assets: a,
            rho: &rho,
            corr_stress_ppm: 100_000,
            basis_decorr_ppm: 20_000,
            scenarios: &[],
        },
        hedge_break_bps: 5_000,
    }
}

/// Credit of leg `i` computed over only the legs of venues `v` accepts: the
/// upper bound a non-accepted venue cannot move.
fn accepted_only_bound(legs: &[PortfolioLeg], i: usize, accepts: &[u64; 8]) -> u64 {
    let a = assets();
    let m = model(&a);
    let v = legs[i].venue_index;
    let mask = accepts[v as usize] | venue_bit(v);
    let sub: Vec<_> = legs
        .iter()
        .copied()
        .filter(|l| mask & venue_bit(l.venue_index) != 0)
        .collect();
    if sub.len() < 2 {
        return 0;
    }
    let pos = sub
        .iter()
        .position(|l| (l.venue_index, l.asset) == (legs[i].venue_index, legs[i].asset))
        .unwrap();
    compute_portfolio_credit(&sub, &m, &P).unwrap().leg_credit[pos]
}

#[test]
fn default_allowlist_gives_no_cross_venue_credit() {
    let a = assets();
    let m = model(&a);
    // Long SOL on venue 0, short ETH on venue 1: the worked example.
    let legs = [
        leg(0, 0, true, 50_000_000_000),
        leg(1, 1, false, 50_000_000_000),
    ];
    let none = compute_view_credit(&legs, &m, &P, &|_| 0).unwrap();
    assert!(none.joint.total() > 0, "the joint book is a hedge");
    assert_eq!(none.total(), 0, "nobody allowlists anybody: 0");
    assert_eq!(&none.view_mask[..2], &[0b01, 0b10]);
    assert_eq!(none.view_runs, 0, "single-leg views cost nothing");
    // Both allowlist each other: exactly the joint credit.
    let both = compute_view_credit(&legs, &m, &P, &|v| if v == 0 { 0b10 } else { 0b01 }).unwrap();
    assert_eq!(&both.leg_credit[..2], &both.joint.leg_credit[..2]);
    assert_eq!(both.total(), 7_111_111_110, "worked example");
    assert_eq!(both.view_runs, 0, "full views reuse the joint result");
    // One-sided: venue 0 accepts 1, venue 1 does not accept 0.
    let one = compute_view_credit(&legs, &m, &P, &|v| if v == 0 { 0b10 } else { 0 }).unwrap();
    assert_eq!(one.leg_credit[0], both.leg_credit[0]);
    assert_eq!(one.leg_credit[1], 0);
    // Same-venue hedges need no allowlist.
    let same = [
        leg(0, 0, true, 50_000_000_000),
        leg(0, 1, false, 50_000_000_000),
    ];
    let s = compute_view_credit(&same, &m, &P, &|_| 0).unwrap();
    assert_eq!(s.total(), s.joint.total());
    assert!(s.total() > 0);
}

#[test]
fn malicious_fake_offset_cannot_raise_a_non_allowlisting_venue() {
    let a = assets();
    let m = model(&a);
    // Venue 0 honest: long SOL. Venue 2 (malicious) reports a fake short SOL
    // and allowlists everyone; venue 0 accepts only venue 1.
    let honest = [
        leg(0, 0, true, 50_000_000_000),
        leg(1, 1, true, 10_000_000_000),
    ];
    let with_fake = [honest[0], honest[1], leg(2, 0, false, 50_000_000_000)];
    let acc = |v: u8| match v {
        0 => 0b010,
        2 => u64::MAX,
        _ => 0,
    };
    let base = compute_view_credit(&honest, &m, &P, &acc).unwrap();
    let r = compute_view_credit(&with_fake, &m, &P, &acc).unwrap();
    assert!(
        r.joint.leg_credit[0] > base.joint.leg_credit[0],
        "the fake leg raises the joint credit"
    );
    assert!(
        r.leg_credit[0] <= base.leg_credit[0],
        "but not venue 0's credit"
    );
    assert_eq!(r.view_mask[0], 0b011, "venue 2 is not in venue 0's view");
}

#[test]
fn one_hedge_is_not_used_twice_across_views() {
    let a = assets();
    let m = model(&a);
    // Short SOL on venue 1 could back the long on 0 (in 0's view) and the long
    // on 2 (in 2's view). min(joint, view) keeps the total within the joint one.
    let legs = [
        leg(0, 0, true, 50_000_000_000),
        leg(1, 0, false, 50_000_000_000),
        leg(2, 0, true, 50_000_000_000),
    ];
    let acc = |v: u8| match v {
        0 => 0b010,
        2 => 0b010,
        1 => 0b101,
        _ => 0,
    };
    let r = compute_view_credit(&legs, &m, &P, &acc).unwrap();
    for i in 0..3 {
        assert!(r.leg_credit[i] <= r.joint.leg_credit[i]);
    }
    assert!(r.total() <= r.joint.total());
    // Each pairwise view alone would have given more in total.
    let pair = compute_portfolio_credit(&legs[..2], &m, &P)
        .unwrap()
        .total();
    assert!(2 * pair > r.joint.total(), "pairwise would over-credit: F3");
}

#[test]
fn prop_view_credit_bounds_order_and_untrusted_reports() {
    let mut r = Rng(0xA1_A11_0F1);
    let a = assets();
    let m = model(&a);
    let (mut checked, mut cut_by_view, mut cut_by_joint) = (0u32, 0u32, 0u32);
    for _ in 0..4_000 {
        let n = r.range(1, MAX_LEGS as u64) as usize;
        let mut legs: Vec<PortfolioLeg> = Vec::new();
        while legs.len() < n {
            let v = r.range(0, 7) as u8;
            let asset = r.range(0, 3) as u8;
            if legs.iter().any(|l| (l.venue_index, l.asset) == (v, asset)) {
                continue;
            }
            legs.push(leg(
                v,
                asset,
                r.next() & 1 == 0,
                r.range(1_000_000_000, 100_000_000_000),
            ));
        }
        let mut accepts = [0u64; 8];
        for x in accepts.iter_mut() {
            *x = r.next() & 0xff;
        }
        let acc = |v: u8| accepts[v as usize];
        let Ok(vc) = compute_view_credit(&legs, &m, &P, &acc) else {
            continue;
        };
        checked += 1;
        for i in 0..n {
            let c = vc.leg_credit[i];
            assert!(c <= vc.joint.leg_credit[i], "above the joint credit");
            let bound = accepted_only_bound(&legs, i, &accepts);
            assert!(c <= bound, "above what accepted legs alone justify");
            let own = venue_bit(legs[i].venue_index);
            assert!(vc.view_mask[i] & own != 0);
            assert!(view_still_accepted(
                vc.view_mask[i],
                legs[i].venue_index,
                accepts[legs[i].venue_index as usize]
            ));
            if c < vc.joint.leg_credit[i] {
                cut_by_view += 1;
            }
            if c < bound {
                cut_by_joint += 1;
            }
        }
        assert!(vc.total() <= vc.joint.total());
        // Leg order never matters.
        let mut perm = legs.clone();
        perm.reverse();
        let vp = compute_view_credit(&perm, &m, &P, &acc).unwrap();
        for (i, l) in legs.iter().enumerate() {
            let j = perm
                .iter()
                .position(|x| (x.venue_index, x.asset) == (l.venue_index, l.asset))
                .unwrap();
            assert_eq!(vc.leg_credit[i], vp.leg_credit[j]);
            assert_eq!(vc.view_mask[i], vp.view_mask[j]);
        }
        // Untrusted reports: change every leg of venues outside leg 0's view
        // (size and side) and the bound for leg 0 does not move.
        let v0 = legs[0].venue_index;
        let mask0 = accepts[v0 as usize] | venue_bit(v0);
        let mut changed = legs.clone();
        for l in changed.iter_mut() {
            if mask0 & venue_bit(l.venue_index) == 0 {
                l.is_long = !l.is_long;
                l.notional = l.notional / 2 + 1_000_000_000;
                l.initial_margin = l.notional / 5;
                l.maintenance_margin = l.notional / 10;
            }
        }
        if let Ok(vc2) = compute_view_credit(&changed, &m, &P, &acc) {
            assert!(vc2.leg_credit[0] <= accepted_only_bound(&legs, 0, &accepts));
        }
    }
    println!("view property: {checked} books; credit cut by the view {cut_by_view}, by the joint {cut_by_joint}");
    assert!(checked > 3_000);
    assert!(
        cut_by_view > 100 && cut_by_joint > 100,
        "both sides of the min must be reached"
    );
}
