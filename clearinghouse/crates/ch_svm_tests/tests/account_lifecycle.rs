//! Closing and re-creating a user's consent + portfolio (Stage D: account
//! closure and reinitialization review).

use anchor_lang::prelude::Pubkey;
use ch_core::{accounts as acc, instruction as ins, status, ClearinghouseError as E};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const IM: u64 = 10_000_000_000;

fn close_ix(user: &Pubkey, signer: &Pubkey) -> Instruction {
    ix(
        acc::CloseUserAccounts {
            config: config_pda(),
            user_consent: consent_pda(user),
            portfolio: portfolio_pda(user),
            user: *signer,
        },
        ins::CloseUserAccounts {},
    )
}

/// Two venues, a user with long SOL / short ETH and live, counted credit.
fn credited() -> (World, Kp) {
    let mut w = World::new(2);
    let u = w.user();
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    assert!(w.config().global_outstanding_credit > 0);
    (w, u)
}

/// Revoke consent for every venue, then the user closes both snapshots.
fn close_snapshots(w: &mut World, u: &Kp) {
    let up = kp_pubkey(u);
    assert_ok(&w.set_consent(u, true, 0));
    for (venue, asset) in [(0, SOL), (1, ETH)] {
        let i = w.close_ix(&up, venue, asset, &up);
        assert_ok(&w.env.send(&[i], &[u]));
    }
}

#[test]
fn close_requires_every_snapshot_closed_first() {
    let (mut w, u) = credited();
    let up = kp_pubkey(&u);
    assert_ch_err(
        &w.env.send(&[close_ix(&up, &up)], &[&u]),
        E::SnapshotsStillOpen,
    );
    assert!(w.env.exists(&portfolio_pda(&up)));
}

#[test]
fn close_releases_outstanding_credit_and_refunds_rent() {
    let (mut w, u) = credited();
    let up = kp_pubkey(&u);
    close_snapshots(&mut w, &u);
    // Closing snapshots invalidates but does not by itself release counters.
    assert_eq!(w.portfolio(&u).status, status::INVALIDATED);

    let rent = |w: &World, k: &Pubkey| {
        w.env
            .svm
            .get_account(&k.to_bytes().into())
            .map(|a| a.lamports)
            .unwrap_or(0)
    };
    let before = rent(&w, &up);
    let held = rent(&w, &consent_pda(&up)) + rent(&w, &portfolio_pda(&up));
    assert!(held > 0);

    assert_ok(&w.env.send(&[close_ix(&up, &up)], &[&u]));

    assert!(!w.env.exists(&consent_pda(&up)));
    assert!(!w.env.exists(&portfolio_pda(&up)));
    // The transaction fee is paid by the harness payer, not the user.
    assert_eq!(rent(&w, &up), before + held);
    let c = w.config();
    assert_eq!(c.global_outstanding_credit, 0);
    assert!(c.venue_outstanding.iter().all(|&v| v == 0));
    assert!(c.asset_outstanding.iter().all(|&v| v == 0));
}

#[test]
fn nobody_else_can_close_a_users_accounts() {
    let (mut w, u) = credited();
    let up = kp_pubkey(&u);
    close_snapshots(&mut w, &u);
    let mallory = w.env.funded(1);
    let mp = kp_pubkey(&mallory);
    // The victim's accounts with Mallory signing: the seeds bind the signer.
    assert_anchor_err(
        &w.env.send(&[close_ix(&up, &mp)], &[&mallory]),
        anchor_lang::error::ErrorCode::ConstraintSeeds,
    );
    assert!(w.env.exists(&portfolio_pda(&up)));
}

#[test]
fn recreating_after_close_starts_empty_and_works_normally() {
    let (mut w, u) = credited();
    let up = kp_pubkey(&u);
    close_snapshots(&mut w, &u);
    assert_ok(&w.env.send(&[close_ix(&up, &up)], &[&u]));

    assert_ok(&w.set_consent(&u, true, u64::MAX));
    let p = w.portfolio(&u);
    assert_eq!(p.status, status::EMPTY);
    assert_eq!((p.num_registered, p.num_legs, p.total_credit), (0, 0, 0));
    assert!(!p.counted);
    assert_eq!(w.usable_total(&u), 0);

    // The snapshot PDAs can be created again and the credit is the same as
    // for a user who never closed anything.
    assert_ok(&w.submit(&u, 0, SOL, true, IM));
    assert_ok(&w.submit(&u, 1, ETH, false, IM));
    assert_ok(&w.refresh(&u));
    assert_eq!(w.portfolio(&u).total_credit, 7_111_111_110);
    assert_eq!(w.config().global_outstanding_credit, 7_111_111_110);
}
