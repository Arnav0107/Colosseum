//! C2: the reference venue engine (programs/mock_perps_engine, two
//! instances A and B) consuming Clearinghouse credit on the compiled programs.
//!
//!     cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,venue-cpi --sbf-out-dir target/deploy-cpi
//!     cargo build-sbf --manifest-path programs/mock_perps_engine/Cargo.toml --features instance-a --sbf-out-dir target/deploy-engine-a
//!     cargo build-sbf --manifest-path programs/mock_perps_engine/Cargo.toml --features instance-b --sbf-out-dir target/deploy-engine-b
//!     cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test engine -- --nocapture
//!
//! Instructions are built from Anchor discriminators and Borsh (this crate
//! cannot depend on the engine crate: it builds ch_core with venue-cpi).

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::{AnchorDeserialize, AnchorSerialize};
use ch_core::{accounts as acc, instruction as ins, AssetRiskArgs, ClearinghouseError as E};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const D_INIT: [u8; 8] = [253, 71, 163, 152, 77, 21, 254, 42];
const D_ACCEPT: [u8; 8] = [161, 100, 115, 184, 218, 160, 74, 232];
const D_DEPOSIT: [u8; 8] = [242, 35, 198, 137, 82, 225, 242, 182];
const D_OPEN: [u8; 8] = [135, 128, 47, 77, 15, 152, 240, 49];
const D_REDUCE: [u8; 8] = [96, 202, 33, 80, 24, 197, 33, 77];
const D_LIQUIDATE: [u8; 8] = [223, 179, 226, 125, 48, 46, 39, 74];
/// Engine error codes (Anchor: 6000 + index).
const INSUFFICIENT_MARGIN: u32 = 6000;
const NOT_LIQUIDATABLE: u32 = 6001;

const USD: u64 = 1_000_000;
const SOL_PX: u64 = 150 * USD;
const ETH_PX: u64 = 3_000 * USD;

fn pda(seeds: &[&[u8]], program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}

fn data(d: [u8; 8], args: impl AnchorSerialize) -> Vec<u8> {
    let mut v = d.to_vec();
    args.serialize(&mut v).unwrap();
    v
}

#[derive(AnchorDeserialize)]
struct Position {
    active: bool,
    _asset: u8,
    _is_long: bool,
    _size_e6: u64,
    _entry: u64,
}

#[derive(AnchorDeserialize)]
struct MarginAccount {
    _user: Pubkey,
    collateral: u64,
    _bad_debt: u64,
    positions: [Position; 4],
    _bump: u8,
}

struct Venue {
    program: Pubkey,
    id: [u8; 32],
    index: u8,
}

struct Fx {
    env: Env,
    admin: Kp,
    a: Venue,
    b: Venue,
}

