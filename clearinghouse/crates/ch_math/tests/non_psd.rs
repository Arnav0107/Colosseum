//! M2: random symmetric correlation matrices, including non-PSD ones.
//!
//!     cargo test -p ch_math --test non_psd -- --nocapture
//!
//! On-chain `ch_core` bounds each ρ to [−1, 1] and limits each step. Since A4
//! the engine also checks that the correlation sub-matrix over the assets
//! present is positive definite (`ch_math::psd`, integer LDLᵀ); if it is not,
//! the parametric term is replaced by Σ standalone risk, so credit is 0.
//! This suite pins what the engine guarantees for *any* symmetric matrix:
//!
//! 1. A sub-matrix that an f64 classifier calls non-PSD always gives the
//!    fallback and zero credit; the integer check never accepts one, and
//!    `NegativeVariance` no longer occurs.
//! 2. For every accepted input, held margin `ΣIM − credit ≥ ΣIM · min(1, R_p / S)`
//!    and, with the floor `credit ≤ (ΣIM − R_p)⁺` (A2), `ΣIM − credit ≥
//!    min(ΣIM, R_p) ≥ min(ΣIM, worst scenario loss)`, including when `ΣIM < S`
//!    (before A2 this failed; see
//!    `floor_closes_the_gap_when_im_is_below_standalone_risk`).
//! 3. Credit respects every cap.
//! 4. Permuting the legs gives an identical result (including the error).
//!
//! Inputs come from a fixed-seed xorshift generator (deterministic, no
//! dependencies).

use ch_math::portfolio::*;
use ch_math::risk::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
    fn signed(&mut self, mag: i64) -> i64 {
        self.range(0, (2 * mag) as u64) as i64 - mag
    }
}

const N_ASSETS: usize = 6;

struct Sample {
    assets: [AssetRisk; MAX_ASSETS],
    rho: [[i64; N_ASSETS]; N_ASSETS],
    delta: u32,
    beta: u32,
    scenarios: Vec<[i16; MAX_ASSETS]>,
    hedge_break: u16,
    params: CreditParams,
    legs: Vec<PortfolioLeg>,
}

/// Test-only PSD classifier (f64 Cholesky with a small tolerance) of the full
/// matrix. Floats are fine here: this never runs on-chain.
fn is_psd(rho: &[[i64; N_ASSETS]; N_ASSETS]) -> bool {
    let all: Vec<usize> = (0..N_ASSETS).collect();
    is_psd_sub(rho, &all)
}

/// Smallest f64 LDLᵀ pivot of the sub-matrix over `idx` (−∞ if a pivot is
/// non-positive before the end). Every pivot is ≥ the smallest eigenvalue.
fn min_pivot_f64(rho: &[[i64; N_ASSETS]; N_ASSETS], idx: &[usize]) -> f64 {
    let n = idx.len();
    let a = |i: usize, j: usize| rho[idx[i]][idx[j]] as f64 / 1e6;
    let mut l = vec![vec![0.0f64; n]; n];
    let mut d = vec![0.0f64; n];
    let mut min = f64::INFINITY;
    for j in 0..n {
        d[j] = a(j, j) - (0..j).map(|k| l[j][k] * l[j][k] * d[k]).sum::<f64>();
        min = min.min(d[j]);
        if d[j] <= 0.0 {
            return f64::NEG_INFINITY;
        }
        for i in j + 1..n {
            l[i][j] = (a(i, j) - (0..j).map(|k| l[i][k] * l[j][k] * d[k]).sum::<f64>()) / d[j];
        }
    }
    min
}

/// f64 PSD classifier (Cholesky, tolerance 1e-9) of the sub-matrix over `idx`.
fn is_psd_sub(rho: &[[i64; N_ASSETS]; N_ASSETS], idx: &[usize]) -> bool {
    let n = idx.len();
    let a: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| rho[idx[i]][idx[j]] as f64 / 1e6).collect())
        .collect();
    let mut l = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let sum: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                let d = a[i][i] - sum;
                if d < -1e-9 {
                    return false;
                }
                l[i][j] = d.max(0.0).sqrt();
            } else if l[j][j] > 1e-12 {
                l[i][j] = (a[i][j] - sum) / l[j][j];
            } else if (a[i][j] - sum).abs() > 1e-9 {
                return false;
            }
        }
    }
    true
}

