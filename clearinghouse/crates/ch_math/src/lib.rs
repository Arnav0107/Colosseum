pub const PRECISION: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leg {
    pub asset: u8,
    pub signed_required_margin: i64, // + long, - short
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathError {
    NegativeVariance,
    Overflow,
}

/// Integer square root floor: largest integer r such that r*r <= n.
/// Uses bitwise digit-by-digit algorithm, guaranteed to never overflow u128
/// and bounded to at most 64 steps.
pub fn isqrt_floor(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut res = 0u128;
    let mut one = 1u128 << 126;
    while one > n {
        one >>= 2;
    }
    let mut op = n;
    while one != 0 {
        let sum = res.saturating_add(one);
        if op >= sum {
            op = op.saturating_sub(sum);
            res = (res >> 1).saturating_add(one);
        } else {
            res >>= 1;
        }
        one >>= 2;
    }
    res
}

/// Integer square root ceiling: smallest integer r such that r*r >= n.
pub fn isqrt_ceil(n: u128) -> u128 {
    let floor = isqrt_floor(n);
    match floor.checked_mul(floor) {
        Some(sq) if sq == n => floor,
        _ => floor.saturating_add(1),
    }
}

/// Computes combined netted portfolio risk for signed legs.
/// = isqrt_ceil( sum_i sum_j r_i * r_j * rho(i,j) / 1_000_000 ), computed in i128.
/// rho(i,i) = 1_000_000 (asset correlation with itself is always 1.0).
/// A negative total variance returns NegativeVariance (fail closed).
/// Rounding goes UP (conservative).
pub fn combined_risk(legs: &[Leg], rho: &dyn Fn(u8, u8) -> i64) -> Result<u64, MathError> {
    if legs.is_empty() {
        return Ok(0);
    }

    let mut total_cov = 0i128;
    for i in 0..legs.len() {
        let r_i = legs[i].signed_required_margin as i128;
        for j in 0..legs.len() {
            let r_j = legs[j].signed_required_margin as i128;
            let r_prod = r_i.checked_mul(r_j).ok_or(MathError::Overflow)?;

            let corr = if legs[i].asset == legs[j].asset {
                1_000_000i128
            } else {
                rho(legs[i].asset, legs[j].asset) as i128
            };

            let term = r_prod.checked_mul(corr).ok_or(MathError::Overflow)?;
            total_cov = total_cov.checked_add(term).ok_or(MathError::Overflow)?;
        }
    }

    if total_cov < 0 {
        return Err(MathError::NegativeVariance);
    }
    if total_cov == 0 {
        return Ok(0);
    }

    let div = total_cov / 1_000_000;
    let rem = total_cov % 1_000_000;
    let variance = if rem > 0 {
        div.checked_add(1).ok_or(MathError::Overflow)?
    } else {
        div
    };

    let variance_u128 = match u128::try_from(variance) {
        Ok(v) => v,
        Err(_) => return Err(MathError::Overflow),
    };

    let risk_u128 = isqrt_ceil(variance_u128);
    match u64::try_from(risk_u128) {
        Ok(r) => Ok(r),
        Err(_) => Err(MathError::Overflow),
    }
}

/// Computes margin credit total:
/// (sum_required - combined) * (10_000 - haircut_bps) / 10_000, rounded DOWN, saturating.
pub fn credit_total(sum_required: u64, combined: u64, haircut_bps: u16) -> u64 {
    let diff = sum_required.saturating_sub(combined);
    if diff == 0 || haircut_bps >= 10_000 {
        return 0;
    }
    let factor = 10_000u64.saturating_sub(haircut_bps as u64);
    let product = (diff as u128).saturating_mul(factor as u128);
    (product / 10_000) as u64
}

/// Pro-rata split across venues:
/// Floor each share, remainder allocated to the last entry.
pub fn split_pro_rata<const N: usize>(total: u64, required: [u64; N]) -> [u64; N] {
    let mut result = [0u64; N];
    if N == 0 || total == 0 {
        return result;
    }
    let mut sum_required = 0u128;
    for &req in &required {
        sum_required = sum_required.saturating_add(req as u128);
    }
    if sum_required == 0 {
        return result;
    }

    let mut allocated = 0u64;
    for i in 0..(N - 1) {
        let share = ((total as u128).saturating_mul(required[i] as u128) / sum_required) as u64;
        result[i] = share;
        allocated = allocated.saturating_add(share);
    }
    result[N - 1] = total.saturating_sub(allocated);
    result
}

/// Maximum credit capacity supported by default fund:
/// fund_balance * 10_000 / min_ratio_bps.
pub fn fund_cap(fund_balance: u64, min_ratio_bps: u16) -> u64 {
    if min_ratio_bps == 0 {
        return 0;
    }
    let num = (fund_balance as u128).saturating_mul(10_000);
    let cap = num / (min_ratio_bps as u128);
    if cap > u64::MAX as u128 {
        u64::MAX
    } else {
        cap as u64
    }
}

/// Liquidation drop tolerance in bps:
/// trigger_bps * collateral / position, rounded down.
pub fn liquidation_drop_bps(collateral: u64, position: u64, trigger_bps: u16) -> u64 {
    if position == 0 {
        return 0;
    }
    let num = (trigger_bps as u128).saturating_mul(collateral as u128);
    let res = num / (position as u128);
    if res > u64::MAX as u128 {
        u64::MAX
    } else {
        res as u64
    }
}

/// Price move in bps:
/// |new - old| * 10_000 / old, rounded UP; old == 0 returns u64::MAX.
pub fn price_move_bps(old_price: u64, new_price: u64) -> u64 {
    if old_price == 0 {
        return u64::MAX;
    }
    let diff = if new_price >= old_price {
        (new_price - old_price) as u128
    } else {
        (old_price - new_price) as u128
    };
    let num = diff.saturating_mul(10_000);
    let div = num / (old_price as u128);
    let rem = num % (old_price as u128);
    let res = if rem > 0 {
        div.saturating_add(1)
    } else {
        div
    };
    if res > u64::MAX as u128 {
        u64::MAX
    } else {
        res as u64
    }
}