impl Fx {
    fn new() -> Self {
        let mut env = Env::new_engines();
        let admin = env.funded(100);
        let ak = kp_pubkey(&admin);
        assert_ok(&env.send(&[initialize_ix(&ak, default_params())], &[&admin]));
        let mut fx = Self {
            env,
            admin,
            a: Venue {
                program: ENGINE_A_ID,
                id: venue_id("engine-a"),
                index: 0,
            },
            b: Venue {
                program: ENGINE_B_ID,
                id: venue_id("engine-b"),
                index: 1,
            },
        };
        for (asset, shock) in [(SOL, 1_000u16), (ETH, 800)] {
            let i = ix(
                risk_governance(&ak),
                ins::SetAssetRisk {
                    asset_id: asset,
                    params: AssetRiskArgs {
                        shock_bps: shock,
                        gap_bps: shock,
                        basis_bps: 100,
                    },
                },
            );
            fx.admin_send(i);
        }
        for rho in [250_000, 500_000, 750_000, 800_000] {
            let i = ix(
                risk_governance(&ak),
                ins::UpdateCorrelations {
                    entries: vec![ch_core::CorrEntry {
                        a: SOL,
                        b: ETH,
                        rho,
                    }],
                },
            );
            fx.admin_send(i);
        }
        fx.set_price(SOL, SOL_PX);
        fx.set_price(ETH, ETH_PX);
        for v in [0usize, 1] {
            let (program, id, index) = {
                let v = fx.venue(v);
                (v.program, v.id, v.index)
            };
            let signer = pda(&[b"clearinghouse"], &program);
            fx.admin_send(register_venue_program_ix(&ak, id, program, signer, index));
            // Engine: IM 20%, MM 10%.
            let i = Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new(pda(&[b"engine"], &program), false),
                    AccountMeta::new(pda(&[b"vault"], &program), false),
                    AccountMeta::new(ak, true),
                    AccountMeta::new_readonly(anchor_lang::system_program::ID, false),
                ],
                data: data(D_INIT, (id, index, 2_000u16, 1_000u16)),
            };
            fx.admin_send(i);
        }
        // A1: A and B allowlist each other (default would be "own reports only").
        assert_ok(&fx.accept(0, 0b10));
        assert_ok(&fx.accept(1, 0b01));
        fx
    }

    fn venue(&self, v: usize) -> &Venue {
        if v == 0 {
            &self.a
        } else {
            &self.b
        }
    }

    fn admin_send(&mut self, i: Instruction) {
        let admin = self.admin.insecure_clone();
        assert_ok(&self.env.send(&[i], &[&admin]));
    }

    fn set_price(&mut self, asset: u8, price: u64) {
        let ak = kp_pubkey(&self.admin);
        let ts = self.env.clock().unix_timestamp;
        let i = ix(
            acc::SetMockPrice {
                config: config_pda(),
                mock_price: mock_price_pda(asset),
                admin: ak,
                system_program: anchor_lang::system_program::ID,
            },
            ins::SetMockPrice {
                asset_id: asset,
                price_micro: price,
                conf_micro: price / 10_000,
                publish_ts: ts,
                ema_price_micro: price,
            },
        );
        self.admin_send(i);
    }

    fn accept(&mut self, v: usize, mask: u64) -> TxResult {
        let (program, id) = (self.venue(v).program, self.venue(v).id);
        let ak = kp_pubkey(&self.admin);
        let i = Instruction {
            program_id: program,
            accounts: vec![
                AccountMeta::new_readonly(pda(&[b"engine"], &program), false),
                AccountMeta::new_readonly(ak, true),
                AccountMeta::new(config_pda(), false),
                AccountMeta::new_readonly(venue_pda(&id), false),
                AccountMeta::new_readonly(pda(&[b"clearinghouse"], &program), false),
                AccountMeta::new_readonly(ch_core::ID, false),
            ],
            data: data(D_ACCEPT, mask),
        };
        let admin = self.admin.insecure_clone();
        self.env.send(&[i], &[&admin])
    }

    /// A consenting user with `collateral` (micro-USD) deposited on each listed venue.
    fn user(&mut self, deposits: &[(usize, u64)]) -> Kp {
        let u = self.env.funded(1_000);
        let up = kp_pubkey(&u);
        assert_ok(
            &self
                .env
                .send(&[World::consent_ix(&up, true, u64::MAX)], &[&u]),
        );
        for &(v, amount) in deposits {
            let program = self.venue(v).program;
            let i = Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new(pda(&[b"account", up.as_ref()], &program), false),
                    AccountMeta::new(pda(&[b"vault"], &program), false),
                    AccountMeta::new(up, true),
                    AccountMeta::new_readonly(anchor_lang::system_program::ID, false),
                ],
                data: data(D_DEPOSIT, amount),
            };
            assert_ok(&self.env.send(&[i], &[&u]));
        }
        u
    }

    fn trade_metas(&self, v: usize, user: &Pubkey, asset: u8) -> Vec<AccountMeta> {
        let ven = self.venue(v);
        vec![
            AccountMeta::new_readonly(pda(&[b"engine"], &ven.program), false),
            AccountMeta::new(pda(&[b"account", user.as_ref()], &ven.program), false),
            AccountMeta::new(*user, true),
            AccountMeta::new_readonly(mock_price_pda(asset), false),
            AccountMeta::new(config_pda(), false),
            AccountMeta::new_readonly(risk_model_pda(), false),
            AccountMeta::new_readonly(venue_pda(&ven.id), false),
            AccountMeta::new_readonly(consent_pda(user), false),
            AccountMeta::new(portfolio_pda(user), false),
            AccountMeta::new(snapshot_pda(user, &ven.id, asset), false),
            AccountMeta::new_readonly(pda(&[b"clearinghouse"], &ven.program), false),
            AccountMeta::new_readonly(ch_core::ID, false),
            AccountMeta::new_readonly(anchor_lang::system_program::ID, false),
        ]
    }

    /// Refresh accounts for the user's legs *after* this trade: every
    /// registered snapshot plus `extra` (the one being opened), then oracles.
    fn refresh_metas(&self, user: &Pubkey, extra: Option<(usize, u8)>) -> Vec<AccountMeta> {
        let p: ch_core::Portfolio = self.env.account(&portfolio_pda(user)).expect("portfolio");
        let mut keys: Vec<(u8, u8)> = p.registered[..p.num_registered as usize]
            .iter()
            .map(|k| (k.venue_index, k.asset_id))
            .collect();
        if let Some((v, a)) = extra {
            let k = (self.venue(v).index, a);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        let id_of = |idx: u8| {
            if idx == self.a.index {
                self.a.id
            } else {
                self.b.id
            }
        };
        let mut metas: Vec<AccountMeta> = keys
            .iter()
            .map(|(v, a)| AccountMeta::new_readonly(snapshot_pda(user, &id_of(*v), *a), false))
            .collect();
        metas.extend(
            keys.iter()
                .map(|(_, a)| AccountMeta::new_readonly(mock_price_pda(*a), false)),
        );
        metas
    }

    /// Open (or add to) a position; `refresh` = recompute credit in the same instruction.
    fn open(
        &mut self,
        user: &Kp,
        v: usize,
        asset: u8,
        long: bool,
        notional: u64,
        refresh: bool,
    ) -> TxResult {
        let up = kp_pubkey(user);
        let mut metas = self.trade_metas(v, &up, asset);
        if refresh {
            metas.extend(self.refresh_metas(&up, Some((v, asset))));
        }
        let i = Instruction {
            program_id: self.venue(v).program,
            accounts: metas,
            data: data(D_OPEN, (asset, long, notional)),
        };
        self.env
            .send(&[compute_unit_limit_ix(1_400_000), i], &[user])
    }

    fn reduce(&mut self, user: &Kp, v: usize, asset: u8, notional: u64) -> TxResult {
        let up = kp_pubkey(user);
        let i = Instruction {
            program_id: self.venue(v).program,
            accounts: self.trade_metas(v, &up, asset),
            data: data(D_REDUCE, (asset, notional)),
        };
        self.env
            .send(&[compute_unit_limit_ix(1_400_000), i], &[user])
    }

    fn liquidate(&mut self, user: &Pubkey, v: usize, asset: u8) -> TxResult {
        let liq = self.env.funded(1);
        let ven = self.venue(v);
        let i = Instruction {
            program_id: ven.program,
            accounts: vec![
                AccountMeta::new_readonly(pda(&[b"engine"], &ven.program), false),
                AccountMeta::new(pda(&[b"account", user.as_ref()], &ven.program), false),
                AccountMeta::new_readonly(kp_pubkey(&liq), true),
                AccountMeta::new_readonly(mock_price_pda(asset), false),
                AccountMeta::new_readonly(venue_pda(&ven.id), false),
                AccountMeta::new_readonly(consent_pda(user), false),
                AccountMeta::new(portfolio_pda(user), false),
                AccountMeta::new(snapshot_pda(user, &ven.id, asset), false),
                AccountMeta::new(*user, false),
                AccountMeta::new_readonly(pda(&[b"clearinghouse"], &ven.program), false),
                AccountMeta::new_readonly(ch_core::ID, false),
                // Prices for the account's positions.
                AccountMeta::new_readonly(mock_price_pda(asset), false),
            ],
            data: data(D_LIQUIDATE, asset),
        };
        self.env
            .send(&[compute_unit_limit_ix(1_400_000), i], &[&liq])
    }

    fn usable(&self, user: &Kp, v: usize, asset: u8) -> u64 {
        let up = kp_pubkey(user);
        let p: ch_core::Portfolio = self.env.account(&portfolio_pda(&up)).expect("portfolio");
        let c: ch_core::GlobalConfig = self.env.account(&config_pda()).expect("config");
        let idx = self.venue(v).index;
        p.usable_credit(
            c.risk_epoch,
            c.accepts_of(idx),
            self.env.clock().slot,
            idx,
            asset,
        )
    }

    fn margin_account(&self, user: &Kp, v: usize) -> MarginAccount {
        let program = self.venue(v).program;
        let key = pda(&[b"account", kp_pubkey(user).as_ref()], &program);
        let acc = self
            .env
            .svm
            .get_account(&key.to_bytes().into())
            .expect("margin account");
        MarginAccount::deserialize(&mut &acc.data[8..]).unwrap()
    }

    /// Largest SOL long (step $500) user can open on venue A in one instruction
    /// with credit refreshed in the same instruction. Uses failed trades only
    /// as probes (a failed transaction changes nothing).
    fn max_open(&mut self, user: &Kp, refresh: bool) -> u64 {
        let mut n = 0;
        let step = 500 * USD;
        loop {
            let res = self.open(user, 0, SOL, true, n + step, refresh);
            if res.is_ok() {
                // Undo: this is a probe. Close the position again.
                assert_ok(&self.reduce(user, 0, SOL, 10 * (n + step)));
                n += step;
            } else {
                assert_eq!(
                    custom_code(&res),
                    Some(INSUFFICIENT_MARGIN),
                    "only margin may stop a probe"
                );
                return n;
            }
            assert!(n < 200_000 * USD, "probe runaway");
        }
    }
}

