//! The production oracle path (`pyth`): `PriceUpdateV2` accounts owned by the
//! Pyth receiver, matched to a governed feed id per asset.
//!
//! Runs target/deploy-pyth/ch_core.so (built with
//! `--features pyth,dev-venue-signer`). Price accounts are injected into
//! LiteSVM from the real mainnet SOL/USD account
//! (programs/ch_core/tests/fixtures), with the feed id, price and publish time
//! rewritten per test. Production bounds apply (timelock >= 216,000 slots), so
//! every credit-increasing change goes through propose -> wait -> apply.

use anchor_lang::prelude::Pubkey;
use ch_core::{
    accounts as acc, instruction as ins, status, ClearinghouseError as E, RiskChange,
    PYTH_RECEIVER_ID,
};
use ch_math::portfolio::PortfolioLeg;
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const FIXTURE: &[u8] =
    include_bytes!("../../../programs/ch_core/tests/fixtures/pyth_sol_usd_price_update_v2.bin");
const TIMELOCK: u64 = 216_000;
const IM: u64 = 10_000_000_000;

fn feed(asset: u8) -> [u8; 32] {
    [0xF0 | asset; 32]
}

/// The fixture with a different feed id, price (expo -8), conf and publish time.
fn price_update(feed_id: [u8; 32], price_usd_e8: i64, conf_e8: u64, publish_ts: i64) -> Vec<u8> {
    let mut d = FIXTURE.to_vec();
    d[41..73].copy_from_slice(&feed_id);
    d[73..81].copy_from_slice(&price_usd_e8.to_le_bytes());
    d[81..89].copy_from_slice(&conf_e8.to_le_bytes());
    d[93..101].copy_from_slice(&publish_ts.to_le_bytes());
    // B3: EMA price/conf (message offsets 68 and 76) = the price, so the
    // regime throttle sees no move unless a test sets one.
    d[109..117].copy_from_slice(&price_usd_e8.to_le_bytes());
    d[117..125].copy_from_slice(&conf_e8.to_le_bytes());
    d
}

fn params() -> ch_core::ConfigParams {
    ch_core::ConfigParams {
        timelock_slots: TIMELOCK,
        risk_model_max_age_slots: 2_000_000,
        ..default_params()
    }
}

struct Fx {
    env: Env,
    admin: Kp,
    venue_auth: [Kp; 2],
    venues: [[u8; 32]; 2],
    user: Kp,
}

impl Fx {
    fn admin_pk(&self) -> Pubkey {
        kp_pubkey(&self.admin)
    }

    fn send_admin(&mut self, i: Instruction) -> TxResult {
        let a = self.admin.insecure_clone();
        self.env.send(&[i], &[&a])
    }

    fn pending(&self) -> RiskChange {
        let p: ch_core::PendingChange = self.env.account(&pending_change_pda()).unwrap();
        p.change
    }

    /// Propose; if it queued, wait out the timelock and apply it.
    fn govern(&mut self, i: Instruction) {
        assert_ok(&self.send_admin(i));
        if self.pending() != RiskChange::None {
            self.env.advance_slots(TIMELOCK);
            let ak = self.admin_pk();
            assert_ok(&self.send_admin(apply_pending_ix(&ak)));
            assert_eq!(self.pending(), RiskChange::None);
        }
    }

    fn set_feed_ix(&self, asset: u8, feed_id: [u8; 32]) -> Instruction {
        ix(
            risk_governance(&self.admin_pk()),
            ins::SetOracleFeed {
                asset_id: asset,
                feed_id,
            },
        )
    }

    fn oracle(asset: u8) -> Pubkey {
        Pubkey::find_program_address(&[b"test-pyth", &[asset]], &PYTH_RECEIVER_ID).0
    }

