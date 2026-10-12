//! Stage D: separated roles, bounds, per-update step limits, the timelock for
//! credit-increasing changes, guardian powers, and timelocked venue activation.

use anchor_lang::prelude::Pubkey;
use ch_core::{ClearinghouseError as E, ConfigParams, RiskChange};
use ch_math::risk::scenario_kind;
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const IM: u64 = 10_000_000_000;
const TIMELOCK: u64 = 100;

fn pending(w: &World) -> ch_core::PendingChange {
    w.env
        .account(&pending_change_pda())
        .expect("pending account")
}

/// World with a hedged user, then the timelock raised to TIMELOCK (raising it
/// is credit-reducing, so it applies at once).
fn timelocked() -> (World, Kp) {
    let mut w = World::new(3);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    let mut p = w.config().params;
    p.timelock_slots = TIMELOCK;
    let admin = w.admin_pk();
    assert_ok(&w.admin_send(update_params_ix(&admin, p)));
    assert_eq!(pending(&w).change, RiskChange::None);
    // The parameter change bumped the epoch; recompute so credit is live.
    assert_ok(&w.refresh(&u));
    (w, u)
}

fn with(p: ConfigParams, f: impl Fn(&mut ConfigParams)) -> ConfigParams {
    let mut q = p;
    f(&mut q);
    q
}

#[test]
fn roles_are_separate_and_rotatable() {
    let mut w = World::new(1);
    let admin = w.admin_pk();
    let risk = w.env.funded(1);
    let vm = w.env.funded(1);
    let guard = w.env.funded(1);
    let (rk, vk, gk) = (kp_pubkey(&risk), kp_pubkey(&vm), kp_pubkey(&guard));
    assert_ok(&w.admin_send(set_roles_ix(&admin, rk, vk, gk)));
    let c = w.config();
    assert_eq!(
        (c.risk_authority, c.venue_manager, c.guardian),
        (rk, vk, gk)
    );

    let p = with(c.params, |p| p.haircut_bps = 3_000);
    // The admin no longer holds the risk role.
    assert_ch_err(&w.admin_send(update_params_ix(&admin, p)), E::Unauthorized);
    // Venue manager and guardian cannot change risk parameters either.
    for k in [&vm, &guard] {
        let r = w.env.send(&[update_params_ix(&kp_pubkey(k), p)], &[k]);
        assert_ch_err(&r, E::Unauthorized);
    }
    assert_ok(&w.env.send(&[update_params_ix(&rk, p)], &[&risk]));

    // Only the venue manager registers venues.
    let id = venue_id("venue-x");
    let r = w.env.send(
        &[register_venue_ix(&rk, None, id, Pubkey::new_unique(), 5)],
        &[&risk],
    );
    assert_ch_err(&r, E::Unauthorized);
    assert_ok(&w.env.send(
        &[register_venue_ix(&vk, None, id, Pubkey::new_unique(), 5)],
        &[&vm],
    ));

    // Guardian pauses; only the admin unpauses.
    assert_ch_err(&w.env.send(&[pause_ix(&vk)], &[&vm]), E::Unauthorized);
    assert_ok(&w.env.send(&[pause_ix(&gk)], &[&guard]));
    assert!(w.config().paused);
    assert_ch_err(&w.env.send(&[unpause_ix(&gk)], &[&guard]), E::Unauthorized);
    assert_ok(&w.admin_send(unpause_ix(&admin)));
    assert!(!w.config().paused);

    // Only the admin rotates roles.
    let r = w.env.send(&[set_roles_ix(&gk, gk, gk, gk)], &[&guard]);
    assert_ch_err(&r, E::Unauthorized);
}