/// (a) Same collateral on venue A ($8,000): the user who holds the ETH short
/// on venue B can open more SOL size than an identical unhedged user.
#[test]
fn hedged_user_opens_more_size_than_unhedged_with_same_collateral() {
    let mut fx = Fx::new();
    let hedged = fx.user(&[(0, 8_000 * USD), (1, 12_000 * USD)]);
    let plain = fx.user(&[(0, 8_000 * USD)]);
    assert_ok(&fx.open(&hedged, 1, ETH, false, 50_000 * USD, true));

    // $50,000 long SOL needs IM $10,000: the unhedged user cannot.
    let r = fx.open(&plain, 0, SOL, true, 50_000 * USD, true);
    assert_eq!(custom_code(&r), Some(INSUFFICIENT_MARGIN));
    // The hedged user can: credit computed for the resulting book in the
    // same instruction (worked example: $3,555.56 at venue A), so the
    // requirement is max($10,000 − $3,555.56, $5,000) = $6,444.44.
    let cu = assert_ok(&fx.open(&hedged, 0, SOL, true, 50_000 * USD, true));
    println!("C2(a): open_position with in-instruction snapshot + refresh (2 legs): {cu} CU");
    // ≈ $3,555.56 (the worked example); slightly less because the engine
    // reports notional as floored size × price ($49,999.99995).
    let credit = fx.usable(&hedged, 0, SOL);
    assert!(
        credit > 3_555_000_000 && credit <= 3_555_555_555,
        "credit {credit}"
    );
    assert_ok(&fx.reduce(&hedged, 0, SOL, 500_000 * USD));

    let max_plain = fx.max_open(&plain, true);
    let max_hedged = fx.max_open(&hedged, true);
    println!(
        "C2(a): collateral $8,000 on venue A. max SOL long: unhedged ${}, hedged ${} (+${} enabled by credit)",
        max_plain / USD,
        max_hedged / USD,
        (max_hedged - max_plain) / USD
    );
    assert_eq!(max_plain, 40_000 * USD, "IM 20% of $40,000 = $8,000");
    assert!(max_hedged > max_plain, "credit must enable extra size");
}

