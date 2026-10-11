//! Portfolio account, joint netting, input binding, atomic invalidation, risk
//! epoch, caps and permissionless refresh — exercised against the compiled
//! program. Expected credits come from the same `ch_math` stressed model the
//! program uses (fixture parameters in `setup::DEFAULT_ASSET_RISK`).

use anchor_lang::prelude::Pubkey;
use ch_core::{status, ClearinghouseError as E, ConfigParams};
use ch_math::portfolio::PortfolioLeg;
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const IM: u64 = 10_000_000_000; // $10,000

/// Expected per-leg credits for legs given as (venue_index, asset, is_long, im)
/// with notional = 5 IM and MM = IM / 2.
fn expected(legs: &[(u8, u8, bool, u64)], rho01: i64, p: &ConfigParams) -> Vec<u64> {
    let legs: Vec<PortfolioLeg> = legs
        .iter()
        .map(|&(v, a, l, im)| PortfolioLeg {
            venue_index: v,
            asset: a,
            is_long: l,
            notional: 5 * im,
            initial_margin: im,
            maintenance_margin: im / 2,
        })
        .collect();
    expected_credit(&legs, rho01, &[], p).leg_credit[..legs.len()].to_vec()
}

fn leg_credits(w: &World, user: &Kp) -> Vec<u64> {
    let p = w.portfolio(user);
    p.legs[..p.num_legs as usize]
        .iter()
        .map(|l| l.credit)
        .collect()
}

/// Two venues, long SOL on venue 0 and short ETH on venue 1, refreshed.
fn hedged() -> (World, Kp) {
    let mut w = World::new(4);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    (w, u)
}

#[test]
fn hedged_pair_credit_matches_engine_and_is_counted_once() {
    let (w, u) = hedged();
    let p = w.portfolio(&u);
    assert_eq!(p.status, status::ACTIVE);
    assert_eq!(p.credit_kind, ch_core::CREDIT_KIND_TRADING_POWER_ONLY);
    assert_eq!(p.num_legs, 2);
    // Stressed model (docs/RISK_MODEL.md worked example): S = 9e9 (10% of
    // $50k SOL + 8% of $50k ETH), R_p = 5e9 (SOL gaps alone), 20% haircut:
    // floor(20e9 * 4e9 / 9e9) * 0.8 = 7_111_111_110, split pro rata by IM.
    assert_eq!(leg_credits(&w, &u), vec![3_555_555_555, 3_555_555_555]);
    assert_eq!(
        leg_credits(&w, &u),
        expected(
            &[(0, SOL, true, IM), (1, ETH, false, IM)],
            800_000,
            &default_params()
        )
    );
    assert_eq!(p.total_credit, 7_111_111_110);
    assert_eq!(p.sum_standalone_risk, 9_000_000_000);
    assert_eq!(p.portfolio_risk, 5_000_000_000);
    assert_eq!(w.usable(&u, 0, SOL), 3_555_555_555);
    assert_eq!(w.usable(&u, 1, ETH), 3_555_555_555);
    assert_eq!(w.usable(&u, 1, SOL), 0, "no leg for (venue 1, SOL)");
    // Input binding: snapshot versions, baseline price, epoch, model version.
    assert_eq!(p.legs[0].snapshot_version, 1);
    assert_eq!(p.legs[0].baseline_price, 150_000_000);
    assert_eq!(p.legs[1].baseline_price, 3_000_000_000);
    let c = w.config();
    assert_eq!(p.risk_epoch, c.risk_epoch);
    assert_eq!(c.global_outstanding_credit, p.total_credit);
    assert_eq!(c.venue_outstanding[0], 3_555_555_555);
    assert_eq!(c.asset_outstanding[ETH as usize], 3_555_555_555);
}

#[test]
fn single_unhedged_leg_gets_zero() {
    let mut w = World::new(2);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).total_credit, 0);
    assert_eq!(w.usable(&u, 0, SOL), 0);
}

#[test]
fn f3_three_hedge_venues_are_netted_jointly() {
    let mut w = World::new(4);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    for v in 1..=3 {
        assert_ok(&w.submit(&u, v, ETH, false, IM));
    }
    assert_ok(&w.refresh(&u));
    let legs = [
        (0, SOL, true, IM),
        (1, ETH, false, IM),
        (2, ETH, false, IM),
        (3, ETH, false, IM),
    ];
    let mut exp = expected(&legs, 800_000, &default_params());
    let mut got = leg_credits(&w, &u);
    exp.sort();
    got.sort();
    assert_eq!(got, exp);
    let p = w.portfolio(&u);
    // Margin held >= netted risk (the pairwise scheme violated this).
    assert!(4 * IM - p.total_credit >= p.portfolio_risk);
}

