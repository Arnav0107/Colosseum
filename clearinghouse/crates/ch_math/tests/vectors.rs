//! Rust side of the shared Rust/TypeScript parity vectors.
//!
//! `cargo test -p ch_math --test vectors` checks every case's `expect` block.
//! `CH_UPDATE_VECTORS=1 cargo test -p ch_math --test vectors` regenerates the
//! `expect` blocks (inputs are never changed). testbench/src/vectors.test.ts
//! reads the same file and must reproduce every value with the TS port.

use ch_math::portfolio::*;
use ch_math::risk::*;
use serde_json::{json, Value};

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/portfolio.json");

fn u64_of(v: &Value) -> u64 {
    v.as_str()
        .expect("integers are decimal strings")
        .parse()
        .expect("u64")
}

fn rho_table(m: &Value) -> Vec<(u8, u8, i64)> {
    m["rho"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t[0].as_u64().unwrap() as u8,
                t[1].as_u64().unwrap() as u8,
                t[2].as_i64().unwrap(),
            )
        })
        .collect()
}

fn legs_of(case: &Value) -> Vec<PortfolioLeg> {
    case["legs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| PortfolioLeg {
            venue_index: l["venue"].as_u64().unwrap() as u8,
            asset: l["asset"].as_u64().unwrap() as u8,
            is_long: l["long"].as_bool().unwrap(),
            notional: u64_of(&l["notional"]),
            initial_margin: u64_of(&l["im"]),
            maintenance_margin: u64_of(&l["mm"]),
        })
        .collect()
}

fn params_of(case: &Value) -> CreditParams {
    let p = &case["params"];
    CreditParams {
        haircut_bps: p["haircut_bps"].as_u64().unwrap() as u16,
        max_credit_per_user: u64_of(&p["max_credit_per_user"]),
        max_credit_bps_of_im: p["max_credit_bps_of_im"].as_u64().unwrap() as u16,
        cap_at_im_minus_mm: p["cap_at_im_minus_mm"].as_bool().unwrap(),
    }
}

fn err_name(e: PortfolioError) -> &'static str {
    match e {
        PortfolioError::TooManyLegs => "TooManyLegs",
        PortfolioError::DuplicateLeg => "DuplicateLeg",
        PortfolioError::InvalidAsset => "InvalidAsset",
        PortfolioError::UnsupportedAsset => "UnsupportedAsset",
        PortfolioError::ZeroNotional => "ZeroNotional",
        PortfolioError::InvalidMargin => "InvalidMargin",
        PortfolioError::NegativeVariance => "NegativeVariance",
        PortfolioError::Overflow => "Overflow",
    }
}

fn result_json(r: Result<PortfolioCredit, PortfolioError>, extra: Value) -> Value {
    match r {
        Err(e) => json!({ "error": err_name(e) }),
        Ok(c) => {
            let mut v = json!({
                "sum_standalone_risk": c.sum_standalone_risk.to_string(),
                "portfolio_risk": c.portfolio_risk.to_string(),
                "hedge_benefit_ppm": c.hedge_benefit_ppm,
                "sum_initial_margin": c.sum_initial_margin.to_string(),
                "total_before_leg_caps": c.total_before_leg_caps.to_string(),
                "leg_credit": c.leg_credit[..c.num_legs].iter().map(|x| x.to_string()).collect::<Vec<_>>(),
                "total": c.total().to_string(),
            });
            if let (Some(obj), Some(extra)) = (v.as_object_mut(), extra.as_object()) {
                for (k, val) in extra {
                    obj.insert(k.clone(), val.clone());
                }
            }
            v
        }
    }
}

fn evaluate(model: &Value, case: &Value) -> Value {
    let legs = legs_of(case);
    let params = params_of(case);
    let table = rho_table(model);
    let rho = move |a: u8, b: u8| {
        table
            .iter()
            .find(|(x, y, _)| (*x, *y) == (a.min(b), a.max(b)))
            .map(|t| t.2)
            .unwrap_or(0)
    };
    match model["kind"].as_str().unwrap() {
        "margin_as_risk" => {
            let m = MarginAsRiskModel { rho: &rho };
            result_json(compute_portfolio_credit(&legs, &m, &params), json!({}))
        }
        "stressed" => {
            let mut assets = [AssetRisk::default(); MAX_ASSETS];
            for (k, v) in model["assets"].as_object().unwrap() {
                let a: usize = k.parse().unwrap();
                assets[a] = AssetRisk {
                    shock_bps: v[0].as_u64().unwrap() as u16,
                    gap_bps: v[1].as_u64().unwrap() as u16,
                    basis_bps: v[2].as_u64().unwrap() as u16,
                };
            }
            let scenarios: Vec<[i16; MAX_ASSETS]> = model["scenarios"]
                .as_array()
                .unwrap()
                .iter()
                .map(|sc| {
                    let mut v = [0i16; MAX_ASSETS];
                    for mv in sc.as_array().unwrap() {
                        v[mv[0].as_u64().unwrap() as usize] = mv[1].as_i64().unwrap() as i16;
                    }
                    v
                })
                .collect();
            let m = StressedRiskModel {
                assets: &assets,
                rho: &rho,
                corr_stress_ppm: model["corr_stress_ppm"].as_u64().unwrap() as u32,
                basis_decorr_ppm: model["basis_decorr_ppm"].as_u64().unwrap() as u32,
                scenarios: &scenarios,
            };
            let r = compute_portfolio_credit(&legs, &m, &params);
            let extra = if r.is_ok() {
                json!({
                    "parametric_risk": m.parametric_risk(&legs).unwrap().to_string(),
                    "scenario_risk": m.scenario_risk(&legs).unwrap().to_string(),
                })
            } else {
                json!({})
            };
            result_json(r, extra)
        }
        k => panic!("unknown model kind {k}"),
    }
}

#[test]
fn shared_vectors_match_the_rust_engine() {
    let raw = std::fs::read_to_string(PATH).expect("vectors file");
    let mut doc: Value = serde_json::from_str(&raw).expect("valid JSON");
    let models = doc["models"].clone();
    let update = std::env::var("CH_UPDATE_VECTORS").as_deref() == Ok("1");
    let cases = doc["cases"].as_array_mut().unwrap();
    assert!(cases.len() >= 10);
    for case in cases.iter_mut() {
        let model = &models[case["model"].as_str().unwrap()];
        let got = evaluate(model, case);
        if update {
            case["expect"] = got;
        } else {
            assert_eq!(
                case["expect"], got,
                "vector '{}' (regenerate with CH_UPDATE_VECTORS=1 only if the change is intended)",
                case["name"]
            );
        }
    }
    if update {
        std::fs::write(PATH, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
    }
}
