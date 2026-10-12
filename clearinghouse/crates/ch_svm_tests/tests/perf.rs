//! Measurements for docs/PERFORMANCE.md, taken on the compiled SBF program.
//!
//!     cargo test --test perf -- --nocapture
//!
//! Prints compute units per instruction, account sizes with rent, and the
//! serialized size of a refresh transaction at MAX_LEGS. It also asserts the
//! budgets that matter: every instruction fits the default 200k-CU
//! per-instruction limit, and a MAX_LEGS refresh fits one 1,232-byte packet.

use anchor_lang::prelude::Pubkey;
use ch_core::{instruction as ins, AssetRiskArgs, CorrEntry};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;
use solana_signer::Signer;

const IM: u64 = 10_000_000_000;
/// Default per-instruction compute limit without a ComputeBudget instruction.
const DEFAULT_CU_LIMIT: u64 = 200_000;
/// Maximum serialized transaction size (one packet).
const PACKET_DATA_SIZE: usize = 1_232;

struct Report {
    cu: Vec<(String, u64)>,
}

impl Report {
    fn cu(&mut self, what: &str, res: &TxResult) -> u64 {
        let cu = assert_ok(res);
        assert!(
            cu <= DEFAULT_CU_LIMIT,
            "{what}: {cu} CU exceeds the default limit"
        );
        self.cu.push((what.to_string(), cu));
        cu
    }
}

fn compact_len(n: usize) -> usize {
    match n {
        0..=0x7f => 1,
        0x80..=0x3fff => 2,
        _ => 3,
    }
}