#[test]
fn f2_shrink_to_near_zero_invalidates_in_the_same_instruction() {
    let (mut w, u) = hedged();
    let v_before = w.portfolio(&u).version;
    // Venue 1 shrinks the hedge to $1 notional.
    assert_ok(&w.submit_full(&u, 1, ETH, 1_000_000, false, 200_000, 100_000));
    let p = w.portfolio(&u);
    assert_eq!(p.status, status::INVALIDATED);
    assert!(p.version > v_before);
    assert_eq!(w.usable(&u, 0, SOL), 0);
    assert_eq!(w.usable(&u, 1, ETH), 0, "old $5,470 credit is gone");

    assert_ok(&w.refresh(&u));
    let p = w.portfolio(&u);
    assert_eq!(p.legs[1].snapshot_version, 2);
    // Recomputed from the new inputs only.
    let legs = [
        PortfolioLeg {
            venue_index: 0,
            asset: SOL,
            is_long: true,
            notional: 5 * IM,
            initial_margin: IM,
            maintenance_margin: IM / 2,
        },
        PortfolioLeg {
            venue_index: 1,
            asset: ETH,
            is_long: false,
            notional: 1_000_000,
            initial_margin: 200_000,
            maintenance_margin: 100_000,
        },
    ];
    let exp = expected_credit(&legs, 800_000, &[], &default_params());
    assert_eq!(leg_credits(&w, &u), exp.leg_credit[..2].to_vec());
    assert!(
        p.total_credit < 1_000_000,
        "the $5,470 credit did not survive the shrink"
    );
}

#[test]
fn f2_side_flip_invalidates_and_recomputes_as_same_direction() {
    let (mut w, u) = hedged();
    assert_ok(&w.submit(&u, 1, ETH, true, IM)); // flip short -> long
    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);
    assert_eq!(w.usable(&u, 1, ETH), 0);
    assert_ok(&w.refresh(&u));
    assert_eq!(
        leg_credits(&w, &u),
        expected(
            &[(0, SOL, true, IM), (1, ETH, true, IM)],
            800_000,
            &default_params()
        )
    );
    // Same-direction book: the market-wide scenario loses the full sum of
    // standalone risks, so there is no hedge benefit at all.
    assert_eq!(w.portfolio(&u).total_credit, 0);
}

#[test]
fn hedge_leg_removed_invalidates_and_leaves_zero() {
    let (mut w, u) = hedged();
    let venue1 = kp_pubkey(&w.venues[1].authority);
    let rent_before = w
        .env
        .svm
        .get_account(&venue1.to_bytes().into())
        .unwrap()
        .lamports;
    let i = w.close_ix(&kp_pubkey(&u), 1, ETH, &venue1);
    let auth = w.venues[1].authority.insecure_clone();
    assert_ok(&w.env.send(&[i], &[&auth]));
    let p = w.portfolio(&u);
    assert_eq!(p.status, status::INVALIDATED);
    assert_eq!(p.num_registered, 1);
    assert!(!w
        .env
        .exists(&snapshot_pda(&kp_pubkey(&u), &w.venues[1].id, ETH)));
    let rent_after = w
        .env
        .svm
        .get_account(&venue1.to_bytes().into())
        .unwrap()
        .lamports;
    assert!(rent_after > rent_before, "rent returned to the payer");
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).total_credit, 0);
}

#[test]
fn venue_deactivated_invalidates_globally_and_blocks_refresh() {
    let (mut w, u) = hedged();
    let epoch = w.config().risk_epoch;
    let admin = w.admin_pk();
    let admin_kp = w.admin.insecure_clone();
    let id1 = w.venues[1].id;
    assert_ok(
        &w.env
            .send(&[set_venue_active_ix(&admin, &id1, false)], &[&admin_kp]),
    );
    let c = w.config();
    assert_eq!(c.risk_epoch, epoch + 1);
    assert_eq!(
        c.global_outstanding_credit, 0,
        "epoch bump drops all shares"
    );
    assert_eq!(w.usable(&u, 0, SOL), 0);
    assert_eq!(w.usable(&u, 1, ETH), 0);
    assert_ch_err(&w.refresh(&u), E::VenueInactive);
    // A new snapshot from the inactive venue is refused too.
    assert_ch_err(&w.submit(&u, 1, ETH, false, IM), E::VenueInactive);

    // The user can drop the leg of the inactive venue and refresh the rest.
    let user_pk = kp_pubkey(&u);
    let i = w.close_ix(&user_pk, 1, ETH, &user_pk);
    assert_ok(&w.env.send(&[i], &[&u]));
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).total_credit, 0);
}

