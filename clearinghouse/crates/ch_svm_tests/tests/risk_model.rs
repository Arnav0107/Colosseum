//! Stage C on-chain: stressed risk model parameters, scenarios, IM − MM caps,
//! fail-closed unsupported assets, and venue-rule parity with ch_client.

use ch_core::{status, ClearinghouseError as E};
use ch_math::portfolio::PortfolioLeg;
use ch_math::risk::scenario_kind;
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const IM: u64 = 10_000_000_000;

fn hedged(w: &mut World) -> Kp {
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    u
}

#[test]
fn asset_without_risk_parameters_fails_closed() {
    let mut w = World::new(2);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, 5, false, IM)); // asset 5 has shock 0
                                               // Needs a price account to get as far as the model.
    w.set_price(5, 1_000_000);
    assert_ch_err(&w.refresh(&u), E::UnsupportedAsset);
    assert_eq!(w.usable_total(&u), 0);
}

#[test]
fn risk_parameter_bounds_are_enforced() {
    let mut w = World::new(1);
    let bad_asset = [
        (SOL, 50, 50, 100),     // shock below floor
        (SOL, 1_000, 900, 100), // gap below shock
        (SOL, 1_000, 1_000, 5), // basis below floor
        (SOL, 10_001, 10_001, 100),
        (SOL, 0, 100, 0), // disabled asset with leftovers
    ];
    for (a, s, g, b) in bad_asset {
        let i = w.set_asset_risk_ix(a, s, g, b);
        assert_ch_err(&w.admin_send(i), E::InvalidRiskParams);
    }
    let i = w.set_asset_risk_ix(32, 1_000, 1_000, 100);
    assert_ch_err(&w.admin_send(i), E::InvalidAssetId);

    for (d, b) in [
        (10_000u32, 20_000u32),
        (100_000, 1_000),
        (1_000_001, 20_000),
    ] {
        let i = w.set_stress_ix(d, b);
        assert_ch_err(&w.admin_send(i), E::InvalidRiskParams);
    }

    let i = w.set_scenario_ix(8, scenario_kind::HYPOTHETICAL, &[(SOL, -1_000)]);
    assert_ch_err(&w.admin_send(i), E::InvalidScenario);
    let i = w.set_scenario_ix(0, 3, &[(SOL, -1_000)]);
    assert_ch_err(&w.admin_send(i), E::InvalidScenario);
    let i = w.set_scenario_ix(0, scenario_kind::UNUSED, &[(SOL, -1_000)]);
    assert_ch_err(&w.admin_send(i), E::InvalidScenario);
    let i = w.set_scenario_ix(0, scenario_kind::HYPOTHETICAL, &[(SOL, -10_001)]);
    assert_ch_err(&w.admin_send(i), E::InvalidRiskParams);

    // Only the admin (Stage D: risk authority) may change the model.
    let mallory = w.env.funded(1);
    let mut i = w.set_stress_ix(200_000, 20_000);
    for m in i.accounts.iter_mut() {
        if m.pubkey == w.admin_pk() {
            m.pubkey = kp_pubkey(&mallory);
        }
    }
    assert_ch_err(&w.env.send(&[i], &[&mallory]), E::Unauthorized);
}

#[test]
fn every_model_change_bumps_the_epoch_and_invalidates_credit() {
    let mut w = World::new(2);
    let u = hedged(&mut w);
    let changes = vec![
        w.set_asset_risk_ix(SOL, 1_200, 1_200, 100),
        w.set_stress_ix(150_000, 20_000),
        w.set_scenario_ix(0, scenario_kind::HYPOTHETICAL, &[(SOL, -500)]),
    ];
    for i in changes {
        assert_ok(&w.refresh(&u));
        assert!(w.usable_total(&u) > 0);
        let e = w.config().risk_epoch;
        assert_ok(&w.admin_send(i));
        assert_eq!(w.config().risk_epoch, e + 1);
        assert_eq!(w.usable_total(&u), 0);
        assert_eq!(w.config().global_outstanding_credit, 0);
    }
}

