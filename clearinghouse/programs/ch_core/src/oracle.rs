use anchor_lang::prelude::*;
use crate::ClearinghouseError;

#[cfg(all(feature = "mock-oracle", feature = "pyth"))]
compile_error!("enable only one oracle backend");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriceReading {
    pub price_micro: u64,
    pub conf_micro: u64,
    pub publish_ts: i64,
}

/// Validates that a price reading is non-zero, fresh, and within confidence bounds.
pub fn check_price(
    r: &PriceReading,
    now_ts: i64,
    max_age_secs: i64,
    max_conf_bps: u16,
) -> Result<()> {
    // Rejects zero or invalid price
    require!(r.price_micro > 0, ClearinghouseError::InvalidPrice);

    // Rejects stale price reading (or excessive forward drift)
    let age = now_ts.saturating_sub(r.publish_ts);
    let future_drift = r.publish_ts.saturating_sub(now_ts);
    require!(
        age <= max_age_secs && future_drift <= max_age_secs,
        ClearinghouseError::PriceStale
    );

    // Rejects if confidence interval is too wide (conf * 10_000 / price > max_conf_bps)
    let conf_bps = (r.conf_micro as u128)
        .saturating_mul(10_000)
        .checked_div(r.price_micro as u128)
        .ok_or(ClearinghouseError::MathOverflow)?;

    require!(
        conf_bps <= max_conf_bps as u128,
        ClearinghouseError::PriceUncertain
    );

    Ok(())
}

/// Converts Pyth price, confidence, and exponent to micro-USD (scale 1e6).
/// Returns (price_micro, conf_micro).
pub fn pyth_to_micro(price: i64, conf: u64, expo: i32) -> Result<(u64, u64)> {
    require!(price > 0, ClearinghouseError::InvalidPrice);
    let p = price as u64;

    // Target scale is 10^6 (micro-USD)
    // Real value = price * 10^expo. In micro-USD: price * 10^(expo + 6)
    let net_expo = expo
        .checked_add(6)
        .ok_or(ClearinghouseError::MathOverflow)?;

    if net_expo == 0 {
        Ok((p, conf))
    } else if net_expo > 0 {
        let factor = 10u128
            .checked_pow(net_expo as u32)
            .ok_or(ClearinghouseError::MathOverflow)?;
        let p_micro = (p as u128)
            .checked_mul(factor)
            .ok_or(ClearinghouseError::MathOverflow)?;
        let conf_micro = (conf as u128)
            .checked_mul(factor)
            .ok_or(ClearinghouseError::MathOverflow)?;
        Ok((
            u64::try_from(p_micro).map_err(|_| ClearinghouseError::MathOverflow)?,
            u64::try_from(conf_micro).map_err(|_| ClearinghouseError::MathOverflow)?,
        ))
    } else {
        let neg_expo = net_expo
            .checked_neg()
            .ok_or(ClearinghouseError::MathOverflow)?;
        let divisor = 10u128
            .checked_pow(neg_expo as u32)
            .ok_or(ClearinghouseError::MathOverflow)?;
        let p_micro = (p as u128)
            .checked_div(divisor)
            .ok_or(ClearinghouseError::MathOverflow)?;
        let conf_micro = (conf as u128)
            .checked_div(divisor)
            .ok_or(ClearinghouseError::MathOverflow)?;
        Ok((
            u64::try_from(p_micro).map_err(|_| ClearinghouseError::MathOverflow)?,
            u64::try_from(conf_micro).map_err(|_| ClearinghouseError::MathOverflow)?,
        ))
    }
}

// -----------------------------------------------------------------------------
// Backend Implementations
// -----------------------------------------------------------------------------

#[cfg(feature = "mock-oracle")]
#[account]
#[derive(InitSpace)]
pub struct MockPrice {
    pub asset_id: u8,
    pub price_micro: u64,
    pub conf_micro: u64,
    pub publish_ts: i64,
    pub bump: u8,
}