fn sample(r: &mut Rng) -> Sample {
    // One sample in five is adversarial: strongly negative correlations and
    // same-direction legs, the shape that drives a non-PSD form negative.
    let adversarial = r.range(0, 4) == 0;
    let mut assets = [AssetRisk::default(); MAX_ASSETS];
    for a in assets.iter_mut().take(N_ASSETS) {
        let shock = r.range(100, 5_000) as u16;
        *a = AssetRisk {
            shock_bps: shock,
            gap_bps: r.range(shock as u64, (2 * shock as u64).min(10_000)) as u16,
            basis_bps: r.range(10, 500) as u16,
        };
    }
    // Every off-diagonal entry independent in [−1, 1]: most such matrices
    // are not PSD.
    let mut rho = [[1_000_000i64; N_ASSETS]; N_ASSETS];
    let pairs = (0..N_ASSETS).flat_map(|i| (i + 1..N_ASSETS).map(move |j| (i, j)));
    for (i, j) in pairs {
        let v = if adversarial {
            -(r.range(600_000, 1_000_000) as i64)
        } else {
            r.signed(1_000_000)
        };
        rho[i][j] = v;
        rho[j][i] = v;
    }
    let scenarios = (0..r.range(0, 2))
        .map(|_| {
            let mut v = [0i16; MAX_ASSETS];
            for m in v.iter_mut().take(N_ASSETS) {
                *m = r.signed(6_000) as i16;
            }
            v
        })
        .collect();
    let mut legs = Vec::new();
    let n = r.range(1, MAX_LEGS as u64) as usize;
    while legs.len() < n {
        let venue = r.range(0, 4) as u8;
        let asset = r.range(0, N_ASSETS as u64 - 1) as u8;
        if legs
            .iter()
            .any(|l: &PortfolioLeg| (l.venue_index, l.asset) == (venue, asset))
        {
            continue;
        }
        let notional = r.range(1_000_000, 1_000_000_000_000);
        // IM between 1% and 100% of notional, so both IM < S and IM > S occur.
        let im = (notional as u128 * r.range(100, 10_000) as u128 / 10_000).max(1) as u64;
        let mm = r.range(1, im);
        legs.push(PortfolioLeg {
            venue_index: venue,
            asset,
            is_long: adversarial || r.next() & 1 == 0,
            notional,
            initial_margin: im,
            maintenance_margin: mm,
        });
    }
    Sample {
        assets,
        rho,
        delta: if adversarial {
            50_000
        } else {
            r.range(50_000, 300_000) as u32
        },
        beta: r.range(10_000, 200_000) as u32,
        scenarios,
        hedge_break: [0u16, 2_500, 5_000, 7_500][r.range(0, 3) as usize],
        params: CreditParams {
            haircut_bps: r.range(500, 5_000) as u16,
            max_credit_per_user: r.range(0, 2_000_000_000_000),
            max_credit_bps_of_im: r.range(0, 9_000) as u16,
            cap_at_im_minus_mm: true,
        },
        legs,
    }
}

struct Outcome {
    result: Result<PortfolioCredit, PortfolioError>,
    worst_loss: u64,
}

fn run(s: &Sample, legs: &[PortfolioLeg]) -> Outcome {
    let rho = |a: u8, b: u8| s.rho[a as usize][b as usize];
    let m = HedgeBreakModel {
        inner: StressedRiskModel {
            assets: &s.assets,
            rho: &rho,
            corr_stress_ppm: s.delta,
            basis_decorr_ppm: s.beta,
            scenarios: &s.scenarios,
        },
        hedge_break_bps: s.hedge_break,
    };
    let result = compute_portfolio_credit(legs, &m, &s.params);
    // Scenario losses do not depend on ρ, so they exist even when the
    // parametric term fails.
    let worst_loss = m.scenario_risk(legs).unwrap_or(0);
    Outcome { result, worst_loss }
}

