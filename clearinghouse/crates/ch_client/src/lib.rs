/// Calculates the effective required margin for a venue given an active margin credit.
/// Returns `own_required.saturating_sub(credit_amount)` if `now_slot <= valid_until_slot`,
/// otherwise returns `own_required` (expired credit provides zero relief).
pub fn effective_required_margin(
    own_required: u64,
    credit_amount: u64,
    valid_until_slot: u64,
    now_slot: u64,
) -> u64 {
    if now_slot <= valid_until_slot {
        own_required.saturating_sub(credit_amount)
    } else {
        own_required
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effective_required_margin_active() {
        assert_eq!(
            effective_required_margin(10_000_000_000, 3_000_000_000, 100, 50),
            7_000_000_000
        );
    }

    #[test]
    fn test_effective_required_margin_credit_exceeds_own() {
        // Credit > own_required saturates to 0
        assert_eq!(
            effective_required_margin(5_000_000_000, 8_000_000_000, 100, 50),
            0
        );
    }

    #[test]
    fn test_effective_required_margin_expiry_boundary() {
        // now_slot == valid_until_slot -> valid and active
        assert_eq!(
            effective_required_margin(10_000_000_000, 4_000_000_000, 100, 100),
            6_000_000_000
        );

        // now_slot == valid_until_slot + 1 -> expired, full own requirement
        assert_eq!(
            effective_required_margin(10_000_000_000, 4_000_000_000, 100, 101),
            10_000_000_000
        );
    }
}