/// (b) A user with a single leg gets zero credit.
#[test]
fn single_leg_user_gets_zero_credit() {
    let mut fx = Fx::new();
    let u = fx.user(&[(0, 12_000 * USD)]);
    assert_ok(&fx.open(&u, 0, SOL, true, 50_000 * USD, true));
    assert_eq!(fx.usable(&u, 0, SOL), 0);
    let p: ch_core::Portfolio = fx.env.account(&portfolio_pda(&kp_pubkey(&u))).unwrap();
    assert_eq!(p.total_credit, 0);
    // Adding $10,001 more needs IM beyond the $12,000 collateral.
    let r = fx.open(&u, 0, SOL, true, 10_001 * USD, true);
    assert_eq!(custom_code(&r), Some(INSUFFICIENT_MARGIN));
}

/// (c) Closing the hedge on venue B removes the benefit at venue A in the
/// same instruction: the next risk-increasing trade needs full IM.
#[test]
fn closing_the_hedge_removes_the_benefit_in_the_same_instruction() {
    let mut fx = Fx::new();
    let u = fx.user(&[(0, 8_000 * USD), (1, 12_000 * USD)]);
    assert_ok(&fx.open(&u, 1, ETH, false, 50_000 * USD, true));
    assert_ok(&fx.open(&u, 0, SOL, true, 40_000 * USD, true));
    assert!(fx.usable(&u, 0, SOL) > 0);
    assert_ok(&fx.reduce(&u, 1, ETH, 60_000 * USD)); // closes the ETH short
    assert_eq!(fx.usable(&u, 0, SOL), 0, "void in the same instruction");
    // $40,000 SOL already needs IM $8,000 = all collateral: no more room.
    let r = fx.open(&u, 0, SOL, true, 1_000 * USD, true);
    assert_eq!(custom_code(&r), Some(INSUFFICIENT_MARGIN));
}