/// Credits keyed by (venue, asset) so permutations compare equal.
fn keyed(legs: &[PortfolioLeg], c: &PortfolioCredit) -> Vec<((u8, u8), u64)> {
    let mut v: Vec<_> = legs
        .iter()
        .zip(c.leg_credit.iter())
        .map(|(l, &x)| ((l.venue_index, l.asset), x))
        .collect();
    v.sort();
    v
}

#[test]
fn random_symmetric_matrices_fail_closed_respect_caps_and_ignore_leg_order() {
    let mut r = Rng(0x2545_F491_4F6C_DD1D);
    let (mut accepted, mut negative, mut im_below_s, mut credited, mut rp_above_s) =
        (0u32, 0u32, 0u32, 0u32, 0u32);
    let (mut non_psd, mut non_psd_accepted, mut non_psd_credited) = (0u32, 0u32, 0u32);
    let (mut sub_non_psd, mut fallback) = (0u32, 0u32);
    for _ in 0..20_000 {
        let s = sample(&mut r);
        let out = run(&s, &s.legs);
        if !is_psd(&s.rho) {
            non_psd += 1;
            if let Ok(c) = &out.result {
                non_psd_accepted += 1;
                if c.total() > 0 {
                    non_psd_credited += 1;
                }
            }
        }
        // (1) A4, on the sub-matrix the engine actually uses.
        let mut present: Vec<usize> = s.legs.iter().map(|l| l.asset as usize).collect();
        present.sort_unstable();
        present.dedup();
        let sub_psd = is_psd_sub(&s.rho, &present);
        if let Ok(c) = &out.result {
            if !sub_psd {
                sub_non_psd += 1;
                assert!(c.psd_fallback, "non-PSD sub-matrix accepted: {:?}", s.rho);
                assert_eq!(c.total(), 0, "non-PSD sub-matrix earned credit");
            }
            if !c.psd_fallback {
                assert!(sub_psd, "integer check accepted an f64-non-PSD sub-matrix");
            } else {
                fallback += 1;
                assert_eq!(c.total(), 0, "fallback must give zero credit");
                assert!(c.portfolio_risk >= c.sum_standalone_risk);
            }
        }

        // (4) Permutation invariance, including the error.
        let mut perm = s.legs.clone();
        let k = r.range(1, perm.len() as u64) as usize;
        let len = perm.len();
        perm.rotate_left(k % len);
        perm.reverse();
        let out_p = run(&s, &perm);
        match (&out.result, &out_p.result) {
            (Ok(a), Ok(b)) => {
                assert_eq!(keyed(&s.legs, a), keyed(&perm, b));
                assert_eq!(
                    (a.portfolio_risk, a.sum_standalone_risk, a.total()),
                    (b.portfolio_risk, b.sum_standalone_risk, b.total())
                );
            }
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("leg order changed success/failure"),
        }

        let c = match out.result {
            // Before A4 a non-PSD matrix could fail here (refresh reverted).
            // With the PSD fallback this must not happen any more; counted and
            // asserted to be 0 below.
            Err(PortfolioError::NegativeVariance) => {
                negative += 1;
                continue;
            }
            Err(e) => panic!("unexpected error for valid inputs: {e:?}"),
            Ok(c) => c,
        };
        accepted += 1;
        if c.total() > 0 {
            credited += 1;
        }
        let sum_im = c.sum_initial_margin as u128;
        let s_risk = c.sum_standalone_risk as u128;
        let total = c.total() as u128;

        // (3) Every cap.
        assert!(c.total() <= s.params.max_credit_per_user);
        assert!(c.total() <= c.total_before_leg_caps);
        if let Some(benefit) =
            (sum_im * s_risk.saturating_sub(c.portfolio_risk as u128)).checked_div(s_risk)
        {
            assert!(total <= benefit, "credit above the hedge benefit");
        }
        for (l, &x) in s.legs.iter().zip(c.leg_credit.iter()) {
            assert!(x <= l.initial_margin - l.maintenance_margin, "IM − MM cap");
            assert!(
                x as u128
                    <= l.initial_margin as u128 * s.params.max_credit_bps_of_im as u128 / 10_000,
                "bps-of-IM cap"
            );
        }
        // Hedge-break bound: credit ≤ ΣIM · (1 − h) · (1 − haircut), whatever ρ is.
        let bound = sum_im * (10_000 - s.hedge_break as u128) / 10_000
            * (10_000 - s.params.haircut_bps as u128)
            / 10_000;
        assert!(total <= bound, "hedge-break bound");

        // (2) Held margin vs risk.
        let held = sum_im - total;
        assert!(c.portfolio_risk as u128 >= out.worst_loss as u128);
        if s_risk > 0 {
            // Held fraction of IM is at least min(1, R_p / S). R_p can exceed
            // S (gaps above shocks, large governance scenarios): then credit
            // is 0 and the whole IM is held.
            let rp = (c.portfolio_risk as u128).min(s_risk);
            assert!(
                held * s_risk >= sum_im * rp,
                "held fraction below min(1, R_p/S)"
            );
            if c.portfolio_risk as u128 > s_risk {
                assert_eq!(total, 0, "R_p > S must give zero credit");
                rp_above_s += 1;
            }
        }
        // Floor (A2): held ≥ min(ΣIM, R_p) ≥ min(ΣIM, worst loss), now for
        // every accepted input, including ΣIM < S.
        let worst = out.worst_loss as u128;
        assert!(
            held >= (c.portfolio_risk as u128).min(sum_im),
            "held margin below min(ΣIM, R_p)"
        );
        assert!(
            held >= worst.min(sum_im),
            "credit pushed held margin below the worst loss"
        );
        if total > 0 {
            assert!(held >= worst, "credited and held below the worst loss");
        }
        if sum_im < s_risk {
            im_below_s += 1;
        }
    }
    println!(
        "samples 20000: accepted {accepted} (with credit {credited}), NegativeVariance {negative}, \
         accepted with ΣIM < S {im_below_s}, R_p > S {rp_above_s}; non-PSD matrices {non_psd}, of which accepted {non_psd_accepted} (credited {non_psd_credited}); \
         non-PSD sub-matrix over the assets present {sub_non_psd}, PSD fallback {fallback}"
    );
    assert_eq!(negative, 0, "A4: NegativeVariance must no longer occur");
    // The generator must actually reach every branch.
    assert!(
        sub_non_psd > 1_000,
        "too few non-PSD sub-matrices: {sub_non_psd}"
    );
    assert!(non_psd_accepted > 1_000, "too few accepted non-PSD samples");
    assert!(credited > 1_000, "too few credited samples: {credited}");
    assert!(im_below_s > 1_000);
}