    /// Post a genuine-looking Full update for `asset` at `publish_ts`.
    fn post(&mut self, asset: u8, feed_id: [u8; 32], price_e8: i64, publish_ts: i64) {
        let data = price_update(feed_id, price_e8, price_e8 as u64 / 10_000, publish_ts);
        self.env
            .set_raw_account(&Self::oracle(asset), &PYTH_RECEIVER_ID, data);
    }

    fn now(&self) -> i64 {
        self.env.clock().unix_timestamp
    }

    fn submit(&mut self, v: usize, asset: u8, is_long: bool, im: u64) -> TxResult {
        let u = kp_pubkey(&self.user);
        let auth = self.venue_auth[v].insecure_clone();
        let i = ix(
            acc::SubmitPositionSnapshot {
                config: config_pda(),
                venue_registration: venue_pda(&self.venues[v]),
                user_consent: consent_pda(&u),
                portfolio: portfolio_pda(&u),
                pos_snapshot: snapshot_pda(&u, &self.venues[v], asset),
                user: u,
                venue_authority: kp_pubkey(&auth),
                payer: kp_pubkey(&auth),
                system_program: anchor_lang::system_program::ID,
                risk_model: risk_model_pda(),
            },
            ins::SubmitPositionSnapshot {
                venue_id: self.venues[v],
                asset_id: asset,
                notional: 5 * im,
                is_long,
                initial_margin: im,
                maintenance_margin: im / 2,
                mark_price_micro: 0,
            },
        );
        self.env.send(&[i], &[&auth])
    }

    fn refresh_with(&mut self, oracles: [Pubkey; 2]) -> TxResult {
        let u = kp_pubkey(&self.user);
        let mut i = ix(
            acc::RefreshPortfolio {
                config: config_pda(),
                risk_model: risk_model_pda(),
                user_consent: consent_pda(&u),
                portfolio: portfolio_pda(&u),
            },
            ins::RefreshPortfolio {},
        );
        let metas = [
            snapshot_pda(&u, &self.venues[0], SOL),
            snapshot_pda(&u, &self.venues[1], ETH),
            oracles[0],
            oracles[1],
        ];
        i.accounts.extend(metas.iter().map(|k| {
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(*k, false)
        }));
        self.env.send(&[i], &[])
    }

    fn refresh(&mut self) -> TxResult {
        self.refresh_with([Self::oracle(SOL), Self::oracle(ETH)])
    }

    fn invalidate_with(&mut self, oracles: [Pubkey; 2]) -> TxResult {
        let u = kp_pubkey(&self.user);
        let mut i = ix(
            acc::InvalidatePortfolio {
                config: config_pda(),
                risk_model: risk_model_pda(),
                user_consent: consent_pda(&u),
                portfolio: portfolio_pda(&u),
            },
            ins::InvalidatePortfolio {},
        );
        i.accounts.extend(oracles.iter().map(|k| {
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(*k, false)
        }));
        self.env.send(&[i], &[])
    }

    fn portfolio(&self) -> ch_core::Portfolio {
        self.env
            .account(&portfolio_pda(&kp_pubkey(&self.user)))
            .unwrap()
    }
}

