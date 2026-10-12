//! Regime-aware credit throttle (B3).
//!
//! At refresh, each leg's oracle reading gives two ratios:
//! * `m` = |price − ema| / ema in bps (rounded up), the move away from the
//!   oracle's own exponential moving average;
//! * `c` = conf / price in bps (rounded up), the confidence width.
//!
//! Each maps to a scale `lin(x; start, full)` = 1 for x ≤ start, 0 for
//! x ≥ full, linear (floored, ppm) in between; `start = 0` disables that
//! ratio. A leg's scale is the smaller of the two, and the portfolio's is the
//! smallest over its legs. Every credit is multiplied by it (floored). The
//! scale only ever lowers credit.
//!
//! Evidence and the chosen thresholds: docs/BACKTEST.md ("Regime throttle").

use crate::price_move_bps;

pub const SCALE_ONE: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThrottleParams {
    /// Move from the EMA (bps) up to which credit is full; 0 = move part off.
    pub move_start_bps: u16,
    /// Move (bps) at which credit reaches 0.
    pub move_full_bps: u16,
    /// Confidence width (bps) up to which credit is full; 0 = conf part off.
    pub conf_start_bps: u16,
    /// Confidence width (bps) at which credit reaches 0 (`max_conf_bps`).
    pub conf_full_bps: u16,
}

/// 1e6 if x ≤ start (or start = 0), 0 if x ≥ full (or full ≤ start), linear between.
pub fn lin_ppm(x: u64, start: u16, full: u16) -> u32 {
    if start == 0 || x <= start as u64 {
        return SCALE_ONE;
    }
    if full <= start || x >= full as u64 {
        return 0;
    }
    let num = SCALE_ONE as u64 * (full as u64 - x);
    (num / (full - start) as u64) as u32
}

/// ceil(conf · 10_000 / price); u64::MAX when price is 0 (fail closed).
pub fn conf_bps(price: u64, conf: u64) -> u64 {
    if price == 0 {
        return u64::MAX;
    }
    let num = conf as u128 * 10_000;
    let p = price as u128;
    u64::try_from(num.div_ceil(p)).unwrap_or(u64::MAX)
}

/// Scale of one leg from its oracle reading.
pub fn leg_scale_ppm(price: u64, ema: u64, conf: u64, p: &ThrottleParams) -> u32 {
    let m = price_move_bps(ema, price);
    let c = conf_bps(price, conf);
    lin_ppm(m, p.move_start_bps, p.move_full_bps).min(lin_ppm(c, p.conf_start_bps, p.conf_full_bps))
}

/// floor(credit · scale / 1e6).
pub fn apply_scale(credit: u64, scale_ppm: u32) -> u64 {
    (credit as u128 * scale_ppm.min(SCALE_ONE) as u128 / SCALE_ONE as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: ThrottleParams = ThrottleParams {
        move_start_bps: 50,
        move_full_bps: 150,
        conf_start_bps: 50,
        conf_full_bps: 100,
    };

    #[test]
    fn linear_between_thresholds_and_off_when_start_is_zero() {
        assert_eq!(lin_ppm(0, 50, 150), SCALE_ONE);
        assert_eq!(lin_ppm(50, 50, 150), SCALE_ONE);
        assert_eq!(lin_ppm(100, 50, 150), 500_000);
        assert_eq!(lin_ppm(149, 50, 150), 10_000);
        assert_eq!(lin_ppm(150, 50, 150), 0);
        assert_eq!(lin_ppm(u64::MAX, 50, 150), 0);
        assert_eq!(lin_ppm(u64::MAX, 0, 0), SCALE_ONE, "off");
        assert_eq!(lin_ppm(60, 50, 50), 0, "full <= start fails safe");
    }

    #[test]
    fn leg_scale_takes_the_worse_of_move_and_confidence() {
        // Price 101 vs EMA 100: 100 bps move -> 0.5; conf 0.3 -> 30 bps -> 1.
        assert_eq!(
            leg_scale_ppm(101_000_000, 100_000_000, 300_000, &P),
            500_000
        );
        // No move, conf 75 bps -> 0.5.
        assert_eq!(
            leg_scale_ppm(100_000_000, 100_000_000, 750_000, &P),
            500_000
        );
        // Both: the smaller wins.
        assert_eq!(
            leg_scale_ppm(101_000_000, 100_000_000, 900_000, &P),
            200_000
        );
        // Zero price fails closed.
        assert_eq!(leg_scale_ppm(0, 100_000_000, 0, &P), 0);
        assert_eq!(apply_scale(7_111_111_110, 500_000), 3_555_555_555);
        assert_eq!(apply_scale(7, SCALE_ONE), 7);
        assert_eq!(apply_scale(u64::MAX, SCALE_ONE), u64::MAX);
    }
}
