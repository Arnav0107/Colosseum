//! Venue-side rules for Clearinghouse credit.
//!
//! # What a credit is — and is not
//!
//! Clearinghouse distinguishes four capabilities and implements only the first two:
//!
//! 1. **Calculated margin relief** — a number computed on-chain from the
//!    user's consented positions across venues. *Implemented.*
//! 2. **Venue-recognized trading-power credit** — a venue that has integrated
//!    these rules may lower its **initial**-margin requirement for
//!    risk-increasing actions by that number. *Implemented (opt-in by venues).*
//! 3. **Enforceable collateral or guarantee coverage** — someone pays if the
//!    user defaults. *Not implemented. There is no funded backstop.*
//! 4. **Cross-venue P&L settlement** — gains on one venue cover losses on
//!    another. *Not implemented. Collateral never moves between venues.*
//!
//! A credit is therefore `TradingPowerOnly`:
//!
//! * it may only reduce the initial-margin requirement, and never below the
//!   venue's own maintenance margin: `effective IM = max(IM − credit, MM)`;
//! * it never changes the maintenance margin or the liquidation threshold, so
//!   losing a credit can never by itself make a position liquidatable;
//! * it never counts towards withdrawable equity;
//! * it is meaningless to a venue that does not enforce the rules above.
//!
//! # The validity check
//!
//! Before using a credit a venue must check, in the same transaction:
//! the portfolio account is owned by the Clearinghouse program and is the PDA
//! `[b"portfolio", user]`; the config account is the PDA `[b"config"]`;
//! and then [`usable_credit`]: status Active, `risk_epoch` equal to the
//! config's current epoch, `now_slot <= expires_slot`, credit kind
//! TradingPowerOnly, and a leg for exactly this venue index and asset.
//! Anything else is zero relief, even though the account still exists.
//!
//! This crate is dependency-free; it mirrors `ch_core::Portfolio::usable_credit`
//! (the LiteSVM tests check both agree).

/// `Portfolio.status` value for a usable computation.
pub const STATUS_ACTIVE: u8 = 1;
/// The only credit kind.
pub const CREDIT_KIND_TRADING_POWER_ONLY: u8 = 1;

/// One leg's credit as stored in the portfolio account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegCredit {
    pub venue_index: u8,
    pub asset_id: u8,
    pub credit: u64,
}

/// The fields of a `Portfolio` account a venue needs.
#[derive(Clone, Copy, Debug)]
pub struct PortfolioView<'a> {
    pub status: u8,
    pub credit_kind: u8,
    pub risk_epoch: u64,
    pub expires_slot: u64,
    pub legs: &'a [LegCredit],
}

/// Credit a venue may apply right now for (venue_index, asset_id); 0 if any
/// condition fails.
pub fn usable_credit(
    p: &PortfolioView,
    config_risk_epoch: u64,
    now_slot: u64,
    venue_index: u8,
    asset_id: u8,
) -> u64 {
    if p.status != STATUS_ACTIVE
        || p.credit_kind != CREDIT_KIND_TRADING_POWER_ONLY
        || p.risk_epoch != config_risk_epoch
        || now_slot > p.expires_slot
    {
        return 0;
    }
    p.legs
        .iter()
        .find(|l| l.venue_index == venue_index && l.asset_id == asset_id)
        .map(|l| l.credit)
        .unwrap_or(0)
}

/// Initial-margin requirement after applying a usable credit:
/// `max(IM − credit, MM)`. If the venue's own inputs are inconsistent
/// (MM > IM) the credit is ignored and IM is returned.
pub fn effective_initial_margin(initial_margin: u64, maintenance_margin: u64, credit: u64) -> u64 {
    if maintenance_margin > initial_margin {
        return initial_margin;
    }
    initial_margin
        .saturating_sub(credit)
        .max(maintenance_margin)
}

/// The maintenance requirement is the venue's own; credit has no input here.
/// Provided so integrations have one obvious place to call, and so the rule is
/// pinned by a test.
pub fn maintenance_requirement(maintenance_margin: u64) -> u64 {
    maintenance_margin
}