#[test]
fn measure_compute_sizes_and_transaction_limits() {
    let mut r = Report { cu: Vec::new() };

    // ---- governance / setup (fresh environment)
    let mut env = Env::new();
    let admin = env.funded(100);
    let ak = kp_pubkey(&admin);
    let res = env.send(&[initialize_ix(&ak, default_params())], &[&admin]);
    r.cu("initialize", &res);
    let auth = env.funded(1);
    let res = env.send(
        &[register_venue_ix(
            &ak,
            None,
            venue_id("perf-venue"),
            kp_pubkey(&auth),
            0,
        )],
        &[&admin],
    );
    r.cu("register_venue", &res);
    let i = ix(
        risk_governance(&ak),
        ins::SetAssetRisk {
            asset_id: SOL,
            params: AssetRiskArgs {
                shock_bps: 1_000,
                gap_bps: 1_000,
                basis_bps: 100,
            },
        },
    );
    r.cu("set_asset_risk", &env.send(&[i], &[&admin]));
    let one = ix(
        risk_governance(&ak),
        ins::UpdateCorrelations {
            entries: vec![CorrEntry {
                a: 0,
                b: 1,
                rho: 200_000,
            }],
        },
    );
    r.cu(
        "update_correlations (1 entry)",
        &env.send(&[one], &[&admin]),
    );
    // The largest batch that fits one transaction.
    let mut batch = Vec::new();
    'outer: for a in 0..32u8 {
        for b in (a + 1)..32u8 {
            batch.push(CorrEntry { a, b, rho: 100_000 });
            if batch.len() == 100 {
                break 'outer;
            }
        }
    }
    let big = ix(
        risk_governance(&ak),
        ins::UpdateCorrelations { entries: batch },
    );
    r.cu(
        "update_correlations (100 entries)",
        &env.send(&[big], &[&admin]),
    );
    let i = ix(
        risk_governance(&ak),
        ins::SetScenario {
            index: 0,
            kind: 2,
            moves: (0..32u8)
                .map(|asset| ch_core::AssetMove {
                    asset,
                    move_bps: -3_000,
                })
                .collect(),
        },
    );
    r.cu("set_scenario (32 moves)", &env.send(&[i], &[&admin]));

    // ---- user flow at MAX_LEGS: 4 venues x {SOL, ETH}, mixed directions
    let mut w = World::new(4);
    let u = w.env.funded(10);
    let up = kp_pubkey(&u);
    let res = w.env.send(&[World::consent_ix(&up, true, u64::MAX)], &[&u]);
    r.cu("update_user_consent (creates consent + portfolio)", &res);
    let res = w.submit(&u, 0, SOL, true, IM);
    r.cu("submit_position_snapshot (create)", &res);
    let res = w.submit(&u, 0, SOL, true, IM);
    r.cu("submit_position_snapshot (update)", &res);
    let res = w.refresh(&u);
    r.cu("refresh_portfolio (1 leg)", &res);

    let mut n = 1;
    for v in 0..4usize {
        for (asset, long) in [(SOL, v % 2 == 0), (ETH, v % 2 == 1)] {
            if v == 0 && asset == SOL {
                continue;
            }
            assert_ok(&w.submit(&u, v, asset, long, IM));
            n += 1;
            if [2, 4, 8].contains(&n) {
                w.refresh_prices();
                let res = w.refresh(&u);
                r.cu(&format!("refresh_portfolio ({n} legs)"), &res);
            }
        }
    }
    assert_eq!(n, ch_math::portfolio::MAX_LEGS);
    assert_eq!(w.portfolio(&u).num_legs as usize, n);

    // Serialized size of the MAX_LEGS refresh, fee payer as the only signer.
    let refresh = w.refresh_ix(&up);
    let n_accounts = refresh.accounts.len();
    let tx = solana_transaction::Transaction::new_signed_with_payer(
        &[refresh],
        Some(&w.env.payer.pubkey()),
        &[&w.env.payer],
        w.env.svm.latest_blockhash(),
    );
    let tx_size =
        compact_len(tx.signatures.len()) + 64 * tx.signatures.len() + tx.message_data().len();
    assert!(
        tx_size <= PACKET_DATA_SIZE,
        "refresh at MAX_LEGS is {tx_size} bytes"
    );

    let res = w.invalidate(&u, false);
    // No reason yet: measure the rejected path's cost only for information.
    assert!(res.is_err());
    w.env.advance_slots(3_001);
    let res = w.invalidate(&u, false);
    r.cu("invalidate_portfolio (expired, 8 legs)", &res);
    w.refresh_prices();
    assert_ok(&w.refresh(&u));
    w.env.advance_slots(5);
    w.set_price(SOL, 170_000_000);
    let res = w.invalidate(&u, true);
    r.cu("invalidate_portfolio (price evidence, 8 oracles)", &res);
    let res = w.env.send(
        &[w.close_ix(&up, 0, SOL, &kp_pubkey(&w.venues[0].authority))],
        &[&w.venues[0].authority.insecure_clone()],
    );
    r.cu("close_position_snapshot", &res);

    // ---- A4 worst case: 8 legs in 8 distinct assets, so the PSD check
    // factorizes a full 8x8 correlation sub-matrix (all pairs 0.3: PD).
    let mut w8 = World::new(8);
    let u8_ = w8.user();
    for a in 3..8u8 {
        let i = w8.set_asset_risk_ix(a, 800, 800, 100);
        assert_ok(&w8.admin_send(i));
        w8.set_price(a, 10_000_000);
    }
    let pairs: Vec<(u8, u8, i32)> = (0..8u8)
        .flat_map(|a| ((a + 1)..8u8).map(move |b| (a, b, 300_000)))
        .collect();
    w8.set_corr(&pairs);
    for v in 0..8usize {
        assert_ok(&w8.submit(&u8_, v, v as u8, v % 2 == 0, IM));
    }
    // Default limit, no ComputeBudget instruction: r.cu asserts <= 200k.
    let res = w8.refresh(&u8_);
    r.cu(
        "refresh_portfolio (8 legs, 8 distinct assets: 8x8 PSD check)",
        &res,
    );
    let p8 = w8.portfolio(&u8_);
    assert_eq!(p8.flags & ch_core::portfolio_flags::PSD_FALLBACK, 0);

    // ---- A1 worst case: 8 legs on 8 venues, every venue accepts all others
    // but one, so there are 8 distinct 7-leg views (8 engine runs + joint).
    // Above the default limit: measured under a ComputeBudget instruction.
    let mut wv = World::new(8);
    let uv = wv.user();
    for v in 0..8usize {
        let skip = 1u64 << ((v + 1) % 8);
        assert_ok(&wv.set_acceptance(v, 0xff & !skip));
        let asset = if v % 2 == 0 { SOL } else { ETH };
        assert_ok(&wv.submit(&uv, v, asset, v % 2 == 0, IM));
    }
    let res = wv.refresh_with_cu_limit(&uv, 1_400_000);
    let cu_views = assert_ok(&res);
    let mut wf = World::new(8); // accept_all: every view is the full set
    let uf = wf.user();
    for v in 0..8usize {
        let asset = if v % 2 == 0 { SOL } else { ETH };
        assert_ok(&wf.submit(&uf, v, asset, v % 2 == 0, IM));
    }
    let cu_full = r.cu(
        "refresh_portfolio (8 legs on 8 venues, all allowlisted: joint only)",
        &wf.refresh(&uf),
    );
    println!(
        "\nrefresh_portfolio (8 legs on 8 venues, 8 distinct 7-leg views, ComputeBudget 1.4M): {cu_views} CU (joint only: {cu_full})"
    );
    assert!(cu_views <= 1_400_000);

    // ---- account sizes and rent
    let sizes: Vec<(&str, Pubkey)> = vec![
        ("GlobalConfig", config_pda()),
        ("RiskModel (zero-copy)", risk_model_pda()),
        ("PendingChange", pending_change_pda()),
        ("VenueRegistration", venue_pda(&w.venues[0].id)),
        ("UserConsent", consent_pda(&up)),
        ("Portfolio", portfolio_pda(&up)),
        ("PosSnapshot", snapshot_pda(&up, &w.venues[1].id, SOL)),
        ("MockPrice (localnet only)", mock_price_pda(SOL)),
    ];
    println!("\n| Account | Bytes | Rent-exempt lamports | SOL |\n| --- | ---: | ---: | ---: |");
    for (name, key) in sizes {
        let acc = w.env.svm.get_account(&key.to_bytes().into()).expect(name);
        let len = acc.data.len();
        let rent = w.env.svm.minimum_balance_for_rent_exemption(len);
        println!("| {name} | {len} | {rent} | {:.6} |", rent as f64 / 1e9);
    }

    println!("\n| Instruction | Compute units |\n| --- | ---: |");
    for (what, cu) in &r.cu {
        println!("| {what} | {cu} |");
    }
    println!(
        "\nrefresh_portfolio at MAX_LEGS = {}: {n_accounts} accounts, {tx_size} bytes serialized (limit {PACKET_DATA_SIZE}), 1 signature",
        ch_math::portfolio::MAX_LEGS
    );
}
