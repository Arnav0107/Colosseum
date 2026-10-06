// TODO(owner): Dev A

pub const FIXED_POINT_SCALE: u128 = 1_000_000;

/// Computes combined netted portfolio risk for two positions a and b with correlation rho (-1.0 to 1.0).
/// Uses standard portfolio variance formula: sqrt(a^2 + b^2 + 2*rho*a*b)
pub fn combined_risk(a: u64, b: u64, rho: f64) -> u64 {
    let a_f = a as f64;
    let b_f = b as f64;
    let variance = a_f * a_f + b_f * b_f + 2.0 * rho * a_f * b_f;
    if variance <= 0.0 {
        return 0;
    }
    variance.sqrt().round() as u64
}

/// Integer-only fixed point combined risk calculation (no floats, 1e6 scaling)
/// a and b in micro-units (or base units), rho_scaled in fixed-point 1e6 (e.g. -800_000 for -0.8)
pub fn combined_risk_fixed(a: u64, b: u64, rho_scaled: i64) -> u64 {
    let a128 = a as u128;
    let b128 = b as u128;
    let a2 = a128 * a128;
    let b2 = b128 * b128;
    let sum_sq = a2 + b2;

    let ab = a128 * b128;
    let cross_term = (2 * ab as i128 * rho_scaled as i128) / (FIXED_POINT_SCALE as i128);

    let variance = sum_sq as i128 + cross_term;
    if variance <= 0 {
        return 0;
    }
    integer_sqrt_round(variance as u128) as u64
}

/// Calculates margin credit from unhedged requirements and combined netted risk
pub fn calculate_margin_credit(total_unhedged: u64, netted_risk: u64) -> u64 {
    total_unhedged.saturating_sub(netted_risk)
}

/// Calculates liquidation drop tolerance given collateral relative to baseline collateral
pub fn liquidation_drop_tolerance(collateral: f64, baseline_collateral: f64, baseline_drop: f64) -> f64 {
    if baseline_collateral <= 0.0 {
        return 0.0;
    }
    (collateral / baseline_collateral) * baseline_drop
}

/// Helper: Integer square root using Newton-Raphson
pub fn integer_sqrt(val: u128) -> u128 {
    if val == 0 {
        return 0;
    }
    let mut x0 = val / 2;
    if x0 == 0 {
        return 1;
    }
    let mut x1 = (x0 + val / x0) / 2;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + val / x0) / 2;
    }
    x0
}

/// Helper: Rounded integer square root
pub fn integer_sqrt_round(val: u128) -> u128 {
    let s = integer_sqrt(val);
    if s * s + s < val {
        s + 1
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_sqrt() {
        assert_eq!(integer_sqrt(40_000_000), 6324);
        assert_eq!(integer_sqrt_round(40_000_000), 6325);
    }

    #[test]
    fn test_combined_risk_fixed_matches() {
        let fixed = combined_risk_fixed(10_000, 10_000, -800_000);
        let float = combined_risk(10_000, 10_000, -0.8);
        assert_eq!(fixed, 6_325);
        assert_eq!(float, 6_325);
    }
}