#[test]
fn consent_revoked_invalidates_in_the_same_instruction() {
    let (mut w, u) = hedged();
    assert_ok(&w.set_consent(&u, false, u64::MAX));
    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);
    assert_eq!(w.usable(&u, 0, SOL), 0);
    assert_ch_err(&w.refresh(&u), E::UserConsentMissing);

    // Re-consent but without venue 1.
    assert_ok(&w.set_consent(&u, true, 0b1));
    assert_ch_err(&w.refresh(&u), E::VenueNotAuthorized);
    // The user may now remove venue 1's snapshot; the venue's own consent rule
    // no longer covers it.
    let user_pk = kp_pubkey(&u);
    let i = w.close_ix(&user_pk, 1, ETH, &user_pk);
    assert_ok(&w.env.send(&[i], &[&u]));
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).total_credit, 0);
}

#[test]
fn stale_snapshot_allows_invalidation_and_blocks_refresh() {
    let mut params = default_params();
    params.snapshot_max_age_slots = 100;
    let mut w = World::with_params(2, params);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    assert_ch_err(&w.invalidate(&u, false), E::InvalidationNotJustified);

    w.env.advance_slots(101);
    w.refresh_prices();
    assert_ok(&w.invalidate(&u, false));
    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);
    assert_eq!(w.config().global_outstanding_credit, 0);
    assert_ch_err(&w.refresh(&u), E::SnapshotStale);
}

#[test]
fn price_breach_vs_stored_baseline_allows_invalidation() {
    let (mut w, u) = hedged();
    // Evidence must be published after the baseline (see the next test).
    w.env.advance_slots(5);
    // 2% move: within the default 3% guard (max_move_bps = 300).
    w.set_price(SOL, 153_000_000);
    assert_ch_err(&w.invalidate(&u, true), E::InvalidationNotJustified);
    // 6% move from the baseline recorded at refresh.
    w.set_price(SOL, 159_000_000);
    assert_ok(&w.invalidate(&u, true));
    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);
    assert_eq!(w.config().global_outstanding_credit, 0);
    // A fresh refresh records the new baseline.
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).legs[0].baseline_price, 159_000_000);
}

/// M1: with the default params (max_move_bps = 300) a 15% move is a price
/// breach. Invalidation succeeds with exactly the PRICE_BREACH reason, and
/// afterwards a venue applying the ch_client rule charges the full IM.
#[test]
fn m1_fifteen_percent_move_is_a_price_breach_and_the_venue_charges_full_im() {
    assert_eq!(default_params().max_move_bps, 300);
    let (mut w, u) = hedged();
    let before = w.usable(&u, 0, SOL);
    assert!(before > 0);
    let im = IM;
    let mm = IM / 2;
    assert!(ch_client::effective_initial_margin(im, mm, before) < im);

    w.env.advance_slots(5);
    w.set_price(SOL, 172_500_000); // +15% vs the $150 baseline
    w.set_price(ETH, 3_000_000_000); // ETH unchanged, republished (fresh)
    let r = w.invalidate(&u, true);
    assert_ok(&r);
    let reasons = invalidation_reasons(&r).expect("PortfolioInvalidated event");
    assert_eq!(
        reasons,
        ch_core::invalidation::PRICE_BREACH,
        "reasons {reasons:#b}"
    );

    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);
    for (v, a) in [(0u8, SOL), (1, ETH)] {
        let credit = w.usable(&u, v, a);
        assert_eq!(credit, 0);
        assert_eq!(ch_client::effective_initial_margin(im, mm, credit), im);
    }
}

