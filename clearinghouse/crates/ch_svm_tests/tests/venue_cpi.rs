//! F10: venue authentication in the production build (`venue-cpi`).
//! A snapshot is accepted only via CPI from the registered venue program,
//! signed by that program's `[b"clearinghouse"]` PDA.
//!
//! Runs target/deploy-cpi/ch_core.so (built with
//! `--features mock-oracle,venue-cpi`) plus the mock_perps_a/b programs.

use anchor_lang::prelude::Pubkey;
use anchor_lang::Discriminator;
use ch_core::{
    accounts as acc, instruction as ins, status, AssetRiskArgs, ClearinghouseError as E,
};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;

const IM: u64 = 10_000_000_000;

fn signer_pda(program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"clearinghouse"], program).0
}

struct Fx {
    env: Env,
    admin: Kp,
    user: Kp,
    a: [u8; 32],
    b: [u8; 32],
}

fn admin_send(env: &mut Env, admin: &Kp, i: Instruction) -> TxResult {
    env.send(&[i], &[admin])
}

fn setup() -> Fx {
    let mut env = Env::new_cpi();
    let admin = env.funded(100);
    let ak = kp_pubkey(&admin);
    assert_ok(&admin_send(
        &mut env,
        &admin,
        initialize_ix(&ak, default_params()),
    ));
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
        assert_ok(&admin_send(&mut env, &admin, i));
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
        assert_ok(&admin_send(&mut env, &admin, i));
    }
    let ts = env.clock().unix_timestamp;
    for (asset, price) in [(SOL, 150_000_000u64), (ETH, 3_000_000_000)] {
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
        assert_ok(&admin_send(&mut env, &admin, i));
    }
    let a = venue_id("cpi-venue-a");
    let b = venue_id("cpi-venue-b");
    let pa = mock_perps_a::ID;
    let pb = mock_perps_b_id();
    let i = register_venue_program_ix(&ak, a, pa, signer_pda(&pa), 0);
    assert_ok(&admin_send(&mut env, &admin, i));
    let i = register_venue_program_ix(&ak, b, pb, signer_pda(&pb), 1);
    assert_ok(&admin_send(&mut env, &admin, i));

    // A1: both venue programs allowlist each other, by CPI.
    assert_ok(&env.send(&[accept_ix(pa, a, 0b10)], &[]));
    assert_ok(&env.send(&[accept_ix(pb, b, 0b01)], &[]));

    let user = env.funded(10);
    let i = World::consent_ix(&kp_pubkey(&user), true, u64::MAX);
    assert_ok(&env.send(&[i], &[&user]));
    Fx {
        env,
        admin,
        user,
        a,
        b,
    }
}

/// A1: venue program `program` sets its allowlist by CPI (its PDA signs).
fn accept_ix(program: Pubkey, venue_id: [u8; 32], mask: u64) -> Instruction {
    ix_for(
        program,
        mock_perps_a::accounts::SetAcceptance {
            config: config_pda(),
            venue_registration: venue_pda(&venue_id),
            venue_signer: signer_pda(&program),
            ch_core_program: ch_core::ID,
        },
        mock_perps_a::instruction::SetAcceptance { accepts_mask: mask },
    )
}

#[allow(clippy::too_many_arguments)]
fn report_ix(
    program: Pubkey,
    venue_id: [u8; 32],
    user: &Pubkey,
    asset: u8,
    is_long: bool,
    im: u64,
    payer: &Pubkey,
) -> Instruction {
    ix_for(
        program,
        mock_perps_a::accounts::ReportPosition {
            config: config_pda(),
            venue_registration: venue_pda(&venue_id),
            user_consent: consent_pda(user),
            portfolio: portfolio_pda(user),
            pos_snapshot: snapshot_pda(user, &venue_id, asset),
            user: *user,
            venue_signer: signer_pda(&program),
            payer: *payer,
            system_program: anchor_lang::system_program::ID,
            ch_core_program: ch_core::ID,
            risk_model: risk_model_pda(),
        },
        mock_perps_a::instruction::ReportPosition {
            venue_id,
            asset_id: asset,
            notional: 5 * im,
            is_long,
            initial_margin: im,
            maintenance_margin: im / 2,
            mark_price_micro: 0,
        },
    )
}

#[test]
fn mock_venue_builds_the_same_instruction_as_ch_core() {
    assert_eq!(
        &mock_perps_a::SUBMIT_POSITION_SNAPSHOT_DISCRIMINATOR[..],
        ins::SubmitPositionSnapshot::DISCRIMINATOR
    );
    assert_eq!(
        &mock_perps_a::SET_VENUE_ACCEPTANCE_DISCRIMINATOR[..],
        ins::SetVenueAcceptance::DISCRIMINATOR
    );
}

#[test]
fn registration_requires_the_venue_programs_pda() {
    let mut fx = setup();
    let ak = kp_pubkey(&fx.admin);
    let keypair_authority = fx.env.funded(1);
    let i = register_venue_program_ix(
        &ak,
        venue_id("hot-key-venue"),
        mock_perps_a::ID,
        kp_pubkey(&keypair_authority),
        2,
    );
    assert_ch_err(
        &admin_send(&mut fx.env, &fx.admin, i),
        E::VenueAuthorityNotProgramPda,
    );
    // Another program's PDA is not this program's PDA either.
    let i = register_venue_program_ix(
        &ak,
        venue_id("wrong-pda-venue"),
        mock_perps_a::ID,
        signer_pda(&mock_perps_b_id()),
        2,
    );
    assert_ch_err(
        &admin_send(&mut fx.env, &fx.admin, i),
        E::VenueAuthorityNotProgramPda,
    );
}

