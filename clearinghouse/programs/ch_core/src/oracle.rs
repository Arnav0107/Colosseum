use crate::ClearinghouseError;
use anchor_lang::prelude::*;

// Fail closed at build time: exactly one oracle backend must be chosen
// explicitly. `mock-oracle` is never a default (F9).
#[cfg(all(feature = "mock-oracle", feature = "pyth"))]
compile_error!("ch_core: enable only one oracle backend (`mock-oracle` or `pyth`)");
#[cfg(not(any(feature = "mock-oracle", feature = "pyth")))]
compile_error!(
    "ch_core: no oracle backend selected. Build with `--features mock-oracle`      (localnet/tests only) or `--features pyth`."
);

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
/// Mock backend. `_expected_feed` is unused: the mock account is the PDA
/// `[b"mock_price", asset_id]`, which already pins the asset.
pub fn read_price(
    account: &AccountInfo,
    expected_asset: u8,
    _expected_feed: &[u8; 32],
) -> Result<PriceReading> {
    require_keys_eq!(
        *account.owner,
        crate::ID,
        ClearinghouseError::InvalidOracleAccount
    );

    let mut data: &[u8] = &account.try_borrow_data()?;
    let mock_price = MockPrice::try_deserialize(&mut data)?;
    require_eq!(
        mock_price.asset_id,
        expected_asset,
        ClearinghouseError::InvalidAssetId
    );

    let expected_pda = Pubkey::create_program_address(
        &[b"mock_price", &[expected_asset], &[mock_price.bump]],
        &crate::ID,
    )
    .map_err(|_| ClearinghouseError::InvalidOracleAccount)?;
    require_keys_eq!(
        account.key(),
        expected_pda,
        ClearinghouseError::InvalidOracleAccount
    );

    Ok(PriceReading {
        price_micro: mock_price.price_micro,
        conf_micro: mock_price.conf_micro,
        publish_ts: mock_price.publish_ts,
    })
}

/// Pyth Solana Receiver program (owner of every `PriceUpdateV2` account).
pub const PYTH_RECEIVER_ID: Pubkey = pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");

/// `sha256("account:PriceUpdateV2")[..8]`.
pub const PRICE_UPDATE_V2_DISCRIMINATOR: [u8; 8] = [34, 241, 35, 99, 157, 126, 244, 205];

/// Fields of a `PriceUpdateV2` that the clearinghouse uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PythPrice {
    pub feed_id: [u8; 32],
    pub price: i64,
    pub conf: u64,
    pub exponent: i32,
    pub publish_time: i64,
}

/// Parse a `PriceUpdateV2` account body by hand (the Pyth SDK does not build
/// against Anchor 1.x / Solana 3.x crates). Layout, Borsh:
///
/// ```text
/// [0..8)    discriminator
/// [8..40)   write_authority: Pubkey
/// [40]      verification_level tag: 0 = Partial { num_signatures: u8 }, 1 = Full
/// then      PriceFeedMessage { feed_id [u8;32], price i64, conf u64,
///           exponent i32, publish_time i64, prev_publish_time i64,
///           ema_price i64, ema_conf u64 }, posted_slot u64
/// ```
///
/// Only `Full` verification is accepted: a `Partial` update was checked
/// against fewer Wormhole guardian signatures than the quorum.
pub fn parse_price_update_v2(data: &[u8]) -> Result<PythPrice> {
    let err = || error!(ClearinghouseError::InvalidOracleAccount);
    require!(
        data.get(..8) == Some(&PRICE_UPDATE_V2_DISCRIMINATOR[..]),
        ClearinghouseError::InvalidOracleAccount
    );
    // Full = tag 1 with no payload; Partial (tag 0) is rejected.
    require!(
        data.get(40) == Some(&1),
        ClearinghouseError::OracleNotFullyVerified
    );
    let msg = data.get(41..41 + 84).ok_or_else(err)?;
    let mut feed_id = [0u8; 32];
    feed_id.copy_from_slice(&msg[0..32]);
    let le8 = |at: usize| -> [u8; 8] {
        let mut b = [0u8; 8];
        b.copy_from_slice(&msg[at..at + 8]);
        b
    };
    let mut e = [0u8; 4];
    e.copy_from_slice(&msg[48..52]);
    Ok(PythPrice {
        feed_id,
        price: i64::from_le_bytes(le8(32)),
        conf: u64::from_le_bytes(le8(40)),
        exponent: i32::from_le_bytes(e),
        publish_time: i64::from_le_bytes(le8(52)),
    })
}