#[test]
fn oracle_evidence_not_newer_than_the_baseline_is_rejected() {
    // With a real oracle anyone can present an older genuine update. Only a
    // price published after the one the credit was computed on is evidence.
    let (mut w, u) = hedged();
    let ts = w.env.clock().unix_timestamp;
    w.set_price_full(SOL, 159_000_000, 15_900, ts); // 6% "move", same second
    assert_ch_err(&w.invalidate(&u, true), E::InvalidationNotJustified);
    w.set_price_full(SOL, 159_000_000, 15_900, ts - 1);
    assert_ch_err(&w.invalidate(&u, true), E::InvalidationNotJustified);
    // Even a stale-looking old reading is not a trip.
    w.set_price_full(SOL, 150_000_000, 15_000, ts - 1_000);
    assert_ch_err(&w.invalidate(&u, true), E::InvalidationNotJustified);
    assert_eq!(w.portfolio(&u).status, status::ACTIVE);
    // A newer reading for one leg is evidence even if the others are unchanged.
    w.set_price_full(SOL, 159_000_000, 15_900, ts + 1);
    assert_ok(&w.invalidate(&u, true));
    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);
}

#[test]
fn f5_resubmitting_a_snapshot_cannot_reset_the_baseline_under_live_credit() {
    // v0 bug: a venue re-submitting the same position overwrote the price the
    // guard compared against, while the credit stayed live. Now the baseline
    // lives in the portfolio and any snapshot write invalidates the credit.
    let (mut w, u) = hedged();
    w.set_price(SOL, 159_000_000); // 6% move
    assert_ok(&w.submit(&u, 0, SOL, true, IM)); // same position, re-reported
    let p = w.portfolio(&u);
    assert_eq!(p.status, status::INVALIDATED);
    assert_eq!(
        p.legs[0].baseline_price, 150_000_000,
        "baseline untouched by the venue"
    );
    assert_eq!(w.usable(&u, 0, SOL), 0);
}

#[test]
fn wrong_oracle_in_invalidate_is_an_error_not_a_trip() {
    let (mut w, u) = hedged();
    let user = kp_pubkey(&u);
    let mut i = w.invalidate_ix(&user, false);
    // ETH oracle for the SOL leg.
    for k in [mock_price_pda(ETH), mock_price_pda(ETH)] {
        i.accounts
            .push(anchor_lang::solana_program::instruction::AccountMeta::new_readonly(k, false));
    }
    let r = w.env.send(&[i], &[]);
    assert!(r.is_err());
    assert_eq!(w.portfolio(&u).status, status::ACTIVE);
}

#[test]
fn correlation_update_bumps_epoch_and_zeroes_usable_credit() {
    let (mut w, u) = hedged();
    let epoch = w.config().risk_epoch;
    w.set_corr(&[(SOL, ETH, 900_000)]);
    let c = w.config();
    assert_eq!(c.risk_epoch, epoch + 1);
    assert_eq!(c.global_outstanding_credit, 0);
    assert_eq!(w.usable(&u, 0, SOL), 0);
    // Invalidation of an old-epoch portfolio releases nothing further.
    assert_ok(&w.invalidate(&u, false));
    assert_eq!(w.config().global_outstanding_credit, 0);
    assert_ok(&w.refresh(&u));
    assert_eq!(
        leg_credits(&w, &u),
        expected(
            &[(0, SOL, true, IM), (1, ETH, false, IM)],
            900_000,
            &default_params()
        )
    );
    assert_eq!(
        w.config().global_outstanding_credit,
        w.portfolio(&u).total_credit
    );
}

#[test]
fn config_update_bumps_epoch_and_zeroes_usable_credit() {
    let (mut w, u) = hedged();
    let mut p = default_params();
    p.haircut_bps = 5_000;
    let admin = w.admin_pk();
    let admin_kp = w.admin.insecure_clone();
    let i = update_params_ix(&admin, p);
    assert_ok(&w.env.send(&[i], &[&admin_kp]));
    assert_eq!(w.usable(&u, 0, SOL), 0);
    assert_eq!(w.config().global_outstanding_credit, 0);
    assert_ok(&w.refresh(&u));
    assert_eq!(
        leg_credits(&w, &u),
        expected(&[(0, SOL, true, IM), (1, ETH, false, IM)], 800_000, &p)
    );
}

