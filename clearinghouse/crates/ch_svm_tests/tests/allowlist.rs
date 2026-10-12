//! A1: counterparty allowlists on the compiled program. A venue applies credit
//! only from reports of venues it accepts (plus itself); removing a venue
//! voids the affected credit in the same instruction, without an epoch bump.

use ch_core::{status, ClearinghouseError as E};
use ch_math::portfolio::PortfolioLeg;
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const IM: u64 = 10_000_000_000; // $10,000

fn leg(v: u8, a: u8, long: bool) -> PortfolioLeg {
    PortfolioLeg {
        venue_index: v,
        asset: a,
        is_long: long,
        notional: 5 * IM,
        initial_margin: IM,
        maintenance_margin: IM / 2,
    }
}

/// Three venues; nobody allowlists anybody (the program's default).
fn isolated() -> World {
    let mut w = World::new(3);
    for v in 0..3 {
        assert_ok(&w.set_acceptance(v, 0));
    }
    w
}

#[test]
fn fresh_config_accepts_only_own_reports() {
    let mut env = Env::new();
    let admin = env.funded(10);
    let ak = kp_pubkey(&admin);
    assert_ok(&env.send(&[initialize_ix(&ak, default_params())], &[&admin]));
    let c: ch_core::GlobalConfig = env.account(&config_pda()).unwrap();
    assert!(c.venue_accepts.iter().all(|m| *m == 0));
    // With the default, a cross-venue hedge earns no usable credit.
    let mut w = isolated();
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).status, status::ACTIVE);
    assert_eq!(w.usable_total(&u), 0);
    // Same-venue hedges need no allowlist.
    let u2 = w.user();
    assert_ok(&w.submit(&u2, 0, SOL, true, IM));
    assert_ok(&w.submit(&u2, 0, ETH, false, IM));
    assert_ok(&w.refresh(&u2));
    assert!(w.usable_total(&u2) > 0);
}

/// Venue 2 is malicious: it reports a fictitious short SOL that offsets the
/// long SOL on honest venue 0, and allowlists everybody. Venue 0 has not
/// allowlisted venue 2, so it gets 0 from that report.
#[test]
fn malicious_venue_fake_offset_gives_zero_to_non_allowlisting_venue() {
    let mut w = isolated();
    assert_ok(&w.set_acceptance(2, u64::MAX));
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 2, SOL, false, IM)); // fake offset
    assert_ok(&w.refresh(&u));
    let legs = [leg(0, SOL, true), leg(2, SOL, false)];
    let mut acc = [0u64; 64];
    acc[2] = u64::MAX;
    let want = expected_view_credit(&legs, 800_000, &default_params(), &acc);
    assert!(want.joint.leg_credit[0] > 0, "the joint book looks hedged");
    assert_eq!(want.leg_credit[0], 0);
    assert_eq!(
        w.usable(&u, 0, SOL),
        0,
        "honest venue 0 gets nothing from venue 2"
    );
    let p = w.portfolio(&u);
    let l0 = p.legs[..p.num_legs as usize]
        .iter()
        .find(|l| l.venue_index == 0)
        .unwrap();
    assert_eq!(l0.view_mask, 0b001);
    assert_eq!(w.usable(&u, 2, SOL), want.leg_credit[1]);
}

#[test]
fn allowlisting_restores_credit_after_refresh() {
    let mut w = isolated();
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    assert_eq!(w.usable(&u, 0, SOL), 0);
    // Venue 0 accepts venue 1: nothing changes until the next refresh.
    assert_ok(&w.set_acceptance(0, 0b010));
    assert_eq!(w.usable(&u, 0, SOL), 0);
    assert_ok(&w.refresh(&u));
    let joint = expected_credit(
        &[leg(0, SOL, true), leg(1, ETH, false)],
        800_000,
        &[],
        &default_params(),
    );
    assert_eq!(w.usable(&u, 0, SOL), joint.leg_credit[0]);
    assert_eq!(w.usable(&u, 1, ETH), 0, "venue 1 still accepts only itself");
    assert_ok(&w.set_acceptance(1, 0b001));
    assert_ok(&w.refresh(&u));
    assert_eq!(w.usable_total(&u), 7_111_111_110, "worked example");
}