#[test]
fn credit_increasing_change_waits_for_the_timelock() {
    let (mut w, u) = timelocked();
    let admin = w.admin_pk();
    let before = w.config();
    let lower_haircut = with(before.params, |p| p.haircut_bps = 1_500);
    assert_ok(&w.admin_send(update_params_ix(&admin, lower_haircut)));
    // Queued: nothing changed, credit still usable.
    let c = w.config();
    assert_eq!(c.params.haircut_bps, before.params.haircut_bps);
    assert_eq!(c.risk_epoch, before.risk_epoch);
    assert!(w.usable_total(&u) > 0);
    let pc = pending(&w);
    assert_eq!(pc.change, RiskChange::Params(lower_haircut));
    assert_eq!(pc.eta_slot, w.env.clock().slot + TIMELOCK);

    assert_ch_err(
        &w.admin_send(apply_pending_ix(&admin)),
        E::TimelockNotElapsed,
    );
    w.env.advance_slots(TIMELOCK);
    assert_ok(&w.admin_send(apply_pending_ix(&admin)));
    let c = w.config();
    assert_eq!(c.params.haircut_bps, 1_500);
    assert_eq!(c.risk_epoch, before.risk_epoch + 1);
    assert_eq!(pending(&w).change, RiskChange::None);
    assert_ch_err(&w.admin_send(apply_pending_ix(&admin)), E::NoPendingChange);
}

#[test]
fn credit_reducing_changes_apply_immediately() {
    let (mut w, u) = timelocked();
    let admin = w.admin_pk();
    let e0 = w.config().risk_epoch;
    // Higher haircut, lower global cap.
    let stricter = with(w.config().params, |p| {
        p.haircut_bps = 3_000;
        p.global_credit_cap /= 2;
    });
    assert_ok(&w.admin_send(update_params_ix(&admin, stricter)));
    assert_eq!(w.config().params.haircut_bps, 3_000);
    // A scenario in an empty slot, more correlation stress, wider gaps, and
    // disabling an asset.
    let i = w.set_scenario_ix(0, scenario_kind::HYPOTHETICAL, &[(SOL, -2_000)]);
    assert_ok(&w.admin_send(i));
    let i = w.set_stress_ix(200_000, 20_000);
    assert_ok(&w.admin_send(i));
    let i = w.set_asset_risk_ix(SOL, 1_000, 2_000, 100);
    assert_ok(&w.admin_send(i));
    let i = w.set_asset_risk_ix(BTC, 0, 0, 0);
    assert_ok(&w.admin_send(i));
    assert_eq!(pending(&w).change, RiskChange::None, "nothing queued");
    assert_eq!(w.config().risk_epoch, e0 + 5);
    assert_eq!(
        w.usable_total(&u),
        0,
        "each change invalidated outstanding credit"
    );
}

#[test]
fn correlation_and_shock_changes_are_always_queued() {
    let (mut w, _u) = timelocked();
    let e0 = w.config().risk_epoch;
    // Even a "tighter-looking" correlation change can raise some book's credit.
    let i = w.set_corr_ix(&[(SOL, ETH, 700_000)]);
    assert_ok(&w.admin_send(i));
    assert!(matches!(pending(&w).change, RiskChange::Correlations(_)));
    let admin = w.admin_pk();
    assert_ok(&w.admin_send(cancel_pending_ix(&admin)));
    let i = w.set_asset_risk_ix(SOL, 1_200, 1_200, 100);
    assert_ok(&w.admin_send(i));
    assert!(matches!(pending(&w).change, RiskChange::AssetRisk { .. }));
    assert_eq!(
        w.config().risk_epoch,
        e0,
        "queued changes do not touch state"
    );
}

#[test]
fn only_one_change_can_be_pending_and_the_guardian_can_cancel_it() {
    let (mut w, _u) = timelocked();
    let admin = w.admin_pk();
    let guard = w.env.funded(1);
    let gk = kp_pubkey(&guard);
    let c = w.config();
    assert_ok(&w.admin_send(set_roles_ix(&admin, admin, admin, gk)));

    let p1 = with(c.params, |p| p.haircut_bps = 1_500);
    assert_ok(&w.admin_send(update_params_ix(&admin, p1)));
    let i = w.set_corr_ix(&[(SOL, ETH, 900_000)]);
    assert_ch_err(&w.admin_send(i), E::PendingChangeExists);

    // A random key cannot cancel; the guardian can.
    let mallory = w.env.funded(1);
    let r = w
        .env
        .send(&[cancel_pending_ix(&kp_pubkey(&mallory))], &[&mallory]);
    assert_ch_err(&r, E::Unauthorized);
    assert_ok(&w.env.send(&[cancel_pending_ix(&gk)], &[&guard]));
    assert_eq!(pending(&w).change, RiskChange::None);
    w.env.advance_slots(TIMELOCK);
    assert_ch_err(&w.admin_send(apply_pending_ix(&admin)), E::NoPendingChange);
    // The guardian cannot apply.
    assert_ok(&w.admin_send(update_params_ix(&admin, p1)));
    w.env.advance_slots(TIMELOCK);
    assert_ch_err(
        &w.env.send(&[apply_pending_ix(&gk)], &[&guard]),
        E::Unauthorized,
    );
}