/// HISTORICAL REPLAY (Binance daily closes, 2 -> 15 Nov 2022, see
/// crates/ch_math/tests/risk_stress.rs): adding it as a governance scenario
/// removes all credit from a long-SOL / short-ETH hedge.
#[test]
fn nov2022_replay_scenario_removes_credit_from_sol_eth_hedge() {
    let mut w = World::new(2);
    let u = hedged(&mut w);
    assert_eq!(w.portfolio(&u).total_credit, 7_111_111_110);
    let i = w.set_scenario_ix(
        0,
        scenario_kind::HISTORICAL_REPLAY,
        &[(SOL, -5_351), (ETH, -1_747)],
    );
    assert_ok(&w.admin_send(i));
    assert_ok(&w.refresh(&u));
    let p = w.portfolio(&u);
    assert_eq!(p.total_credit, 0);
    assert!(p.portfolio_risk >= p.sum_standalone_risk);
    // Clearing the slot restores the previous result.
    let i = w.set_scenario_ix(0, scenario_kind::UNUSED, &[]);
    assert_ok(&w.admin_send(i));
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).total_credit, 7_111_111_110);
}

#[test]
fn credit_never_exceeds_im_minus_mm_on_chain() {
    let mut w = World::new(2);
    let u = w.user();
    // MM at 95% of IM: at most 5% of IM may be relieved per leg.
    assert_ok(&w.submit_full(&u, 0, SOL, 5 * IM, true, IM, IM / 100 * 95));
    assert_ok(&w.submit_full(&u, 1, ETH, 5 * IM, false, IM, IM / 100 * 95));
    assert_ok(&w.refresh(&u));
    let p = w.portfolio(&u);
    for l in &p.legs[..2] {
        assert!(l.credit <= l.initial_margin - l.maintenance_margin);
        assert_eq!(l.credit, IM / 100 * 5);
        let eff =
            ch_client::effective_initial_margin(l.initial_margin, l.maintenance_margin, l.credit);
        assert_eq!(eff, l.maintenance_margin);
    }
    let legs = [
        PortfolioLeg {
            venue_index: 0,
            asset: SOL,
            is_long: true,
            notional: 5 * IM,
            initial_margin: IM,
            maintenance_margin: IM / 100 * 95,
        },
        PortfolioLeg {
            venue_index: 1,
            asset: ETH,
            is_long: false,
            notional: 5 * IM,
            initial_margin: IM,
            maintenance_margin: IM / 100 * 95,
        },
    ];
    let exp = expected_credit(&legs, 800_000, &[], &default_params());
    assert_eq!(p.total_credit, exp.total());
}

/// The venue-side rule exists twice (on-chain account method for programs,
/// dependency-free ch_client for everyone else). They must agree.
#[test]
fn ch_client_and_on_chain_usable_credit_agree_in_every_state() {
    let mut w = World::new(3);
    let u = hedged(&mut w);
    let check = |w: &World, label: &str| {
        let p = w.portfolio(&u);
        let c = w.config();
        let legs: Vec<ch_client::LegCredit> = p.legs[..p.num_legs as usize]
            .iter()
            .map(|l| ch_client::LegCredit {
                venue_index: l.venue_index,
                asset_id: l.asset_id,
                credit: l.credit,
                view_mask: l.view_mask,
            })
            .collect();
        let view = ch_client::PortfolioView {
            status: p.status,
            credit_kind: p.credit_kind,
            risk_epoch: p.risk_epoch,
            expires_slot: p.expires_slot,
            legs: &legs,
        };
        let now = w.env.clock().slot;
        for v in 0..3u8 {
            for a in [SOL, ETH, BTC] {
                assert_eq!(
                    p.usable_credit(c.risk_epoch, c.accepts_of(v), now, v, a),
                    ch_client::usable_credit(&view, c.risk_epoch, c.accepts_of(v), now, v, a),
                    "{label}: venue {v} asset {a}"
                );
            }
        }
    };
    check(&w, "active");
    assert!(w.usable_total(&u) > 0);

    w.env.advance_slots(3_001);
    check(&w, "expired");

    w.refresh_prices();
    assert_ok(&w.refresh(&u));
    w.set_corr(&[(SOL, ETH, 850_000)]);
    check(&w, "epoch bumped");

    assert_ok(&w.refresh(&u));
    assert_ok(&w.submit(&u, 1, ETH, false, IM / 2));
    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);
    check(&w, "invalidated");
}