#[test]
fn expiry_zeroes_usable_credit_and_invalidation_releases_exactly_once() {
    let (mut w, u) = hedged();
    let total = w.portfolio(&u).total_credit;
    assert_eq!(w.config().global_outstanding_credit, total);
    let expires = w.portfolio(&u).expires_slot;
    let now = w.env.clock().slot;
    w.env.advance_slots(expires - now);
    assert_eq!(
        w.usable(&u, 0, SOL),
        3_555_555_555,
        "valid through expires_slot"
    );
    w.env.advance_slots(1);
    assert_eq!(w.usable(&u, 0, SOL), 0);
    assert_ok(&w.invalidate(&u, false));
    assert_eq!(w.config().global_outstanding_credit, 0);
    // Second invalidation: nothing left to release, and it says so.
    assert_ch_err(&w.invalidate(&u, false), E::InvalidationNotJustified);
    assert_eq!(w.config().global_outstanding_credit, 0);
}

#[test]
fn revoke_and_recompute_in_the_same_slot_and_transaction() {
    let (mut w, u) = hedged();
    let user = kp_pubkey(&u);
    let slot = w.env.clock().slot;
    // One transaction: venue shrinks the hedge, then anyone refreshes.
    let submit = w.submit_ix(&user, 1, ETH, 5 * IM / 2, false, IM / 2, IM / 4);
    let mut refresh = w.refresh_ix(&user);
    // Registered set is unchanged, so the refresh metas are still correct.
    refresh.accounts.truncate(refresh.accounts.len());
    let auth = w.venues[1].authority.insecure_clone();
    assert_ok(&w.env.send(&[submit, refresh], &[&auth]));
    assert_eq!(w.env.clock().slot, slot);
    let p = w.portfolio(&u);
    assert_eq!(p.status, status::ACTIVE);
    assert_eq!(p.legs[1].snapshot_version, 2);
    assert_eq!(p.legs[1].initial_margin, IM / 2);
    let legs = [
        PortfolioLeg {
            venue_index: 0,
            asset: SOL,
            is_long: true,
            notional: 5 * IM,
            initial_margin: IM,
            maintenance_margin: IM / 2,
        },
        PortfolioLeg {
            venue_index: 1,
            asset: ETH,
            is_long: false,
            notional: 5 * IM / 2,
            initial_margin: IM / 2,
            maintenance_margin: IM / 4,
        },
    ];
    let exp = expected_credit(&legs, 800_000, &[], &default_params());
    assert_eq!(leg_credits(&w, &u), exp.leg_credit[..2].to_vec());
}

#[test]
fn replaying_a_refresh_never_double_counts() {
    let (mut w, u) = hedged();
    let total = w.portfolio(&u).total_credit;
    let v1 = w.portfolio(&u).version;
    for _ in 0..3 {
        assert_ok(&w.refresh(&u));
    }
    assert_eq!(w.config().global_outstanding_credit, total);
    assert_eq!(w.portfolio(&u).total_credit, total);
    assert_eq!(w.portfolio(&u).version, v1 + 3);
}

#[test]
fn leg_order_in_refresh_does_not_change_the_result() {
    let mut w = World::new(4);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, 2 * IM));
    assert_ok(&w.submit(&u, 2, BTC, false, IM / 3));
    assert_ok(&w.refresh(&u));
    let mut a: Vec<_> = w.portfolio(&u).legs[..3]
        .iter()
        .map(|l| (l.venue_index, l.asset_id, l.credit))
        .collect();
    let user = kp_pubkey(&u);
    let metas = w.refresh_metas(&user, Some(&[2, 0, 1]));
    let i = w.refresh_ix_with(&user, &metas);
    assert_ok(&w.env.send(&[i], &[]));
    let mut b: Vec<_> = w.portfolio(&u).legs[..3]
        .iter()
        .map(|l| (l.venue_index, l.asset_id, l.credit))
        .collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

