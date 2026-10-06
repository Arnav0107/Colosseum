// TODO(owner): Dev B

/// Calculates the effective required margin for a venue:
/// required_margin = own_margin - credit
pub fn calculate_required_margin(own_margin: u64, credit: u64) -> u64 {
    own_margin.saturating_sub(credit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_required_margin_stub() {
        assert_eq!(calculate_required_margin(10_000, 3_000), 7_000);
        assert_eq!(calculate_required_margin(5_000, 8_000), 0);
    }
}
