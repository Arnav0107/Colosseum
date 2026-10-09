use ch_math::{
    combined_risk, credit_total, fund_cap, isqrt_ceil, isqrt_floor, liquidation_drop_bps,
    price_move_bps, split_pro_rata, Leg, MathError,
};

#[test]
fn test_golden_hedged_pair() {
    let legs = [
        Leg {
            asset: 0,
            signed_required_margin: 10_000_000_000, // long SOL
        },
        Leg {
            asset: 1,
            signed_required_margin: -10_000_000_000, // short ETH
        },
    ];

    // rho(0,1) = +800_000 -> combined_risk 6_324_555_321
    let rho_800k = |_a: u8, _b: u8| -> i64 { 800_000 };
    let risk = combined_risk(&legs, &rho_800k).unwrap();
    assert_eq!(risk, 6_324_555_321);

    // credit_total, haircut 0, sum_required 20_000_000_000 -> 13_675_444_679
    let credit = credit_total(20_000_000_000, risk, 0);
    assert_eq!(credit, 13_675_444_679);

    // split_pro_rata(13_675_444_679, [10_000_000_000, 10_000_000_000]) -> [6_837_722_339, 6_837_722_340]
    let split = split_pro_rata(credit, [10_000_000_000, 10_000_000_000]);
    assert_eq!(split, [6_837_722_339, 6_837_722_340]);

    // rho = 0 -> 14_142_135_624
    let rho_0 = |_a: u8, _b: u8| -> i64 { 0 };
    assert_eq!(combined_risk(&legs, &rho_0).unwrap(), 14_142_135_624);

    // rho = +1_000_000 -> 0
    let rho_1m = |_a: u8, _b: u8| -> i64 { 1_000_000 };
    assert_eq!(combined_risk(&legs, &rho_1m).unwrap(), 0);

    // rho = -1_000_000 -> 20_000_000_000
    let rho_neg_1m = |_a: u8, _b: u8| -> i64 { -1_000_000 };
    assert_eq!(combined_risk(&legs, &rho_neg_1m).unwrap(), 20_000_000_000);
}

#[test]
fn test_golden_same_direction_legs() {
    let legs = [
        Leg {
            asset: 0,
            signed_required_margin: 10_000_000_000,
        },
        Leg {
            asset: 1,
            signed_required_margin: 10_000_000_000,
        },
    ];

    let rho_800k = |_a: u8, _b: u8| -> i64 { 800_000 };
    let risk = combined_risk(&legs, &rho_800k).unwrap();
    assert_eq!(risk, 18_973_665_962);

    let credit = credit_total(20_000_000_000, risk, 0);
    assert_eq!(credit, 1_026_334_038);
}

#[test]
fn test_golden_fund_cap() {
    assert_eq!(fund_cap(100_000_000_000, 200), 5_000_000_000_000);
}

#[test]
fn test_golden_liquidation_drop_bps() {
    assert_eq!(
        liquidation_drop_bps(10_000_000_000, 50_000_000_000, 8_000),
        1_600
    );
    assert_eq!(
        liquidation_drop_bps(7_000_000_000, 50_000_000_000, 8_000),
        1_120
    );
    assert_eq!(
        liquidation_drop_bps(3_162_277_661, 50_000_000_000, 8_000),
        505
    );
}

#[test]
fn test_golden_price_move_bps() {
    assert_eq!(price_move_bps(100_000_000, 110_000_000), 1_000);
    assert_eq!(price_move_bps(0, 1), u64::MAX);
    assert_eq!(price_move_bps(100_000_000, 90_000_000), 1_000);
    assert_eq!(price_move_bps(100_000_000, 100_000_000), 0);
}

#[test]
fn test_isqrt_edge_cases() {
    // 0 and 1
    assert_eq!(isqrt_floor(0), 0);
    assert_eq!(isqrt_ceil(0), 0);
    assert_eq!(isqrt_floor(1), 1);
    assert_eq!(isqrt_ceil(1), 1);

    // Non-squares
    assert_eq!(isqrt_floor(2), 1);
    assert_eq!(isqrt_ceil(2), 2);
    assert_eq!(isqrt_floor(3), 1);
    assert_eq!(isqrt_ceil(3), 2);

    // Perfect squares
    assert_eq!(isqrt_floor(4), 2);
    assert_eq!(isqrt_ceil(4), 2);
    assert_eq!(isqrt_floor(1_000_000_000_000), 1_000_000);
    assert_eq!(isqrt_ceil(1_000_000_000_000), 1_000_000);

    // Off-by-one near squares
    assert_eq!(isqrt_floor(999_999_999_999), 999_999);
    assert_eq!(isqrt_ceil(999_999_999_999), 1_000_000);
    assert_eq!(isqrt_floor(1_000_000_000_001), 1_000_000);
    assert_eq!(isqrt_ceil(1_000_000_000_001), 1_000_001);

    // Very large u128
    let max = u128::MAX;
    let expected_floor = 18_446_744_073_709_551_615u128; // 2^64 - 1
    assert_eq!(isqrt_floor(max), expected_floor);
    // Ceil of u128::MAX is 2^64 (18_446_744_073_709_551_616)
    assert_eq!(isqrt_ceil(max), 18_446_744_073_709_551_616u128);
}

#[test]
fn test_negative_variance() {
    // Assets with an invalid/inconsistent correlation matrix leading to negative variance
    let legs = [
        Leg {
            asset: 0,
            signed_required_margin: 10_000_000_000,
        },
        Leg {
            asset: 1,
            signed_required_margin: -10_000_000_000,
        },
    ];
    // If cross term exceeds the sum of diagonal terms (e.g. invalid rho > 1.0)
    let rho_invalid = |_a: u8, _b: u8| -> i64 { 1_500_000 };
    let res = combined_risk(&legs, &rho_invalid);
    assert_eq!(res, Err(MathError::NegativeVariance));
}

#[test]
fn test_overflow() {
    let legs = [
        Leg {
            asset: 0,
            signed_required_margin: i64::MAX,
        },
        Leg {
            asset: 1,
            signed_required_margin: i64::MAX,
        },
    ];
    let rho = |_a: u8, _b: u8| -> i64 { 1_000_000 };
    let res = combined_risk(&legs, &rho);
    assert_eq!(res, Err(MathError::Overflow));
}