/// (d) Zero relief whenever the credit is not usable: expired, wrong epoch,
/// paused, venue deactivated, consent withdrawn, not allowlisted.
#[test]
fn no_relief_when_credit_is_not_usable() {
    // Each case: a hedged user with $8,000 on A who could open $50,000 SOL
    // only with credit. Probe a $45,000 open WITHOUT refresh after the event.
    type Case = fn(&mut Fx, &Kp);
    let cases: [(&str, Case); 6] = [
        ("expired", |fx, _| fx.env.advance_slots(3_001)),
        ("wrong epoch", |fx, _| {
            let ak = kp_pubkey(&fx.admin);
            fx.admin_send(bump_epoch_ix(&ak));
        }),
        ("paused (no new credit)", |fx, _| {
            let ak = kp_pubkey(&fx.admin);
            fx.admin_send(pause_ix(&ak));
        }),
        ("venue B deactivated", |fx, _| {
            let ak = kp_pubkey(&fx.admin);
            let id = fx.b.id;
            fx.admin_send(deactivate_venue_ix(&ak, &id));
        }),
        ("consent withdrawn", |fx, u| {
            let up = kp_pubkey(u);
            assert_ok(&fx.env.send(&[World::consent_ix(&up, true, 0b01)], &[u]));
        }),
        ("venue A no longer allowlists B", |fx, _| {
            assert_ok(&fx.accept(0, 0));
        }),
    ];
    for (name, event) in cases {
        let mut fx = Fx::new();
        let u = fx.user(&[(0, 8_000 * USD), (1, 12_000 * USD)]);
        assert_ok(&fx.open(&u, 1, ETH, false, 50_000 * USD, true));
        assert_ok(&fx.open(&u, 0, SOL, true, 10_000 * USD, true));
        assert!(fx.usable(&u, 0, SOL) > 0, "{name}: credit before the event");
        event(&mut fx, &u);
        if name.starts_with("paused") {
            // Pause blocks NEW credit (refresh fails); credit already computed
            // stays usable until it expires (<= credit_ttl_slots). Voiding it
            // at once is bump_risk_epoch's job (OPERATIONS: pause + bump).
            assert!(
                fx.usable(&u, 0, SOL) > 0,
                "paused: existing credit until expiry"
            );
            let r = fx.open(&u, 0, SOL, true, 1_000 * USD, true);
            assert_eq!(
                custom_code(&r),
                Some(u32::from(E::ProgramPaused)),
                "paused: no refresh"
            );
            fx.env.advance_slots(3_001);
        }
        if name == "expired" || name.starts_with("paused") {
            fx.set_price(SOL, SOL_PX);
            fx.set_price(ETH, ETH_PX);
        }
        assert_eq!(fx.usable(&u, 0, SOL), 0, "{name}: no usable credit");
        // Adding $35,000 (total $45,000, IM $9,000 > $8,000) needs credit.
        let r = fx.open(&u, 0, SOL, true, 35_000 * USD, false);
        assert_eq!(custom_code(&r), Some(INSUFFICIENT_MARGIN), "{name}");
        // A refresh in the same instruction restores relief after expiry or
        // an epoch bump (both only void *old* credit); it does not while
        // paused, after deactivation, consent withdrawal or allowlist removal.
        let r = fx.open(&u, 0, SOL, true, 35_000 * USD, true);
        if name == "expired" || name == "wrong epoch" {
            assert_ok(&r);
        } else {
            assert!(r.is_err(), "{name}: refresh must not restore relief");
        }
    }
}