#[test]
fn bounds_and_step_limits_are_enforced() {
    let (mut w, _u) = timelocked();
    let admin = w.admin_pk();
    let c = w.config().params;
    for (bad, err) in [
        (with(c, |p| p.haircut_bps = 400), E::InvalidConfigParams),
        (
            with(c, |p| p.max_credit_bps_of_im = 9_500),
            E::InvalidConfigParams,
        ),
        (with(c, |p| p.max_move_bps = 2_500), E::InvalidConfigParams),
        (with(c, |p| p.haircut_bps = 900), E::ChangeTooLarge), // 2000 -> 900: -1100
        (
            with(c, |p| p.global_credit_cap = c.global_credit_cap * 3),
            E::ChangeTooLarge,
        ),
    ] {
        assert_ch_err(&w.admin_send(update_params_ix(&admin, bad)), err);
    }
    let i = w.set_corr_ix(&[(SOL, ETH, 449_999)]); // 0.8 -> 0.45: step 0.35
    assert_ch_err(&w.admin_send(i), E::ChangeTooLarge);
    let i = w.set_asset_risk_ix(SOL, 499, 499, 100); // 1000 -> 499: below half
    assert_ch_err(&w.admin_send(i), E::ChangeTooLarge);
    let i = w.set_asset_risk_ix(SOL, 2_001, 2_001, 100); // more than double
    assert_ch_err(&w.admin_send(i), E::ChangeTooLarge);
}

#[test]
fn pending_change_is_revalidated_against_the_state_at_apply_time() {
    let (mut w, _u) = timelocked();
    let admin = w.admin_pk();
    let c = w.config().params;
    // Queue 2000 -> 1500 (a 500 bps cut, within the step limit) ...
    assert_ok(&w.admin_send(update_params_ix(&admin, with(c, |p| p.haircut_bps = 1_500))));
    // ... then raise the haircut to 3000 immediately (credit-reducing).
    let stricter = with(c, |p| p.haircut_bps = 3_000);
    assert_ok(&w.admin_send(update_params_ix(&admin, stricter)));
    w.env.advance_slots(TIMELOCK);
    // 3000 -> 1500 now exceeds the step limit: the stale proposal is refused.
    assert_ch_err(&w.admin_send(apply_pending_ix(&admin)), E::ChangeTooLarge);
    assert_eq!(w.config().params.haircut_bps, 3_000);
}

/// One compromised risk key cannot grant credit before the guardian can react:
/// its credit-increasing changes sit in the queue, and the guardian can cancel
/// them and invalidate everything.
#[test]
fn compromised_risk_key_is_contained_by_timelock_and_guardian() {
    let (mut w, u) = timelocked();
    let admin = w.admin_pk();
    let attacker = w.env.funded(1);
    let guard = w.env.funded(1);
    let (ak, gk) = (kp_pubkey(&attacker), kp_pubkey(&guard));
    assert_ok(&w.admin_send(set_roles_ix(&admin, ak, admin, gk)));
    let before = w.portfolio(&u).total_credit;

    let c = w.config().params;
    let loosen = with(c, |p| {
        p.haircut_bps = 1_000;
        p.max_credit_per_user = c.max_credit_per_user * 2;
    });
    assert_ok(&w.env.send(&[update_params_ix(&ak, loosen)], &[&attacker]));
    // Nothing changed yet, and a refresh still yields the old credit.
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).total_credit, before);

    assert_ok(&w.env.send(&[cancel_pending_ix(&gk)], &[&guard]));
    assert_ok(&w.env.send(&[bump_epoch_ix(&gk)], &[&guard]));
    assert_eq!(w.usable_total(&u), 0);
    // The guardian's emergency powers cannot create credit: no param change.
    assert_eq!(w.config().params, c);
}