/// Initialized with production bounds, SOL/ETH risk set, two active venues,
/// a consenting user with long SOL on venue 0 and short ETH on venue 1.
/// Feeds are configured only if `with_feeds`.
fn setup(with_feeds: bool) -> Fx {
    let mut env = Env::new_pyth();
    let admin = env.funded(100);
    let ak = kp_pubkey(&admin);
    assert_ok(&env.send(&[initialize_ix(&ak, params())], &[&admin]));
    let venue_auth = [env.funded(10), env.funded(10)];
    let venues = [venue_id("pyth-venue-0"), venue_id("pyth-venue-1")];
    let user = env.funded(10);
    let mut fx = Fx {
        env,
        admin,
        venue_auth,
        venues,
        user,
    };
    for (asset, shock, gap, basis) in &DEFAULT_ASSET_RISK[..2] {
        let i = ix(
            risk_governance(&ak),
            ins::SetAssetRisk {
                asset_id: *asset,
                params: ch_core::AssetRiskArgs {
                    shock_bps: *shock,
                    gap_bps: *gap,
                    basis_bps: *basis,
                },
            },
        );
        fx.govern(i);
    }
    if with_feeds {
        for asset in [SOL, ETH] {
            let i = fx.set_feed_ix(asset, feed(asset));
            fx.govern(i);
        }
    }
    for v in 0..2 {
        let authority = kp_pubkey(&fx.venue_auth[v]);
        let i = register_venue_ix(&ak, None, fx.venues[v], authority, v as u8);
        assert_ok(&fx.send_admin(i));
    }
    fx.env.advance_slots(TIMELOCK);
    for v in 0..2 {
        let i = activate_venue_ix(&ak, &fx.venues[v]);
        assert_ok(&fx.send_admin(i));
        // A1: the two venues allowlist each other (signed by the venue key).
        let auth = fx.venue_auth[v].insecure_clone();
        let i = set_acceptance_ix(&fx.venues[v], &kp_pubkey(&auth), 0b11);
        assert_ok(&fx.env.send(&[i], &[&auth]));
    }
    let u = kp_pubkey(&fx.user);
    let i = World::consent_ix(&u, true, u64::MAX);
    let uk = fx.user.insecure_clone();
    assert_ok(&fx.env.send(&[i], &[&uk]));
    assert_ok(&fx.submit(0, SOL, true, IM));
    assert_ok(&fx.submit(1, ETH, false, IM));
    fx
}

fn post_both(fx: &mut Fx, ts: i64) {
    fx.post(SOL, feed(SOL), 15_000_000_000, ts);
    fx.post(ETH, feed(ETH), 300_000_000_000, ts);
}

#[test]
fn genuine_full_updates_produce_the_engine_credit() {
    let mut fx = setup(true);
    let ts = fx.now();
    post_both(&mut fx, ts);
    assert_ok(&fx.refresh());

    let p = fx.portfolio();
    assert_eq!(p.status, status::ACTIVE);
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
            notional: 5 * IM,
            initial_margin: IM,
            maintenance_margin: IM / 2,
        },
    ];
    let want = expected_credit(&legs, 0, &[], &params());
    let want_total: u64 = want.leg_credit[..want.num_legs].iter().sum();
    assert!(want_total > 0);
    assert_eq!(p.total_credit, want_total);
    // Baseline = the Pyth price converted to micro-USD, with its publish time.
    assert_eq!(p.legs[0].baseline_price, 150_000_000);
    assert_eq!(p.legs[1].baseline_price, 3_000_000_000);
    assert_eq!(p.legs[0].baseline_publish_ts, ts);
}

#[test]
fn no_feed_configured_fails_closed() {
    let mut fx = setup(false);
    let ts = fx.now();
    post_both(&mut fx, ts);
    assert_ch_err(&fx.refresh(), E::OracleNotConfigured);
}

#[test]
fn wrong_owner_feed_verification_or_staleness_is_rejected() {
    let mut fx = setup(true);
    let ts = fx.now();
    post_both(&mut fx, ts);

    // Same bytes, not owned by the Pyth receiver.
    let fake = Pubkey::new_unique();
    let data = price_update(feed(SOL), 15_000_000_000, 1_500_000, ts);
    fx.env.set_raw_account(&fake, &Pubkey::new_unique(), data);
    assert_ch_err(
        &fx.refresh_with([fake, Fx::oracle(ETH)]),
        E::InvalidOracleAccount,
    );

    // ETH's genuine update offered for the SOL leg.
    assert_ch_err(
        &fx.refresh_with([Fx::oracle(ETH), Fx::oracle(ETH)]),
        E::InvalidOracleAccount,
    );

    // Partially verified update.
    let mut data = price_update(feed(SOL), 15_000_000_000, 1_500_000, ts);
    data[40] = 0;
    fx.env
        .set_raw_account(&Fx::oracle(SOL), &PYTH_RECEIVER_ID, data);
    assert_ch_err(&fx.refresh(), E::OracleNotFullyVerified);

    // Stale (older than max_price_age_secs = 60).
    fx.post(SOL, feed(SOL), 15_000_000_000, ts - 61);
    assert_ch_err(&fx.refresh(), E::PriceStale);

    // Nothing of the above left credit behind.
    assert_eq!(fx.portfolio().total_credit, 0);
}