/// (e) The liquidation threshold is identical with and without credit:
/// same position, same collateral on venue A; one user is hedged on B.
#[test]
fn liquidation_threshold_identical_with_and_without_credit() {
    let mut fx = Fx::new();
    let hedged = fx.user(&[(0, 6_000 * USD), (1, 12_000 * USD)]);
    let plain = fx.user(&[(0, 6_000 * USD)]);
    assert_ok(&fx.open(&hedged, 1, ETH, false, 50_000 * USD, true));
    for u in [&hedged, &plain] {
        assert_ok(&fx.open(u, 0, SOL, true, 30_000 * USD, true));
    }
    assert!(fx.usable(&hedged, 0, SOL) > 0 && fx.usable(&plain, 0, SOL) == 0);
    // 200 SOL long, $6,000 collateral: equity = 6,000 − 200·(150 − p) and
    // MM = 10% · 200·p, so equity < MM below p = $133.33.
    fx.set_price(SOL, 136 * USD);
    for u in [&hedged, &plain] {
        let r = fx.liquidate(&kp_pubkey(u), 0, SOL);
        assert_eq!(
            custom_code(&r),
            Some(NOT_LIQUIDATABLE),
            "above the threshold"
        );
    }
    fx.set_price(SOL, 133 * USD);
    for u in [&hedged, &plain] {
        assert_ok(&fx.liquidate(&kp_pubkey(u), 0, SOL));
        assert!(!fx.margin_account(u, 0).positions[0].active);
    }
    assert_eq!(
        fx.margin_account(&hedged, 0).collateral,
        fx.margin_account(&plain, 0).collateral,
        "same outcome: credit played no part"
    );
}