#[test]
fn new_venues_activate_only_after_the_timelock_and_deactivate_immediately() {
    let (mut w, _u) = timelocked();
    let admin = w.admin_pk();
    let auth = w.env.funded(1);
    let id = venue_id("venue-new");
    assert_ok(&w.admin_send(register_venue_ix(&admin, None, id, kp_pubkey(&auth), 9)));
    let v: ch_core::VenueRegistration = w.env.account(&venue_pda(&id)).unwrap();
    assert!(!v.is_active);
    assert_eq!(v.activation_eta_slot, w.env.clock().slot + TIMELOCK);
    assert_eq!(w.config().active_venues_bitmap & (1 << 9), 0);

    assert_ch_err(
        &w.admin_send(activate_venue_ix(&admin, &id)),
        E::TimelockNotElapsed,
    );
    w.env.advance_slots(TIMELOCK);
    let e0 = w.config().risk_epoch;
    assert_ok(&w.admin_send(activate_venue_ix(&admin, &id)));
    assert_ne!(w.config().active_venues_bitmap & (1 << 9), 0);
    assert_eq!(w.config().risk_epoch, e0 + 1);

    // Guardian deactivation is immediate; re-activation waits again.
    let guard = w.env.funded(1);
    let gk = kp_pubkey(&guard);
    assert_ok(&w.admin_send(set_roles_ix(&admin, admin, admin, gk)));
    assert_ok(&w.env.send(&[deactivate_venue_ix(&gk, &id)], &[&guard]));
    assert_eq!(w.config().active_venues_bitmap & (1 << 9), 0);
    // The guardian cannot (re)activate.
    let r = w.env.send(&[request_activation_ix(&gk, &id)], &[&guard]);
    assert_ch_err(&r, E::Unauthorized);
    assert_ok(&w.admin_send(request_activation_ix(&admin, &id)));
    assert_ch_err(
        &w.admin_send(activate_venue_ix(&admin, &id)),
        E::TimelockNotElapsed,
    );
}

/// A3: raising `hedge_break_bps` can only add scenario loss, so it applies at
/// once (and bumps the epoch); lowering it waits for the timelock.
#[test]
fn hedge_break_increase_applies_immediately() {
    let (mut w, u) = timelocked();
    let admin = w.admin_pk();
    let before = w.config();
    assert_eq!(before.params.hedge_break_bps, 5_000);
    let higher = with(before.params, |p| p.hedge_break_bps = 7_500);
    assert_ok(&w.admin_send(update_params_ix(&admin, higher)));
    let c = w.config();
    assert_eq!(c.params.hedge_break_bps, 7_500);
    assert_eq!(c.risk_epoch, before.risk_epoch + 1);
    assert_eq!(pending(&w).change, RiskChange::None);
    assert_eq!(
        w.usable_total(&u),
        0,
        "old credit void after the epoch bump"
    );
}

#[test]
fn hedge_break_decrease_waits_for_timelock() {
    let (mut w, u) = timelocked();
    let admin = w.admin_pk();
    let before = w.config();
    let lower = with(before.params, |p| p.hedge_break_bps = 2_500);
    assert_ok(&w.admin_send(update_params_ix(&admin, lower)));
    assert_eq!(
        w.config().params.hedge_break_bps,
        5_000,
        "queued, not applied"
    );
    assert_eq!(pending(&w).change, RiskChange::Params(lower));
    assert!(w.usable_total(&u) > 0);
    assert_ch_err(
        &w.admin_send(apply_pending_ix(&admin)),
        E::TimelockNotElapsed,
    );
    w.env.advance_slots(TIMELOCK);
    assert_ok(&w.admin_send(apply_pending_ix(&admin)));
    assert_eq!(w.config().params.hedge_break_bps, 2_500);
    // Out of bounds is rejected outright (mock-oracle builds may use 0).
    let bad = with(w.config().params, |p| p.hedge_break_bps = 2_499);
    assert_ch_err(
        &w.admin_send(update_params_ix(&admin, bad)),
        E::InvalidConfigParams,
    );
    let bad = with(w.config().params, |p| p.hedge_break_bps = 10_001);
    assert_ch_err(
        &w.admin_send(update_params_ix(&admin, bad)),
        E::InvalidConfigParams,
    );
}
