use ch_math::{combined_risk, credit_total, split_pro_rata, Leg};

/// Deterministic Linear Congruential Generator (LCG)
/// Requires zero external dependencies.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn next_range(&mut self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        let range = max - min + 1;
        min + (self.next_u64() % range)
    }

    fn next_i64(&mut self, min: i64, max: i64) -> i64 {
        let u_val = self.next_range(0, (max - min) as u64);
        min + u_val as i64
    }
}

#[test]
fn prop_credit_total_le_sum_required() {
    let mut rng = Lcg::new(0xDEADBEEF_CAFE1001);

    for _ in 0..10_000 {
        let sum_required = rng.next_range(0, 1_000_000_000_000); // Up to 1T micro-USD
        let combined = rng.next_range(0, 2_000_000_000_000);
        let haircut_bps = rng.next_range(0, 15_000) as u16;

        let total = credit_total(sum_required, combined, haircut_bps);

        assert!(
            total <= sum_required,
            "Property violated: credit_total ({}) > sum_required ({}) with combined {} and haircut {}",
            total,
            sum_required,
            combined,
            haircut_bps
        );
    }
}

#[test]
fn prop_split_pro_rata_sums_exactly_to_total() {
    let mut rng = Lcg::new(0xCAFEBABE_FACE2002);

    // Test for N = 2
    for _ in 0..2_500 {
        let total = rng.next_range(0, 1_000_000_000_000);
        let r0 = rng.next_range(1, 100_000_000_000);
        let r1 = rng.next_range(1, 100_000_000_000);

        let split = split_pro_rata(total, [r0, r1]);
        let sum: u64 = split.iter().sum();
        assert_eq!(
            sum, total,
            "Split sum ({}) != total ({}) for [{}, {}]",
            sum, total, r0, r1
        );
    }

    // Test for N = 3
    for _ in 0..2_500 {
        let total = rng.next_range(0, 1_000_000_000_000);
        let r0 = rng.next_range(1, 50_000_000_000);
        let r1 = rng.next_range(1, 50_000_000_000);
        let r2 = rng.next_range(1, 50_000_000_000);

        let split = split_pro_rata(total, [r0, r1, r2]);
        let sum: u64 = split.iter().sum();
        assert_eq!(sum, total, "Split sum != total for N=3");
    }

    // Test for N = 4
    for _ in 0..2_500 {
        let total = rng.next_range(0, 1_000_000_000_000);
        let reqs = [
            rng.next_range(1, 20_000_000_000),
            rng.next_range(1, 20_000_000_000),
            rng.next_range(1, 20_000_000_000),
            rng.next_range(1, 20_000_000_000),
        ];

        let split = split_pro_rata(total, reqs);
        let sum: u64 = split.iter().sum();
        assert_eq!(sum, total, "Split sum != total for N=4");
    }
}

#[test]
fn prop_combined_risk_unchanged_when_legs_swapped() {
    let mut rng = Lcg::new(0x12345678_9ABC3003);

    for _ in 0..5_000 {
        let asset_a = rng.next_range(0, 7) as u8;
        let mut asset_b = rng.next_range(0, 7) as u8;
        if asset_a == asset_b {
            asset_b = (asset_a + 1) % 8;
        }

        let margin_a = rng.next_i64(-50_000_000_000, 50_000_000_000);
        let margin_b = rng.next_i64(-50_000_000_000, 50_000_000_000);

        let rho_val = rng.next_i64(-1_000_000, 1_000_000);
        let rho = |_a: u8, _b: u8| -> i64 { rho_val };

        let leg1 = Leg {
            asset: asset_a,
            signed_required_margin: margin_a,
        };
        let leg2 = Leg {
            asset: asset_b,
            signed_required_margin: margin_b,
        };

        let risk1 = combined_risk(&[leg1, leg2], &rho);
        let risk2 = combined_risk(&[leg2, leg1], &rho);

        assert_eq!(
            risk1, risk2,
            "Property violated: combined risk changed when legs swapped: {:?} vs {:?}",
            risk1, risk2
        );
    }
}

#[test]
fn prop_combined_risk_le_sum_of_abs_margins() {
    let mut rng = Lcg::new(0xFEDCBA98_76544004);

    for _ in 0..5_000 {
        let n_legs = rng.next_range(1, 4) as usize;
        let mut legs = Vec::with_capacity(n_legs);
        let mut sum_abs = 0u64;

        for i in 0..n_legs {
            let margin = rng.next_i64(-20_000_000_000, 20_000_000_000);
            legs.push(Leg {
                asset: i as u8,
                signed_required_margin: margin,
            });
            sum_abs = sum_abs.saturating_add(margin.unsigned_abs());
        }

        // Valid positive semi-definite correlation test: correlation <= 1.0
        let rho_val = rng.next_i64(0, 1_000_000);
        let rho = |_a: u8, _b: u8| -> i64 { rho_val };

        if let Ok(risk) = combined_risk(&legs, &rho) {
            assert!(
                risk <= sum_abs,
                "Property violated: combined_risk ({}) > sum of |r_i| ({})",
                risk,
                sum_abs
            );
        }
    }
}

#[test]
fn prop_opposite_sign_legs_risk_non_increasing_as_rho_rises() {
    let mut rng = Lcg::new(0x98765432_10FE5005);

    for _ in 0..5_000 {
        let m1 = rng.next_range(1, 50_000_000_000) as i64; // Long leg (> 0)
        let m2 = -(rng.next_range(1, 50_000_000_000) as i64); // Short leg (< 0)

        let leg1 = Leg {
            asset: 0,
            signed_required_margin: m1,
        };
        let leg2 = Leg {
            asset: 1,
            signed_required_margin: m2,
        };

        // Pick two correlations rho_low < rho_high
        let rho_low = rng.next_i64(-1_000_000, 999_999);
        let rho_high = rng.next_i64(rho_low, 1_000_000);

        let rho_fn_low = |_a: u8, _b: u8| -> i64 { rho_low };
        let rho_fn_high = |_a: u8, _b: u8| -> i64 { rho_high };

        let risk_low = combined_risk(&[leg1, leg2], &rho_fn_low)
            .expect("Valid combined risk for opposite legs with rho_low");
        let risk_high = combined_risk(&[leg1, leg2], &rho_fn_high)
            .expect("Valid combined risk for opposite legs with rho_high");

        assert!(
            risk_high <= risk_low,
            "Property violated: for opposite-sign legs, risk increased as rho rose: rho_low={}, risk_low={}, rho_high={}, risk_high={}",
            rho_low,
            risk_low,
            rho_high,
            risk_high
        );
    }
}