/// Before the floor (A2), "ΣIM − credit ≥ worst scenario loss" was FALSE when
/// the venues' IM is below the model's standalone risk: this exact book got
/// $4,750 of credit and held $5,250 against a $20,000 worst loss
/// (KNOWN_LIMITATIONS #19). With `credit ≤ (ΣIM − R_p)⁺` it gets 0. The
/// venues' own IM is still below the worst loss (that is their requirement,
/// not something credit can fix), but credit no longer makes it worse.
#[test]
fn floor_closes_the_gap_when_im_is_below_standalone_risk() {
    let mut assets = [AssetRisk::default(); MAX_ASSETS];
    for a in assets.iter_mut().take(2) {
        *a = AssetRisk {
            shock_bps: 2_000,
            gap_bps: 2_000,
            basis_bps: 100,
        };
    }
    let rho = |_: u8, _: u8| 900_000i64;
    let m = StressedRiskModel {
        assets: &assets,
        rho: &rho,
        corr_stress_ppm: 50_000,
        basis_decorr_ppm: 10_000,
        scenarios: &[],
    };
    // $100k long asset 0, $100k short asset 1; venue IM only 5% ($5k each),
    // while the model's 20% shock puts S at $40k.
    let leg = |v: u8, a: u8, long: bool| PortfolioLeg {
        venue_index: v,
        asset: a,
        is_long: long,
        notional: 100_000_000_000,
        initial_margin: 5_000_000_000,
        maintenance_margin: 2_500_000_000,
    };
    let legs = [leg(0, 0, true), leg(1, 1, false)];
    let p = CreditParams {
        haircut_bps: 500,
        max_credit_per_user: u64::MAX,
        max_credit_bps_of_im: 9_000,
        cap_at_im_minus_mm: true,
    };
    let c = compute_portfolio_credit(&legs, &m, &p).unwrap();
    let worst = m.scenario_risk(&legs).unwrap();
    let held = c.sum_initial_margin - c.total();
    println!(
        "ΣIM {} S {} R_p {} worst loss {} credit {} held {}",
        c.sum_initial_margin,
        c.sum_standalone_risk,
        c.portfolio_risk,
        worst,
        c.total(),
        held
    );
    assert!(c.sum_initial_margin < c.sum_standalone_risk);
    assert!(c.portfolio_risk >= c.sum_initial_margin);
    // Without the floor the same book would get $4,750 (ratio formula).
    assert_eq!(
        hedged_credit(
            c.sum_initial_margin,
            c.sum_standalone_risk,
            c.portfolio_risk,
            p.haircut_bps
        ),
        4_750_000_000
    );
    assert_eq!(c.total(), 0, "floor: R_p ≥ ΣIM gives no credit");
    assert_eq!(held, c.sum_initial_margin);
    // The venues' own IM is still below the worst loss; credit does not add to it.
    assert!(c.sum_initial_margin < worst);
}