#[test]
fn unallowlisting_removes_credit_in_the_same_instruction() {
    let mut w = World::new(2); // both allowlist each other
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    assert!(w.usable(&u, 0, SOL) > 0);
    let epoch = w.config().risk_epoch;

    assert_ok(&w.set_acceptance(0, 0)); // venue 0 drops venue 1
    assert_eq!(w.usable(&u, 0, SOL), 0, "void at once");
    assert!(w.usable(&u, 1, ETH) > 0, "venue 1 still accepts venue 0");
    assert_eq!(w.config().risk_epoch, epoch, "no global epoch bump");
    assert_eq!(w.portfolio(&u).status, status::ACTIVE);
}

#[test]
fn acceptance_revoked_is_a_valid_invalidation_reason_and_releases_counters() {
    let mut w = World::new(2);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    // Nothing justifies an invalidation yet.
    assert_ch_err(&w.invalidate(&u, false), E::InvalidationNotJustified);
    assert_ok(&w.set_acceptance(1, 0));
    let c0 = w.config();
    assert_eq!(c0.global_outstanding_credit, 7_111_111_110);
    assert_ok(&w.invalidate(&u, false));
    let c1 = w.config();
    assert_eq!(c1.global_outstanding_credit, 0);
    assert_eq!((c1.venue_outstanding[0], c1.venue_outstanding[1]), (0, 0));
    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);
    assert_ch_err(&w.invalidate(&u, false), E::InvalidationNotJustified);
}

#[test]
fn only_the_venue_authority_can_set_acceptance_and_it_works_while_paused() {
    let mut w = World::new(2);
    // Venue 1's authority signing for venue 0.
    let i = set_acceptance_ix(&w.venues[0].id, &kp_pubkey(&w.venues[1].authority), 0);
    let other = w.venues[1].authority.insecure_clone();
    assert_ch_err(&w.env.send(&[i], &[&other]), E::Unauthorized);
    // Pausing does not block it: it never creates credit by itself.
    let admin = w.admin_pk();
    let i = ix(
        ch_core::accounts::GuardianAction {
            config: config_pda(),
            authority: admin,
        },
        ch_core::instruction::Pause {},
    );
    assert_ok(&w.admin_send(i));
    assert_ok(&w.set_acceptance(0, 0b10));
    // The own bit is stored cleared (canonical form).
    assert_ok(&w.set_acceptance(0, u64::MAX));
    assert_eq!(w.config().venue_accepts[0], !1u64);
}

/// F3 under views: a short SOL on venue 1 must not back both the long on
/// venue 0 (in 0's view) and the long on venue 2 (in 2's view).
#[test]
fn one_hedge_is_not_used_twice_across_views() {
    let mut w = isolated();
    assert_ok(&w.set_acceptance(0, 0b010));
    assert_ok(&w.set_acceptance(2, 0b010));
    assert_ok(&w.set_acceptance(1, 0b101));
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, SOL, false, IM));
    assert_ok(&w.submit(&u, 2, SOL, true, IM));
    assert_ok(&w.refresh(&u));
    let legs = [leg(0, SOL, true), leg(1, SOL, false), leg(2, SOL, true)];
    let mut acc = [0u64; 64];
    acc[0] = 0b010;
    acc[1] = 0b101;
    acc[2] = 0b010;
    let want = expected_view_credit(&legs, 800_000, &default_params(), &acc);
    for (i, v) in [0u8, 1, 2].into_iter().enumerate() {
        assert_eq!(w.usable(&u, v, SOL), want.leg_credit[i], "leg {i}");
        assert!(want.leg_credit[i] <= want.joint.leg_credit[i]);
    }
    assert!(w.usable_total(&u) <= want.joint.total());
    assert!(w.usable_total(&u) <= default_params().max_credit_per_user);
}