#[test]
fn refresh_rejects_incomplete_duplicate_foreign_and_fake_snapshots() {
    let (mut w, u) = hedged();
    let user = kp_pubkey(&u);
    let snap_a = snapshot_pda(&user, &w.venues[0].id, SOL);
    let snap_b = snapshot_pda(&user, &w.venues[1].id, ETH);
    let (o_sol, o_eth) = (mock_price_pda(SOL), mock_price_pda(ETH));

    // Omitting a leg (a caller could drop a risk-adding leg to inflate credit).
    let i = w.refresh_ix_with(&user, &[snap_a, o_sol]);
    assert_ch_err(&w.env.send(&[i], &[]), E::IncompletePortfolio);

    // The same snapshot twice.
    let i = w.refresh_ix_with(&user, &[snap_a, snap_a, o_sol, o_sol]);
    assert_ch_err(&w.env.send(&[i], &[]), E::DuplicateLeg);

    // Another user's snapshot.
    let v = w.user();
    assert_ok(&w.submit(&v, 1, ETH, false, IM));
    let foreign = snapshot_pda(&kp_pubkey(&v), &w.venues[1].id, ETH);
    let i = w.refresh_ix_with(&user, &[snap_a, foreign, o_sol, o_eth]);
    assert_ch_err(&w.env.send(&[i], &[]), E::InvalidSnapshotAccount);

    // A system-owned account in place of a snapshot.
    let fake = kp_pubkey(&w.venues[2].authority);
    let i = w.refresh_ix_with(&user, &[snap_a, fake, o_sol, o_eth]);
    assert_ch_err(&w.env.send(&[i], &[]), E::InvalidSnapshotAccount);

    // Wrong oracle for a leg (ETH price for the SOL leg).
    let i = w.refresh_ix_with(&user, &[snap_a, snap_b, o_eth, o_eth]);
    assert!(w.env.send(&[i], &[]).is_err());

    // None of the failures above changed the live computation.
    let p = w.portfolio(&u);
    assert_eq!(p.status, status::ACTIVE);
    assert_eq!(w.config().global_outstanding_credit, p.total_credit);
}

#[test]
fn failed_refresh_never_creates_usable_credit() {
    let mut w = World::new(2);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    // Stale price: refresh must fail and leave the portfolio unusable.
    let old_ts = w.env.clock().unix_timestamp - 3_600;
    w.set_price_full(SOL, 150_000_000, 15_000, old_ts);
    assert_ch_err(&w.refresh(&u), E::PriceStale);
    assert_ne!(w.portfolio(&u).status, status::ACTIVE);
    assert_eq!(w.usable_total(&u), 0);
    assert_eq!(w.config().global_outstanding_credit, 0);
    // Wide confidence.
    w.set_price_full(SOL, 150_000_000, 15_000_000, w.env.clock().unix_timestamp);
    assert_ch_err(&w.refresh(&u), E::PriceUncertain);
    assert_eq!(w.usable_total(&u), 0);
}

#[test]
fn per_user_cap_bounds_total_credit() {
    let mut params = default_params();
    params.max_credit_per_user = 1_000_000_000;
    let mut w = World::with_params(2, params);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    assert!(w.portfolio(&u).total_credit <= 1_000_000_000);
    assert_eq!(leg_credits(&w, &u), vec![500_000_000, 500_000_000]);
}

