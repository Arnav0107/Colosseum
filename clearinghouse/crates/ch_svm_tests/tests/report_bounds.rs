//! A5: position-report sanity bounds on the compiled program. Each bound
//! rejects the report (fail closed) with its own error; each has a boundary
//! that is still accepted.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::AccountMeta;
use ch_core::{accounts as acc, instruction as ins, ClearinghouseError as E};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const M: u64 = 1_000_000; // $1 in micro-USD

/// A world whose report bounds are small enough to hit in a test:
/// leg cap $100k, per-(user, venue) cap $150k, growth 2x or +$10k per 150 slots.
fn bounded() -> World {
    let mut w = World::new(2);
    let mut p = w.config().params;
    p.max_leg_notional = 100_000 * M;
    p.max_venue_notional_per_user = 150_000 * M;
    p.max_notional_growth_bps = 10_000;
    p.notional_growth_window_slots = 150;
    p.notional_growth_floor = 10_000 * M;
    let admin = w.admin_pk();
    assert_ok(&w.admin_send(update_params_ix(&admin, p)));
    w
}

/// Report `notional` (IM 20%, MM 10%) with an optional mark price and oracle.
fn report(
    w: &mut World,
    user: &Kp,
    venue: usize,
    asset: u8,
    notional: u64,
    mark: u64,
    oracle: Option<Pubkey>,
) -> TxResult {
    let v = &w.venues[venue];
    let u = kp_pubkey(user);
    let mut i = ix(
        acc::SubmitPositionSnapshot {
            config: config_pda(),
            venue_registration: venue_pda(&v.id),
            user_consent: consent_pda(&u),
            portfolio: portfolio_pda(&u),
            pos_snapshot: snapshot_pda(&u, &v.id, asset),
            user: u,
            venue_authority: kp_pubkey(&v.authority),
            payer: kp_pubkey(&v.authority),
            system_program: anchor_lang::system_program::ID,
            risk_model: risk_model_pda(),
        },
        ins::SubmitPositionSnapshot {
            venue_id: v.id,
            asset_id: asset,
            notional,
            is_long: true,
            initial_margin: notional / 5,
            maintenance_margin: notional / 10,
            mark_price_micro: mark,
        },
    );
    if let Some(o) = oracle {
        i.accounts.push(AccountMeta::new_readonly(o, false));
    }
    let auth = v.authority.insecure_clone();
    w.env.send(&[i], &[&auth])
}

#[test]
fn leg_notional_cap_rejects() {
    let mut w = bounded();
    let u = w.user();
    // A new snapshot may only start at the growth floor ($10k); grow in steps.
    assert_ok(&report(&mut w, &u, 0, SOL, 10_000 * M, 0, None));
    w.env.advance_slots(150);
    assert_ok(&report(&mut w, &u, 0, SOL, 20_000 * M, 0, None));
    w.env.advance_slots(150);
    assert_ok(&report(&mut w, &u, 0, SOL, 40_000 * M, 0, None));
    w.env.advance_slots(150);
    assert_ok(&report(&mut w, &u, 0, SOL, 80_000 * M, 0, None));
    w.env.advance_slots(150);
    assert_ok(&report(&mut w, &u, 0, SOL, 100_000 * M, 0, None)); // == cap
    assert_ch_err(
        &report(&mut w, &u, 0, SOL, 100_000 * M + 1, 0, None),
        E::LegNotionalCapExceeded,
    );
}

#[test]
fn venue_notional_cap_rejects_sum() {
    let mut w = bounded();
    let u = w.user();
    let grow = |w: &mut World, u: &Kp, asset: u8, to: u64| {
        let mut n = 10_000 * M;
        assert_ok(&report(w, u, 0, asset, n, 0, None));
        while n < to {
            w.env.advance_slots(150);
            n = (2 * n).min(to);
            assert_ok(&report(w, u, 0, asset, n, 0, None));
        }
    };
    grow(&mut w, &u, SOL, 100_000 * M);
    grow(&mut w, &u, ETH, 40_000 * M);
    w.env.advance_slots(150);
    // SOL $100k + ETH $50k = $150k == cap: accepted. One more micro-dollar: rejected.
    assert_ok(&report(&mut w, &u, 0, ETH, 50_000 * M, 0, None));
    assert_ch_err(
        &report(&mut w, &u, 0, ETH, 50_000 * M + 1, 0, None),
        E::VenueNotionalCapExceeded,
    );
    // Another venue has its own budget.
    assert_ok(&report(&mut w, &u, 1, ETH, 10_000 * M, 0, None));
    // Shrinking is always allowed, and frees room on the venue.
    assert_ok(&report(&mut w, &u, 0, SOL, 60_000 * M, 0, None));
    assert_ok(&report(&mut w, &u, 0, ETH, 50_000 * M + 1, 0, None));
}