#[test]
fn cpi_from_the_registered_venue_program_is_accepted_and_credit_flows() {
    let mut fx = setup();
    let u = kp_pubkey(&fx.user);
    let payer = kp_pubkey(&fx.user);
    let r = fx.env.send(
        &[report_ix(mock_perps_a::ID, fx.a, &u, SOL, true, IM, &payer)],
        &[&fx.user],
    );
    assert_ok(&r);
    let r = fx.env.send(
        &[report_ix(
            mock_perps_b_id(),
            fx.b,
            &u,
            ETH,
            false,
            IM,
            &payer,
        )],
        &[&fx.user],
    );
    assert_ok(&r);
    let snap: ch_core::PosSnapshot = fx.env.account(&snapshot_pda(&u, &fx.a, SOL)).unwrap();
    assert_eq!((snap.version, snap.notional), (1, 5 * IM));

    // Refresh (permissionless) through the normal path.
    let p: ch_core::Portfolio = fx.env.account(&portfolio_pda(&u)).unwrap();
    assert_eq!(p.num_registered, 2);
    let mut metas = vec![snapshot_pda(&u, &fx.a, SOL), snapshot_pda(&u, &fx.b, ETH)];
    metas.extend([mock_price_pda(SOL), mock_price_pda(ETH)]);
    let mut refresh = ix(
        acc::RefreshPortfolio {
            config: config_pda(),
            risk_model: risk_model_pda(),
            user_consent: consent_pda(&u),
            portfolio: portfolio_pda(&u),
        },
        ins::RefreshPortfolio {},
    );
    refresh.accounts.extend(
        metas.iter().map(|k| {
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(*k, false)
        }),
    );
    assert_ok(&fx.env.send(&[refresh], &[]));
    let p: ch_core::Portfolio = fx.env.account(&portfolio_pda(&u)).unwrap();
    assert_eq!(p.status, status::ACTIVE);
    assert_eq!(p.total_credit, 7_111_111_110);

    // A later CPI report invalidates in the same instruction.
    let r = fx.env.send(
        &[report_ix(
            mock_perps_b_id(),
            fx.b,
            &u,
            ETH,
            false,
            IM / 2,
            &payer,
        )],
        &[&fx.user],
    );
    assert_ok(&r);
    let p: ch_core::Portfolio = fx.env.account(&portfolio_pda(&u)).unwrap();
    assert_eq!(p.status, status::INVALIDATED);
}

#[test]
fn another_program_cannot_report_for_a_venue_it_is_not_registered_as() {
    let mut fx = setup();
    let u = kp_pubkey(&fx.user);
    // mock_perps_b signs with *its* PDA but targets venue A (registered to mock_perps_a).
    let i = report_ix(mock_perps_b_id(), fx.a, &u, SOL, true, IM, &u);
    assert_ch_err(&fx.env.send(&[i], &[&fx.user]), E::Unauthorized);
    assert!(!fx.env.exists(&snapshot_pda(&u, &fx.a, SOL)));
}

#[test]
fn nobody_can_sign_for_the_venue_pda_outside_the_venue_program() {
    let mut fx = setup();
    let u = kp_pubkey(&fx.user);
    // A direct call naming the PDA as venue authority without its signature.
    let mut i = ix(
        acc::SubmitPositionSnapshot {
            config: config_pda(),
            venue_registration: venue_pda(&fx.a),
            user_consent: consent_pda(&u),
            portfolio: portfolio_pda(&u),
            pos_snapshot: snapshot_pda(&u, &fx.a, SOL),
            user: u,
            venue_authority: signer_pda(&mock_perps_a::ID),
            payer: u,
            system_program: anchor_lang::system_program::ID,
            risk_model: risk_model_pda(),
        },
        ins::SubmitPositionSnapshot {
            venue_id: fx.a,
            asset_id: SOL,
            notional: 5 * IM,
            is_long: true,
            initial_margin: IM,
            maintenance_margin: IM / 2,
            mark_price_micro: 0,
        },
    );
    for m in i.accounts.iter_mut() {
        if m.pubkey == signer_pda(&mock_perps_a::ID) {
            m.is_signer = false; // nobody holds a key for a PDA
        }
    }
    assert_anchor_err(
        &fx.env.send(&[i], &[&fx.user]),
        anchor_lang::error::ErrorCode::AccountNotSigner,
    );
}

/// A1, production auth path: the allowlist is set by CPI from the registered
/// venue program; another program (signing with its own PDA) cannot change
/// venue A's list, and a plain keypair cannot either.
#[test]
fn acceptance_is_set_only_by_the_venue_programs_pda() {
    let mut fx = setup();
    let c: ch_core::GlobalConfig = fx.env.account(&config_pda()).unwrap();
    assert_eq!((c.venue_accepts[0], c.venue_accepts[1]), (0b10, 0b01));
    // mock_perps_b's PDA is not venue A's authority.
    assert_ch_err(
        &fx.env.send(&[accept_ix(mock_perps_b_id(), fx.a, 0)], &[]),
        E::Unauthorized,
    );
    // A keypair signing directly for venue A.
    let stranger = fx.env.funded(1);
    let i = set_acceptance_ix(&fx.a, &kp_pubkey(&stranger), 0);
    assert_ch_err(&fx.env.send(&[i], &[&stranger]), E::Unauthorized);
    // Venue A itself can remove venue B.
    assert_ok(&fx.env.send(&[accept_ix(mock_perps_a::ID, fx.a, 0)], &[]));
    let c: ch_core::GlobalConfig = fx.env.account(&config_pda()).unwrap();
    assert_eq!(c.venue_accepts[0], 0);
}