#[test]
fn only_oracle_evidence_newer_than_the_baseline_counts() {
    let mut fx = setup(true);
    let t0 = fx.now();
    // An older genuine update with a very different SOL price exists.
    fx.post(SOL, feed(SOL), 10_000_000_000, t0 - 30);
    let old_sol = fx.env.svm.get_account(&Fx::oracle(SOL).to_bytes().into());
    post_both(&mut fx, t0);
    assert_ok(&fx.refresh());

    // Replaying the older update (a 33% "move") is not evidence.
    let old = Pubkey::new_unique();
    fx.env
        .set_raw_account(&old, &PYTH_RECEIVER_ID, old_sol.expect("posted").data);
    assert_ch_err(
        &fx.invalidate_with([old, Fx::oracle(ETH)]),
        E::InvalidationNotJustified,
    );
    // Neither is the very update the credit was computed on.
    assert_ch_err(
        &fx.invalidate_with([Fx::oracle(SOL), Fx::oracle(ETH)]),
        E::InvalidationNotJustified,
    );
    assert_eq!(fx.portfolio().status, status::ACTIVE);

    // A newer update within the move limit is not a reason.
    fx.env.advance_slots(25);
    let t1 = fx.now();
    fx.post(SOL, feed(SOL), 15_300_000_000, t1);
    fx.post(ETH, feed(ETH), 300_000_000_000, t1);
    assert_ch_err(
        &fx.invalidate_with([Fx::oracle(SOL), Fx::oracle(ETH)]),
        E::InvalidationNotJustified,
    );

    // A newer update beyond max_move_bps (3% by default) invalidates, permissionlessly.
    fx.post(SOL, feed(SOL), 14_000_000_000, t1 + 1);
    assert_ok(&fx.invalidate_with([Fx::oracle(SOL), Fx::oracle(ETH)]));
    let p = fx.portfolio();
    assert_eq!(p.status, status::INVALIDATED);
    assert_eq!(p.total_credit, 0);
}

#[test]
fn setting_a_feed_waits_for_the_timelock_and_clearing_it_is_immediate() {
    let mut fx = setup(true);
    let ak = fx.admin_pk();
    // Switching SOL to another feed queues.
    let i = fx.set_feed_ix(SOL, feed(7));
    assert_ok(&fx.send_admin(i));
    assert_eq!(
        fx.pending(),
        RiskChange::OracleFeed {
            asset_id: SOL,
            feed_id: feed(7)
        }
    );
    assert_ch_err(&fx.send_admin(apply_pending_ix(&ak)), E::TimelockNotElapsed);
    assert_ok(&fx.send_admin(cancel_pending_ix(&ak)));

    // Clearing a feed only removes credit: applies at once and fails closed.
    let ts = fx.now();
    post_both(&mut fx, ts);
    assert_ok(&fx.refresh());
    let i = fx.set_feed_ix(SOL, [0; 32]);
    assert_ok(&fx.send_admin(i));
    assert_eq!(fx.pending(), RiskChange::None);
    // The model change bumped the epoch: existing credit is unusable.
    let c: ch_core::GlobalConfig = fx.env.account(&config_pda()).unwrap();
    let p = fx.portfolio();
    assert_eq!(
        p.usable_credit(c.risk_epoch, c.accepts_of(0), fx.env.clock().slot, 0, SOL),
        0
    );
    assert_ch_err(&fx.refresh(), E::OracleNotConfigured);
}
