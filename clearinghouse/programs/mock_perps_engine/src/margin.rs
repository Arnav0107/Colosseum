//! Margin arithmetic for the reference engine (integer only, micro-USD).

use crate::{EngineConfig, EngineError, MarginAccount, Position};
use anchor_lang::prelude::*;

const E6: u128 = 1_000_000;

/// Position value at `price`: size_e6 · price / 1e6 (floored).
pub fn notional(size_e6: u64, price: u64) -> u64 {
    (size_e6 as u128 * price as u128 / E6) as u64
}

/// ceil(x · bps / 10_000): requirements round up.
pub fn bps_ceil(x: u64, bps: u16) -> u64 {
    (x as u128 * bps as u128).div_ceil(10_000) as u64
}

/// Signed unrealized P&L of one position at `price` (micro-USD).
pub fn pnl(p: &Position, price: u64) -> i128 {
    let now = notional(p.size_e6, price) as i128;
    let entry = notional(p.size_e6, p.entry_price) as i128;
    if p.is_long {
        now - entry
    } else {
        entry - now
    }
}

/// Equity = collateral + Σ unrealized P&L, with `asset` marked at `price`
/// and other positions at their entry price (P&L 0).
pub fn equity_at(acct: &MarginAccount, asset: u8, price: u64) -> Result<i128> {
    let mut e = acct.collateral as i128;
    for p in acct
        .positions
        .iter()
        .filter(|p| p.active && p.asset == asset)
    {
        e += pnl(p, price);
    }
    Ok(e)
}

/// Add `notional_delta` (micro-USD at `price`) to the position in `asset`.
pub fn increase(
    acct: &mut MarginAccount,
    asset: u8,
    is_long: bool,
    notional_delta: u64,
    price: u64,
) -> Result<()> {
    require!(price > 0, EngineError::InvalidPrice);
    let add = (notional_delta as u128 * E6 / price as u128) as u64;
    require!(add > 0, EngineError::ZeroSize);
    if let Some(p) = acct
        .positions
        .iter_mut()
        .find(|p| p.active && p.asset == asset)
    {
        require!(p.is_long == is_long, EngineError::SideMismatch);
        let old = p.size_e6 as u128;
        let new = old + add as u128;
        // Weighted average entry.
        p.entry_price = ((old * p.entry_price as u128 + add as u128 * price as u128) / new) as u64;
        p.size_e6 = u64::try_from(new).map_err(|_| error!(EngineError::MathOverflow))?;
        return Ok(());
    }
    let slot = acct
        .positions
        .iter_mut()
        .find(|p| !p.active)
        .ok_or(EngineError::TooManyPositions)?;
    *slot = Position {
        active: true,
        asset,
        is_long,
        size_e6: add,
        entry_price: price,
    };
    Ok(())
}

/// Remove `notional_delta` (micro-USD at `price`); realize the P&L of the
/// removed part into collateral (losses beyond collateral become bad debt).
/// Returns true when the position is fully closed.
pub fn decrease(
    acct: &mut MarginAccount,
    asset: u8,
    notional_delta: u64,
    price: u64,
) -> Result<bool> {
    require!(price > 0, EngineError::InvalidPrice);
    let i = acct
        .positions
        .iter()
        .position(|p| p.active && p.asset == asset)
        .ok_or(EngineError::NoPosition)?;
    let p = acct.positions[i];
    let remove = ((notional_delta as u128 * E6 / price as u128) as u64).min(p.size_e6);
    let part = Position {
        size_e6: remove,
        ..p
    };
    let realized = pnl(&part, price);
    let c = acct.collateral as i128 + realized;
    if c < 0 {
        acct.bad_debt = acct.bad_debt.saturating_add((-c) as u64);
        acct.collateral = 0;
    } else {
        acct.collateral = c as u64;
    }
    let left = p.size_e6 - remove;
    if left == 0 {
        acct.positions[i] = Position::default();
        Ok(true)
    } else {
        acct.positions[i].size_e6 = left;
        Ok(false)
    }
}