#[test]
fn global_venue_and_asset_caps_bound_outstanding_credit() {
    // Global cap: the second user only gets the remaining headroom.
    let mut params = default_params();
    params.global_credit_cap = 12_000_000_000;
    // Two hedged users at 7_111_111_110 each do not both fit.
    let mut w = World::with_params(2, params);
    let a = w.user();
    let b = w.user();
    for u in [&a, &b] {
        assert_ok(&w.submit(u, 0, SOL, true, IM));
        assert_ok(&w.submit(u, 1, ETH, false, IM));
    }
    assert_ok(&w.refresh(&a));
    assert_eq!(w.portfolio(&a).total_credit, 7_111_111_110);
    assert_ok(&w.refresh(&b));
    let tb = w.portfolio(&b).total_credit;
    assert!(tb <= 12_000_000_000 - 7_111_111_110);
    assert!(w.config().global_outstanding_credit <= 12_000_000_000);
    // Releasing A frees headroom for B's next refresh.
    assert_ok(&w.set_consent(&a, false, u64::MAX));
    assert_ok(&w.invalidate(&a, false));
    assert_ok(&w.refresh(&b));
    assert_eq!(w.portfolio(&b).total_credit, 7_111_111_110);

    // Per-venue cap.
    let mut params = default_params();
    params.max_credit_per_venue = 1_000_000_000;
    let mut w = World::with_params(2, params);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    let c = w.config();
    assert!(c.venue_outstanding[0] <= 1_000_000_000 && c.venue_outstanding[1] <= 1_000_000_000);

    // Per-asset cap.
    let mut params = default_params();
    params.max_credit_per_asset = 2_000_000_000;
    let mut w = World::with_params(3, params);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.submit(&u, 2, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    let c = w.config();
    assert!(c.asset_outstanding[ETH as usize] <= 2_000_000_000);
    assert_eq!(c.global_outstanding_credit, w.portfolio(&u).total_credit);
}

#[test]
fn pause_blocks_credit_creation_but_not_removal() {
    let (mut w, u) = hedged();
    let admin = w.admin_pk();
    let admin_kp = w.admin.insecure_clone();
    let i = pause_ix(&admin);
    assert_ok(&w.env.send(&[i], &[&admin_kp]));
    assert_ch_err(&w.refresh(&u), E::ProgramPaused);
    // Snapshot writes (which invalidate) and consent changes still work.
    assert_ok(&w.submit(&u, 1, ETH, false, IM / 2));
    assert_eq!(w.usable_total(&u), 0);
    assert_ok(&w.invalidate(&u, false)); // releases the counters while paused
    assert_eq!(w.config().global_outstanding_credit, 0);
    assert_ok(&w.set_consent(&u, false, 0));
}

#[test]
fn snapshot_validation_rejects_contradictory_inputs() {
    let mut w = World::new(2);
    let u = w.user();
    assert_ch_err(&w.submit_full(&u, 0, SOL, 0, true, 0, 0), E::ZeroNotional);
    assert_ch_err(
        &w.submit_full(&u, 0, SOL, 100, true, 50, 60),
        E::InvalidMargin,
    );
    assert_ch_err(
        &w.submit_full(&u, 0, SOL, 100, true, 150, 60),
        E::InvalidMargin,
    );
    assert_ch_err(
        &w.submit_full(&u, 0, SOL, 100, true, 50, 0),
        E::InvalidMargin,
    );
    assert_ch_err(
        &w.submit_full(&u, 0, 32, 100, true, 50, 25),
        E::InvalidAssetId,
    );

    // Wrong venue authority.
    let user = kp_pubkey(&u);
    let mallory = w.env.funded(1);
    let mut i = w.submit_ix(&user, 0, SOL, 100, true, 50, 25);
    let m = kp_pubkey(&mallory);
    for meta in i.accounts.iter_mut() {
        if meta.pubkey == kp_pubkey(&w.venues[0].authority) {
            meta.pubkey = m;
        }
    }
    assert_ch_err(&w.env.send(&[i], &[&mallory]), E::Unauthorized);

    // Venue not in the user's consent bitmap.
    assert_ok(&w.set_consent(&u, true, 0b10));
    assert_ch_err(&w.submit(&u, 0, SOL, true, IM), E::VenueNotAuthorized);
}

#[test]
fn portfolio_is_bounded_at_max_legs() {
    let mut w = World::new(9);
    let u = w.user();
    for v in 0..8 {
        assert_ok(&w.submit(&u, v, (v % 4) as u8, v % 2 == 0, IM));
    }
    assert_ch_err(&w.submit(&u, 8, SOL, true, IM), E::PortfolioFull);
    // Updating an existing leg is still allowed when full.
    assert_ok(&w.submit(&u, 0, SOL, true, IM / 2));
}

#[test]
fn only_venue_or_user_without_consent_may_close_a_snapshot() {
    let (mut w, u) = hedged();
    let user = kp_pubkey(&u);
    let mallory = w.env.funded(1);
    let i = w.close_ix(&user, 1, ETH, &kp_pubkey(&mallory));
    assert_ch_err(&w.env.send(&[i], &[&mallory]), E::SnapshotCloseNotAllowed);
    // The user cannot drop a leg of a venue they still consent to.
    let i = w.close_ix(&user, 1, ETH, &user);
    assert_ch_err(&w.env.send(&[i], &[&u]), E::SnapshotCloseNotAllowed);
    assert_eq!(w.portfolio(&u).status, status::ACTIVE);
}

#[test]
fn snapshot_versions_are_monotonic() {
    let mut w = World::new(1);
    let u = w.user();
    let user = kp_pubkey(&u);
    for k in 1..=3u64 {
        assert_ok(&w.submit(&u, 0, SOL, true, IM + k));
        let s: ch_core::PosSnapshot = w
            .env
            .account(&snapshot_pda(&user, &w.venues[0].id, SOL))
            .unwrap();
        assert_eq!(s.version, k);
    }
    let _ = Pubkey::default();
}