/// A4 boundary, on near-singular positive-definite matrices (Gram matrices
/// of random vectors plus a small ridge): if the f64 LDLᵀ's smallest pivot is
/// ≥ 2 ppm the integer check must accept; if the integer check accepts, the
/// f64 smallest pivot must be ≥ 0.5 ppm. (Every pivot ≥ λ_min, so this pins
/// "λ_min ≥ ~1e-6 ⇒ accepted" and "accepted ⇒ positive definite".)
#[test]
fn psd_matches_float_classifier_near_singular() {
    let mut r = Rng(0x0A4_5EED_u64);
    let (mut acc, mut rej, mut checked) = (0u32, 0u32, 0u32);
    for _ in 0..5_000 {
        let n = r.range(2, N_ASSETS as u64) as usize;
        let k = r.range(1, n as u64) as usize; // rank of the Gram part
        let v: Vec<Vec<f64>> = (0..n)
            .map(|_| (0..k).map(|_| r.signed(1_000_000) as f64 / 1e6).collect())
            .collect();
        let ridge = [0.0, 1e-7, 5e-7, 1e-6, 2e-6, 1e-5, 1e-3][r.range(0, 6) as usize];
        let mut g = [[0f64; N_ASSETS]; N_ASSETS];
        for i in 0..n {
            for j in 0..n {
                g[i][j] = v[i].iter().zip(&v[j]).map(|(a, b)| a * b).sum::<f64>()
                    + if i == j { ridge } else { 0.0 };
            }
        }
        let mut rho = [[1_000_000i64; N_ASSETS]; N_ASSETS];
        let mut degenerate = false;
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    let den = (g[i][i] * g[j][j]).sqrt();
                    if den < 1e-12 {
                        degenerate = true;
                    } else {
                        rho[i][j] = ((g[i][j] / den) * 1e6).round() as i64;
                    }
                }
            }
        }
        if degenerate {
            continue;
        }
        // g is symmetric, so rho is too (same expression for (i, j) and (j, i)).
        checked += 1;
        let idx: Vec<usize> = (0..n).collect();
        let assets: Vec<u8> = (0..n as u8).collect();
        let m = rho;
        let ok =
            ch_math::psd::corr_is_positive_definite(&assets, &|a, b| m[a as usize][b as usize]);
        let pivot = min_pivot_f64(&rho, &idx);
        if pivot >= 2e-6 {
            assert!(ok, "λ_min ≥ 2 ppm but rejected: {rho:?}");
        }
        if ok {
            acc += 1;
            assert!(
                pivot >= 0.5e-6,
                "accepted with f64 min pivot {pivot}: {rho:?}"
            );
        } else {
            rej += 1;
        }
    }
    println!("near-singular PD set: {checked} matrices, accepted {acc}, rejected {rej}");
    assert!(acc > 500 && rej > 500, "generator must reach both sides");
}