/// (f) A misreporting venue cannot raise another venue's allowance: the mock
/// venue (registered, consented, but not allowlisted by A) reports a fake
/// ETH short for the user; A still requires full IM.
#[test]
fn misreporting_venue_cannot_raise_another_venues_allowance() {
    let mut fx = Fx::new();
    let ak = kp_pubkey(&fx.admin);
    let rogue = mock_perps_b_id();
    let rogue_id = venue_id("rogue");
    let signer = pda(&[b"clearinghouse"], &rogue);
    fx.admin_send(register_venue_program_ix(&ak, rogue_id, rogue, signer, 2));
    let u = fx.user(&[(0, 8_000 * USD)]);
    let up = kp_pubkey(&u);
    // The rogue reports a $50,000 ETH short that does not exist.
    let i = ix_for(
        rogue,
        mock_perps_a::accounts::ReportPosition {
            config: config_pda(),
            venue_registration: venue_pda(&rogue_id),
            user_consent: consent_pda(&up),
            portfolio: portfolio_pda(&up),
            pos_snapshot: snapshot_pda(&up, &rogue_id, ETH),
            user: up,
            venue_signer: signer,
            payer: up,
            system_program: anchor_lang::system_program::ID,
            ch_core_program: ch_core::ID,
            risk_model: risk_model_pda(),
        },
        mock_perps_a::instruction::ReportPosition {
            venue_id: rogue_id,
            asset_id: ETH,
            notional: 50_000 * USD,
            is_long: false,
            initial_margin: 10_000 * USD,
            maintenance_margin: 5_000 * USD,
            mark_price_micro: 0,
        },
    );
    assert_ok(&fx.env.send(&[i], &[&u]));
    // With refresh, the joint book looks hedged, but venue A's view excludes
    // the rogue: the $50,000 long still needs IM $10,000 > $8,000.
    let mut metas = fx.trade_metas(0, &up, SOL);
    let mut refresh = vec![
        AccountMeta::new_readonly(snapshot_pda(&up, &rogue_id, ETH), false),
        AccountMeta::new_readonly(snapshot_pda(&up, &fx.a.id, SOL), false),
    ];
    refresh.push(AccountMeta::new_readonly(mock_price_pda(ETH), false));
    refresh.push(AccountMeta::new_readonly(mock_price_pda(SOL), false));
    metas.extend(refresh);
    let i = Instruction {
        program_id: ENGINE_A_ID,
        accounts: metas,
        data: data(D_OPEN, (SOL, true, 50_000 * USD)),
    };
    let r = fx.env.send(&[compute_unit_limit_ix(1_400_000), i], &[&u]);
    assert_eq!(custom_code(&r), Some(INSUFFICIENT_MARGIN));
    // A smaller open succeeds (full IM). After a permissionless refresh the
    // joint computation sees the fake "hedge", but A's view excludes the
    // rogue, so A's usable credit is 0 (the rogue accepts only itself, so
    // its own leg gets 0 too).
    assert_ok(&fx.open(&u, 0, SOL, true, 40_000 * USD, false));
    let mut r = ix(
        acc::RefreshPortfolio {
            config: config_pda(),
            risk_model: risk_model_pda(),
            user_consent: consent_pda(&up),
            portfolio: portfolio_pda(&up),
        },
        ins::RefreshPortfolio {},
    );
    for k in [
        snapshot_pda(&up, &rogue_id, ETH),
        snapshot_pda(&up, &fx.a.id, SOL),
        mock_price_pda(ETH),
        mock_price_pda(SOL),
    ] {
        r.accounts.push(AccountMeta::new_readonly(k, false));
    }
    assert_ok(&fx.env.send(&[r], &[]));
    let p: ch_core::Portfolio = fx.env.account(&portfolio_pda(&up)).unwrap();
    assert_eq!(p.status, ch_core::status::ACTIVE);
    assert!(
        p.hedge_benefit_ppm > 0,
        "jointly, the fake leg looks like a hedge"
    );
    assert_eq!(
        p.total_credit, 0,
        "but no venue's view credits it (A1 default)"
    );
    assert_eq!(
        fx.usable(&u, 0, SOL),
        0,
        "venue A gets nothing from the rogue"
    );
}
