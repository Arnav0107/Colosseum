//! B3: regime-aware credit throttle on the compiled program. Credit scales
//! down linearly as a leg's price moves away from the oracle EMA (start 50,
//! full 150 bps by default) or its confidence widens (start 50 bps, full at
//! max_conf_bps = 100).

use ch_core::{portfolio_flags, ConfigParams};
use ch_math::portfolio::PortfolioLeg;
use ch_math::throttle::{apply_scale, leg_scale_ppm, ThrottleParams};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const IM: u64 = 10_000_000_000;
const SOL_PX: u64 = 150_000_000;

fn hedged() -> (World, Kp) {
    let mut w = World::new(2);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    (w, u)
}

fn unthrottled_legs() -> [u64; 2] {
    let leg = |v: u8, a: u8, long: bool| PortfolioLeg {
        venue_index: v,
        asset: a,
        is_long: long,
        notional: 5 * IM,
        initial_margin: IM,
        maintenance_margin: IM / 2,
    };
    let c = expected_credit(
        &[leg(0, SOL, true), leg(1, ETH, false)],
        800_000,
        &[],
        &default_params(),
    );
    [c.leg_credit[0], c.leg_credit[1]]
}

fn throttle_params(p: &ConfigParams) -> ThrottleParams {
    ThrottleParams {
        move_start_bps: p.throttle_move_start_bps,
        move_full_bps: p.throttle_move_full_bps,
        conf_start_bps: p.throttle_conf_start_bps,
        conf_full_bps: p.max_conf_bps,
    }
}

#[test]
fn throttle_scales_credit_linearly_between_thresholds() {
    let (mut w, u) = hedged();
    assert_ok(&w.refresh(&u));
    let full = unthrottled_legs();
    assert_eq!(w.usable(&u, 0, SOL), full[0], "price = EMA: no throttle");
    assert_eq!(w.portfolio(&u).flags & portfolio_flags::THROTTLED, 0);

    // SOL trades ~1% above its EMA: between start (50) and full (150).
    let ts = w.env.clock().unix_timestamp;
    let ema = 148_500_000;
    w.set_price_ema(SOL, SOL_PX, SOL_PX / 10_000, ts, ema);
    assert_ok(&w.refresh(&u));
    let scale = leg_scale_ppm(
        SOL_PX,
        ema,
        SOL_PX / 10_000,
        &throttle_params(&default_params()),
    );
    assert!(scale > 0 && scale < 1_000_000, "partial: {scale}");
    assert_eq!(scale, 480_000, "102 bps move -> (150 - 102) / 100");
    assert_eq!(w.usable(&u, 0, SOL), apply_scale(full[0], scale));
    assert_eq!(
        w.usable(&u, 1, ETH),
        apply_scale(full[1], scale),
        "the whole portfolio scales"
    );
    assert_ne!(w.portfolio(&u).flags & portfolio_flags::THROTTLED, 0);
}

#[test]
fn throttle_zero_at_full_move() {
    let (mut w, u) = hedged();
    let ts = w.env.clock().unix_timestamp;
    // 2% from the EMA, past the 150 bps full threshold (the 3% price guard
    // is about the stored baseline, so refresh still succeeds).
    w.set_price_ema(SOL, SOL_PX, SOL_PX / 10_000, ts, 147_000_000);
    assert_ok(&w.refresh(&u));
    assert_eq!(w.usable_total(&u), 0);
    assert_ne!(w.portfolio(&u).flags & portfolio_flags::THROTTLED, 0);
    // EMA catches up: full credit again.
    w.set_price_ema(SOL, SOL_PX, SOL_PX / 10_000, ts, SOL_PX);
    assert_ok(&w.refresh(&u));
    assert_eq!(w.usable_total(&u), unthrottled_legs().iter().sum::<u64>());
    assert_eq!(w.portfolio(&u).flags & portfolio_flags::THROTTLED, 0);
}

#[test]
fn wide_confidence_throttles() {
    let (mut w, u) = hedged();
    let ts = w.env.clock().unix_timestamp;
    // Confidence 75 bps: halfway between start (50) and max_conf_bps (100).
    let conf = SOL_PX * 75 / 10_000;
    w.set_price_ema(SOL, SOL_PX, conf, ts, SOL_PX);
    assert_ok(&w.refresh(&u));
    let full = unthrottled_legs();
    assert_eq!(w.usable(&u, 0, SOL), apply_scale(full[0], 500_000));
    assert_ne!(w.portfolio(&u).flags & portfolio_flags::THROTTLED, 0);
}

#[test]
fn throttle_threshold_lowering_applies_immediately_raising_waits() {
    let mut w = World::new(1);
    let admin = w.admin_pk();
    let mut p = w.config().params;
    p.timelock_slots = 100;
    assert_ok(&w.admin_send(update_params_ix(&admin, p)));
    let mut tighter = w.config().params;
    tighter.throttle_move_start_bps = 25;
    tighter.throttle_move_full_bps = 125;
    assert_ok(&w.admin_send(update_params_ix(&admin, tighter)));
    assert_eq!(
        w.config().params.throttle_move_full_bps,
        125,
        "applied at once"
    );
    let mut off = w.config().params;
    off.throttle_move_start_bps = 0;
    assert_ok(&w.admin_send(update_params_ix(&admin, off)));
    assert_eq!(
        w.config().params.throttle_move_start_bps,
        25,
        "turning it off is queued"
    );
}