#[cfg(all(feature = "mock-oracle", not(feature = "pyth")))]
pub fn read_price(account: &AccountInfo, expected_asset: u8) -> Result<PriceReading> {
    let (expected_pda, _) = Pubkey::find_program_address(
        &[b"mock_price", &[expected_asset]],
        &crate::ID,
    );
    require_keys_eq!(account.key(), expected_pda, ClearinghouseError::InvalidOracleAccount);

    let mut data: &[u8] = &account.try_borrow_data()?;
    let mock_price = MockPrice::try_deserialize(&mut data)?;
    require_eq!(mock_price.asset_id, expected_asset, ClearinghouseError::InvalidAssetId);

    Ok(PriceReading {
        price_micro: mock_price.price_micro,
        conf_micro: mock_price.conf_micro,
        publish_ts: mock_price.publish_ts,
    })
}

#[cfg(feature = "pyth")]
pub fn read_price(_account: &AccountInfo, _expected_asset: u8) -> Result<PriceReading> {
    // Note: If external Pyth SDK is not linked or not configured for this cluster/environment,
    // fail closed by returning OracleNotConfigured.
    Err(ClearinghouseError::OracleNotConfigured.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_price_fresh() {
        let reading = PriceReading {
            price_micro: 150_000_000, // $150.00
            conf_micro: 75_000,       // $0.075 (5 bps)
            publish_ts: 1_000,
        };
        // max age 60s, max conf 10 bps (100 in bps? 10 bps is 10)
        assert!(check_price(&reading, 1_030, 60, 10).is_ok());
    }

    #[test]
    fn test_check_price_stale() {
        let reading = PriceReading {
            price_micro: 150_000_000,
            conf_micro: 75_000,
            publish_ts: 1_000,
        };
        // now is 1_070, age is 70s > max 60s
        let res = check_price(&reading, 1_070, 60, 100);
        assert_eq!(res.unwrap_err(), ClearinghouseError::PriceStale.into());
    }

    #[test]
    fn test_check_price_wide_confidence() {
        let reading = PriceReading {
            price_micro: 100_000_000,
            conf_micro: 2_000_000, // 2% = 200 bps
            publish_ts: 1_000,
        };
        // max conf allowed is 100 bps (1%)
        let res = check_price(&reading, 1_010, 60, 100);
        assert_eq!(res.unwrap_err(), ClearinghouseError::PriceUncertain.into());
    }

    #[test]
    fn test_check_price_zero() {
        let reading = PriceReading {
            price_micro: 0,
            conf_micro: 0,
            publish_ts: 1_000,
        };
        let res = check_price(&reading, 1_010, 60, 100);
        assert_eq!(res.unwrap_err(), ClearinghouseError::InvalidPrice.into());
    }

    #[test]
    fn test_pyth_to_micro_conversion() {
        // SOL price $150.00, expo -8: price = 15_000_000_000, conf = 10_000_000 ($0.10)
        let (p, c) = pyth_to_micro(15_000_000_000, 10_000_000, -8).unwrap();
        assert_eq!(p, 150_000_000);
        assert_eq!(c, 100_000);

        // BTC price $65,000, expo -6: price = 65_000_000_000, conf = 1_000_000
        let (p2, c2) = pyth_to_micro(65_000_000_000, 1_000_000, -6).unwrap();
        assert_eq!(p2, 65_000_000_000);
        assert_eq!(c2, 1_000_000);

        // Price with positive net expo (e.g. expo = -4: $10.0000 = 100_000)
        let (p3, c3) = pyth_to_micro(100_000, 1_000, -4).unwrap();
        assert_eq!(p3, 10_000_000);
        assert_eq!(c3, 100_000);

        // Zero and negative prices reject
        assert!(pyth_to_micro(0, 0, -8).is_err());
        assert!(pyth_to_micro(-100, 10, -8).is_err());
    }
}