/// ch_core `MockPrice` for `asset`: owned by ch_core, PDA
/// `[b"mock_price", [asset]]`, discriminator, matching asset id.
pub fn read_mock_price(info: &AccountInfo, asset: u8) -> Result<u64> {
    require_keys_eq!(*info.owner, ch_core::ID, EngineError::InvalidPrice);
    let (pda, _) = Pubkey::find_program_address(&[b"mock_price", &[asset]], &ch_core::ID);
    require_keys_eq!(info.key(), pda, EngineError::InvalidPrice);
    let data = info.try_borrow_data()?;
    let m = ch_core::MockPrice::try_deserialize(&mut &data[..])
        .map_err(|_| error!(EngineError::InvalidPrice))?;
    require!(
        m.asset_id == asset && m.price_micro > 0,
        EngineError::InvalidPrice
    );
    Ok(m.price_micro)
}

/// (equity, Σ IM, Σ MM) of the whole account with every position marked at
/// its oracle price (remaining accounts: one `MockPrice` per open position,
/// any order). Credit is not involved.
pub fn account_requirements(
    acct: &MarginAccount,
    engine: &EngineConfig,
    oracles: &[AccountInfo],
) -> Result<(i128, u64, u64)> {
    let mut equity = acct.collateral as i128;
    let (mut im, mut mm) = (0u64, 0u64);
    for p in acct.positions.iter().filter(|p| p.active) {
        let price = oracles
            .iter()
            .find_map(|o| read_mock_price(o, p.asset).ok())
            .ok_or(EngineError::InvalidPrice)?;
        equity += pnl(p, price);
        let n = notional(p.size_e6, price);
        im = im.saturating_add(bps_ceil(n, engine.im_bps));
        mm = mm.saturating_add(bps_ceil(n, engine.mm_bps));
    }
    Ok((equity, im, mm))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct() -> MarginAccount {
        MarginAccount {
            user: Pubkey::default(),
            collateral: 10_000_000_000,
            bad_debt: 0,
            positions: [Position::default(); crate::MAX_POSITIONS],
            bump: 0,
        }
    }

    #[test]
    fn open_increase_reduce_and_realize() {
        let mut a = acct();
        increase(&mut a, 0, true, 50_000_000_000, 150_000_000).unwrap();
        let p = a.positions[0];
        assert_eq!(
            notional(p.size_e6, 150_000_000),
            49_999_999_950,
            "size floored"
        );
        // +10%: unrealized +$5k.
        assert_eq!(pnl(&p, 165_000_000) / 1_000_000, 4_999);
        // Same side adds at a new average entry; opposite side is refused.
        increase(&mut a, 0, true, 10_000_000_000, 165_000_000).unwrap();
        assert!(
            a.positions[0].entry_price > 150_000_000 && a.positions[0].entry_price < 165_000_000
        );
        assert!(increase(&mut a, 0, false, 1_000_000, 150_000_000).is_err());
        // Close everything at the entry: no P&L.
        let entry = a.positions[0].entry_price;
        let size = notional(a.positions[0].size_e6, entry);
        assert!(decrease(&mut a, 0, size + 1_000_000, entry).unwrap());
        assert!(!a.positions[0].active);
        assert!((a.collateral as i128 - 10_000_000_000).abs() <= 2);
    }

    #[test]
    fn losses_beyond_collateral_become_bad_debt() {
        let mut a = acct();
        increase(&mut a, 0, true, 100_000_000_000, 100_000_000).unwrap();
        // -20% on $100k = -$20k against $10k collateral.
        assert!(decrease(&mut a, 0, 80_000_000_000, 80_000_000).unwrap());
        assert_eq!(a.collateral, 0);
        assert!(a.bad_debt >= 9_999_000_000 && a.bad_debt <= 10_000_000_000);
    }

    #[test]
    fn requirements_round_up() {
        assert_eq!(bps_ceil(1, 2_000), 1);
        assert_eq!(bps_ceil(50_000_000_000, 2_000), 10_000_000_000);
    }
}