/// Validate a parsed update against the governed feed id and convert it.
pub fn pyth_reading(p: &PythPrice, expected_feed: &[u8; 32]) -> Result<PriceReading> {
    // An all-zero feed id means "no feed configured": fail closed.
    require!(
        *expected_feed != [0u8; 32],
        ClearinghouseError::OracleNotConfigured
    );
    require!(
        p.feed_id == *expected_feed,
        ClearinghouseError::InvalidOracleAccount
    );
    let (price_micro, conf_micro) = pyth_to_micro(p.price, p.conf, p.exponent)?;
    Ok(PriceReading {
        price_micro,
        conf_micro,
        publish_ts: p.publish_time,
    })
}

/// Pyth backend. Any account owned by the receiver with the right feed id is
/// accepted (sponsored push feeds or a freshly posted update); the caller
/// bounds staleness with `check_price`, and invalidation additionally requires
/// evidence newer than the stored baseline.
#[cfg(feature = "pyth")]
pub fn read_price(
    account: &AccountInfo,
    _expected_asset: u8,
    expected_feed: &[u8; 32],
) -> Result<PriceReading> {
    require_keys_eq!(
        *account.owner,
        PYTH_RECEIVER_ID,
        ClearinghouseError::InvalidOracleAccount
    );
    let data = account.try_borrow_data()?;
    pyth_reading(&parse_price_update_v2(&data)?, expected_feed)
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

    /// The real mainnet SOL/USD account (see tests/fixtures/README.md).
    const SOL_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/pyth_sol_usd_price_update_v2.bin");
    const SOL_FEED: [u8; 32] = [
        0xef, 0x0d, 0x8b, 0x6f, 0xda, 0x2c, 0xeb, 0xa4, 0x1d, 0xa1, 0x5d, 0x40, 0x95, 0xd1, 0xda,
        0x39, 0x2a, 0x0d, 0x2f, 0x8e, 0xd0, 0xc6, 0xc7, 0xbc, 0x0f, 0x4c, 0xfa, 0xc8, 0xc2, 0x80,
        0xb5, 0x6d,
    ];

    #[test]
    fn parses_the_real_mainnet_price_update() {
        let p = parse_price_update_v2(SOL_FIXTURE).unwrap();
        assert_eq!(p.feed_id, SOL_FEED);
        assert_eq!(
            (p.price, p.conf, p.exponent, p.publish_time),
            (10_960_825_188, 2_468_469, -8, 1_791_729_342)
        );
        let r = pyth_reading(&p, &SOL_FEED).unwrap();
        assert_eq!((r.price_micro, r.conf_micro), (109_608_251, 24_684));
        assert!(check_price(&r, 1_791_729_350, 60, 100).is_ok());
    }

    #[test]
    fn pyth_parser_fails_closed() {
        let p = parse_price_update_v2(SOL_FIXTURE).unwrap();
        // Unconfigured feed and wrong feed.
        assert_eq!(
            pyth_reading(&p, &[0; 32]).unwrap_err(),
            ClearinghouseError::OracleNotConfigured.into()
        );
        let mut other = SOL_FEED;
        other[0] ^= 1;
        assert_eq!(
            pyth_reading(&p, &other).unwrap_err(),
            ClearinghouseError::InvalidOracleAccount.into()
        );
        // Wrong discriminator.
        let mut d = SOL_FIXTURE.to_vec();
        d[0] ^= 1;
        assert_eq!(
            parse_price_update_v2(&d).unwrap_err(),
            ClearinghouseError::InvalidOracleAccount.into()
        );
        // Partial verification.
        let mut d = SOL_FIXTURE.to_vec();
        d[40] = 0;
        assert_eq!(
            parse_price_update_v2(&d).unwrap_err(),
            ClearinghouseError::OracleNotFullyVerified.into()
        );
        // Truncated.
        for len in [0, 8, 40, 41, 124] {
            assert!(
                parse_price_update_v2(&SOL_FIXTURE[..len]).is_err(),
                "len {len}"
            );
        }
        // Non-positive price.
        let mut d = SOL_FIXTURE.to_vec();
        d[73..81].copy_from_slice(&(-1i64).to_le_bytes());
        let p = parse_price_update_v2(&d).unwrap();
        assert!(pyth_reading(&p, &SOL_FEED).is_err());
    }
}
