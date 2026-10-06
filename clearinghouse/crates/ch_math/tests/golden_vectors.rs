// TODO(owner): Dev A
use ch_math::{combined_risk, combined_risk_fixed, liquidation_drop_tolerance};

/// Golden Vector Test Suite:
/// - a=10_000, b=10_000, rho=-0.8 -> combined risk 6_325
/// - credit = 20_000 - 6_325 = 13_675, split 6_837.50 per app
/// - liquidation SOL drop at collateral 10_000 / 7_000 / 3_162.50 -> 16% / 11.2% / 5.06%

#[test]
fn test_golden_combined_risk() {
    let a: u64 = 10_000;
    let b: u64 = 10_000;
    let rho: f64 = -0.8;
    let risk = combined_risk(a, b, rho);
    assert_eq!(risk, 6_325);

    // Also verify integer-only fixed-point version:
    let risk_fixed = combined_risk_fixed(a, b, -800_000);
    assert_eq!(risk_fixed, 6_325);
}

#[test]
fn test_golden_credit_split() {
    let a: u64 = 10_000;
    let b: u64 = 10_000;
    let rho: f64 = -0.8;
    let risk = combined_risk(a, b, rho);
    let total_required = a + b;
    let credit = total_required - risk;
    assert_eq!(credit, 13_675);

    let split_per_app = credit as f64 / 2.0;
    assert_eq!(split_per_app, 6_837.50);
}

#[test]
fn test_golden_liquidation_sol_drop() {
    // Collateral scenarios: 10_000 / 7_000 / 3_162.50 -> 16% / 11.2% / 5.06%
    let base_collateral = 10_000.0f64;
    let base_drop = 0.16f64; // 16%

    let drops = [
        (10_000.0f64, 0.16f64),
        (7_000.0f64, 0.112f64),
        (3_162.50f64, 0.0506f64),
    ];
    for (collateral, expected_drop) in drops {
        let computed = liquidation_drop_tolerance(collateral, base_collateral, base_drop);
        assert!((computed - expected_drop).abs() < 1e-6);
    }
}