/// Equity a user may withdraw: equity above the *uncredited* initial margin.
/// Credit never counts as withdrawable.
pub fn withdrawable_equity(equity: u64, initial_margin_without_credit: u64) -> u64 {
    equity.saturating_sub(initial_margin_without_credit)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGS: [LegCredit; 2] = [
        LegCredit {
            venue_index: 0,
            asset_id: 0,
            credit: 3_000,
        },
        LegCredit {
            venue_index: 1,
            asset_id: 1,
            credit: 2_000,
        },
    ];

    fn view(status: u8, epoch: u64, expires: u64) -> PortfolioView<'static> {
        PortfolioView {
            status,
            credit_kind: CREDIT_KIND_TRADING_POWER_ONLY,
            risk_epoch: epoch,
            expires_slot: expires,
            legs: &LEGS,
        }
    }

    #[test]
    fn usable_only_when_active_current_unexpired_and_matching_leg() {
        let v = view(STATUS_ACTIVE, 7, 100);
        assert_eq!(
            usable_credit(&v, 7, 100, 0, 0),
            3_000,
            "valid through expires_slot"
        );
        assert_eq!(usable_credit(&v, 7, 101, 0, 0), 0, "expired");
        assert_eq!(usable_credit(&v, 8, 50, 0, 0), 0, "epoch moved on");
        assert_eq!(usable_credit(&v, 7, 50, 0, 1), 0, "no leg for this asset");
        assert_eq!(usable_credit(&v, 7, 50, 2, 0), 0, "not this venue's leg");
        assert_eq!(
            usable_credit(&view(2, 7, 100), 7, 50, 0, 0),
            0,
            "invalidated"
        );
        assert_eq!(
            usable_credit(&view(0, 7, 100), 7, 50, 0, 0),
            0,
            "never computed"
        );
        let mut other_kind = view(STATUS_ACTIVE, 7, 100);
        other_kind.credit_kind = 2;
        assert_eq!(
            usable_credit(&other_kind, 7, 50, 0, 0),
            0,
            "unknown credit kind"
        );
    }

    #[test]
    fn effective_im_never_drops_below_mm() {
        assert_eq!(effective_initial_margin(10_000, 5_000, 3_000), 7_000);
        assert_eq!(effective_initial_margin(10_000, 5_000, 5_000), 5_000);
        assert_eq!(effective_initial_margin(10_000, 5_000, 9_999), 5_000);
        assert_eq!(effective_initial_margin(10_000, 5_000, u64::MAX), 5_000);
        assert_eq!(effective_initial_margin(10_000, 5_000, 0), 10_000);
        // Inconsistent venue inputs: credit ignored.
        assert_eq!(effective_initial_margin(4_000, 5_000, 1_000), 4_000);
    }

    #[test]
    fn prop_effective_im_is_between_mm_and_im() {
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..20_000 {
            let im = next() % 1_000_000_000_000;
            let mm = if im == 0 { 0 } else { next() % (im + 1) };
            let credit = next();
            let e = effective_initial_margin(im, mm, credit);
            assert!(e >= mm && e <= im);
        }
    }

    /// F1, "Cost 2" scenario from the idea PDF: venue A holds $2,000 equity;
    /// its own IM is $10,000 and MM $5,000. Under v0 semantics the credit
    /// ($5,470.18) gave an effective requirement of $4,529.82 and was presented
    /// as if it protected the account. Under the TradingPowerOnly rule the
    /// credit is capped at IM − MM and cannot touch the liquidation threshold:
    /// A is liquidatable exactly when its own MM engine says so.
    #[test]
    fn f1_cost2_credit_cannot_prevent_or_cause_liquidation() {
        let (im, mm, equity) = (10_000_000_000u64, 5_000_000_000u64, 2_000_000_000u64);
        let v0_credit = 5_470_177_871u64;
        // v0 effective requirement (IM − credit) went below MM.
        assert_eq!(im - v0_credit, 4_529_822_129);
        assert!(im - v0_credit < mm);

        // Now: with or without credit, A's liquidation test is equity < MM.
        let liquidatable = |_credit: u64| equity < maintenance_requirement(mm);
        assert!(liquidatable(0));
        assert!(liquidatable(v0_credit));
        assert!(liquidatable(im));
        // Credit only affects new risk-increasing actions, floored at MM.
        assert_eq!(effective_initial_margin(im, mm, v0_credit), mm);
        // And it never makes equity withdrawable.
        assert_eq!(withdrawable_equity(equity, im), 0);
    }

    /// Losing a credit (invalidation, expiry, epoch bump) moves the effective
    /// IM back up to IM but never changes MM, so it cannot by itself trigger a
    /// liquidation (defuses F6).
    #[test]
    fn invalidation_never_changes_the_liquidation_threshold() {
        let (im, mm) = (10_000u64, 5_000u64);
        let before = view(STATUS_ACTIVE, 1, 100);
        let after = view(2, 1, 100);
        let c_before = usable_credit(&before, 1, 10, 0, 0);
        let c_after = usable_credit(&after, 1, 10, 0, 0);
        assert!(c_before > 0 && c_after == 0);
        assert_eq!(maintenance_requirement(mm), mm);
        assert!(
            effective_initial_margin(im, mm, c_after) >= effective_initial_margin(im, mm, c_before)
        );
    }
}