#[test]
fn notional_growth_limit_window_and_floor() {
    let mut w = bounded();
    let u = w.user();
    // New snapshot: base 0, so only the floor ($10k) is allowed.
    assert_ch_err(
        &report(&mut w, &u, 0, SOL, 10_000 * M + 1, 0, None),
        E::NotionalGrowthTooFast,
    );
    assert_ok(&report(&mut w, &u, 0, SOL, 10_000 * M, 0, None));
    // Same window (base 0): the floor still binds.
    assert_ch_err(
        &report(&mut w, &u, 0, SOL, 10_000 * M + 1, 0, None),
        E::NotionalGrowthTooFast,
    );
    // Window rolls over: base = $10k, limit = max(2 x $10k, $10k + $10k) = $20k.
    w.env.advance_slots(150);
    assert_ok(&report(&mut w, &u, 0, SOL, 20_000 * M, 0, None));
    assert_ch_err(
        &report(&mut w, &u, 0, SOL, 20_000 * M + 1, 0, None),
        E::NotionalGrowthTooFast,
    );
    // Close and reopen: base 0 again, no shortcut.
    let auth = w.venues[0].authority.insecure_clone();
    let up = kp_pubkey(&u);
    assert_ok(
        &w.env
            .send(&[w.close_ix(&up, 0, SOL, &kp_pubkey(&auth))], &[&auth]),
    );
    assert_ch_err(
        &report(&mut w, &u, 0, SOL, 20_000 * M, 0, None),
        E::NotionalGrowthTooFast,
    );
    // Growth limit off (0): only the caps apply.
    let mut p = w.config().params;
    p.max_notional_growth_bps = 0;
    let admin = w.admin_pk();
    assert_ok(&w.admin_send(update_params_ix(&admin, p)));
    assert_ok(&report(&mut w, &u, 0, SOL, 100_000 * M, 0, None));
}

#[test]
fn mark_price_must_agree_with_the_oracle() {
    let mut w = bounded();
    let u = w.user();
    let sol = Some(mock_price_pda(SOL));
    // Oracle SOL = $150; max_move_bps = 300 (3%).
    assert_eq!(w.config().params.max_move_bps, 300);
    assert_ok(&report(&mut w, &u, 0, SOL, 10_000 * M, 150 * M, sol));
    assert_ok(&report(&mut w, &u, 0, SOL, 10_000 * M, 154_500_000, sol)); // exactly 3%
    assert_ch_err(
        &report(&mut w, &u, 0, SOL, 10_000 * M, 154_500_001, sol),
        E::MarkPriceDeviation,
    );
    assert_ok(&report(&mut w, &u, 0, SOL, 10_000 * M, 145_500_000, sol)); // -3%
    assert_ch_err(
        &report(&mut w, &u, 0, SOL, 10_000 * M, 145_499_999, sol),
        E::MarkPriceDeviation,
    );
    // A mark without the oracle account.
    assert_ch_err(
        &report(&mut w, &u, 0, SOL, 10_000 * M, 150 * M, None),
        E::MarkPriceOracleMissing,
    );
    // Another asset's oracle is an error, never a pass.
    assert_ch_err(
        &report(
            &mut w,
            &u,
            0,
            SOL,
            10_000 * M,
            150 * M,
            Some(mock_price_pda(ETH)),
        ),
        E::InvalidAssetId,
    );
    // The mark is stored; 0 means "not reported".
    let s: ch_core::PosSnapshot = w
        .env
        .account(&snapshot_pda(&kp_pubkey(&u), &w.venues[0].id, SOL))
        .unwrap();
    assert_eq!(s.mark_price, 145_500_000);
    assert_ok(&report(&mut w, &u, 0, SOL, 10_000 * M, 0, None));
    let s: ch_core::PosSnapshot = w
        .env
        .account(&snapshot_pda(&kp_pubkey(&u), &w.venues[0].id, SOL))
        .unwrap();
    assert_eq!(s.mark_price, 0);
}

#[test]
fn report_bounds_tightening_applies_at_once_loosening_waits() {
    let mut w = World::new(1);
    let admin = w.admin_pk();
    let mut p = w.config().params;
    p.timelock_slots = 100;
    assert_ok(&w.admin_send(update_params_ix(&admin, p)));
    let mut tighter = w.config().params;
    tighter.max_leg_notional /= 2;
    assert_ok(&w.admin_send(update_params_ix(&admin, tighter)));
    assert_eq!(w.config().params.max_leg_notional, tighter.max_leg_notional);
    let mut looser = w.config().params;
    looser.max_notional_growth_bps = 0; // off = loosest
    assert_ok(&w.admin_send(update_params_ix(&admin, looser)));
    assert_eq!(w.config().params.max_notional_growth_bps, 10_000, "queued");
}
