//! F8: venue registry. Regression tests run against the compiled program.

use anchor_lang::prelude::Pubkey;
use ch_core::{ClearinghouseError as E, GlobalConfig, VenueRegistration};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

struct Fx {
    env: Env,
    admin: Kp,
    a: [u8; 32],
    b: [u8; 32],
}

fn fixture() -> Fx {
    let mut env = Env::new();
    let admin = env.funded(10);
    assert_ok(&initialize(&mut env, &admin));
    let a = venue_id("venue-a");
    let b = venue_id("venue-b");
    let admin_pk = kp_pubkey(&admin);
    for (id, idx) in [(a, 0u8), (b, 1u8)] {
        let i = register_venue_ix(&admin_pk, None, id, Pubkey::new_unique(), idx);
        assert_ok(&env.send(&[i], &[&admin]));
    }
    Fx { env, admin, a, b }
}

/// System program "account already in use" surfaces as Custom(0).
const SYSTEM_ACCOUNT_ALREADY_IN_USE: u32 = 0;

#[test]
fn f8a_reregister_existing_venue_at_other_index_is_rejected() {
    let mut fx = fixture();
    let before: VenueRegistration = fx.env.account(&venue_pda(&fx.a)).unwrap();
    let attacker_auth = Pubkey::new_unique();
    let i = register_venue_ix(&kp_pubkey(&fx.admin), None, fx.a, attacker_auth, 1);
    let res = fx.env.send(&[i], &[&fx.admin]);
    assert_code(&res, SYSTEM_ACCOUNT_ALREADY_IN_USE);

    let after: VenueRegistration = fx.env.account(&venue_pda(&fx.a)).unwrap();
    assert_eq!(after.venue_index, before.venue_index);
    assert_eq!(after.venue_authority, before.venue_authority);
    let b: VenueRegistration = fx.env.account(&venue_pda(&fx.b)).unwrap();
    assert_eq!(b.venue_index, 1);
}

#[test]
fn f8a_reregister_existing_venue_at_own_index_is_rejected() {
    let mut fx = fixture();
    let i = register_venue_ix(&kp_pubkey(&fx.admin), None, fx.a, Pubkey::new_unique(), 0);
    assert_code(
        &fx.env.send(&[i], &[&fx.admin]),
        SYSTEM_ACCOUNT_ALREADY_IN_USE,
    );
}

#[test]
fn f8a_new_venue_cannot_take_used_index() {
    let mut fx = fixture();
    let c = venue_id("venue-c");
    let i = register_venue_ix(&kp_pubkey(&fx.admin), None, c, Pubkey::new_unique(), 0);
    assert_ch_err(&fx.env.send(&[i], &[&fx.admin]), E::VenueIndexAlreadyUsed);
    assert!(
        !fx.env.exists(&venue_pda(&c)),
        "failed tx must not create the venue"
    );
}

#[test]
fn register_rejects_index_out_of_range() {
    let mut fx = fixture();
    let c = venue_id("venue-c");
    let i = register_venue_ix(&kp_pubkey(&fx.admin), None, c, Pubkey::new_unique(), 10);
    assert_ch_err(&fx.env.send(&[i], &[&fx.admin]), E::InvalidVenueIndex);
}

#[test]
fn register_rejects_wrong_pda_for_venue_id() {
    let mut fx = fixture();
    let c = venue_id("venue-c");
    let wrong = venue_pda(&venue_id("not-venue-c"));
    let i = register_venue_ix(
        &kp_pubkey(&fx.admin),
        Some(wrong),
        c,
        Pubkey::new_unique(),
        3,
    );
    assert_anchor_err(
        &fx.env.send(&[i], &[&fx.admin]),
        anchor_lang::error::ErrorCode::ConstraintSeeds,
    );
}

#[test]
fn register_requires_admin() {
    let mut fx = fixture();
    let mallory = fx.env.funded(10);
    let c = venue_id("venue-c");
    let i = register_venue_ix(&kp_pubkey(&mallory), None, c, Pubkey::new_unique(), 3);
    assert_ch_err(&fx.env.send(&[i], &[&mallory]), E::Unauthorized);
}

#[test]
fn f8b_set_venue_active_toggles_flag_and_bitmap_and_keeps_index_reserved() {
    let mut fx = fixture();
    let admin = kp_pubkey(&fx.admin);

    let cfg: GlobalConfig = fx.env.account(&config_pda()).unwrap();
    assert_eq!(cfg.active_venues_bitmap, 0b11);
    assert_eq!(cfg.used_venues_bitmap, 0b11);

    assert_ok(
        &fx.env
            .send(&[set_venue_active_ix(&admin, &fx.b, false)], &[&fx.admin]),
    );
    let cfg: GlobalConfig = fx.env.account(&config_pda()).unwrap();
    assert_eq!(cfg.active_venues_bitmap, 0b01);
    assert_eq!(cfg.used_venues_bitmap, 0b11, "index stays reserved");
    let v: VenueRegistration = fx.env.account(&venue_pda(&fx.b)).unwrap();
    assert!(!v.is_active);

    // The deactivated index cannot be handed to a new venue.
    let c = venue_id("venue-c");
    let i = register_venue_ix(&admin, None, c, Pubkey::new_unique(), 1);
    assert_ch_err(&fx.env.send(&[i], &[&fx.admin]), E::VenueIndexAlreadyUsed);

    assert_ok(
        &fx.env
            .send(&[set_venue_active_ix(&admin, &fx.b, true)], &[&fx.admin]),
    );
    let cfg: GlobalConfig = fx.env.account(&config_pda()).unwrap();
    assert_eq!(cfg.active_venues_bitmap, 0b11);
}

#[test]
fn f8b_set_venue_active_requires_admin() {
    let mut fx = fixture();
    let mallory = fx.env.funded(10);
    let i = set_venue_active_ix(&kp_pubkey(&mallory), &fx.a, false);
    assert_ch_err(&fx.env.send(&[i], &[&mallory]), E::Unauthorized);
    let v: VenueRegistration = fx.env.account(&venue_pda(&fx.a)).unwrap();
    assert!(v.is_active);
}
