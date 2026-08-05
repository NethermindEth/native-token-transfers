use soroban_ntt_client::TrimmedAmount;

use crate::WrapperError;

/// Denominator for `dbps` (tenths of a basis point): `100_000` represents 100%.
const DBPS_DENOMINATOR: u128 = 100_000;

/// Computes the referrer fee in the transfer token's own decimals, cleaned to
/// the transfer's trimmed granularity so the fee never leaves dust that the
/// bridge would silently drop.
///
/// Returns `0` when `dbps` is zero or `amount` is non-positive. `dbps` is
/// tenths of a basis point over [`DBPS_DENOMINATOR`] and must fit in the
/// on-wire `u16` fee field; a larger value is `InvalidReferrerFee`.
pub fn referrer_fee(
    amount: i128,
    dbps: u32,
    from_decimals: u8,
    to_decimals: u8,
) -> Result<i128, WrapperError> {
    if dbps > u16::MAX as u32 {
        return Err(WrapperError::InvalidReferrerFee);
    }
    if dbps == 0 || amount <= 0 {
        return Ok(0);
    }
    let mut raw = amount as u128 * dbps as u128 / DBPS_DENOMINATOR;
    TrimmedAmount::remove_dust(&mut raw, from_decimals, to_decimals)
        .map_err(|_| WrapperError::InvalidReferrerFee)?;
    Ok(raw as i128)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fee must pass straight through when source and destination share
    // decimals: no trimming, no dust. A regression here would silently shrink
    // fees on same-precision routes.
    #[test]
    fn same_decimals_passthrough() {
        assert_eq!(referrer_fee(1_000_000, 1_000, 6, 6), Ok(10_000));
    }

    // Cross-decimal fees must be trimmed to the coarser granularity, dropping
    // the low digits. Catches a fee that carries dust the manager cannot bridge.
    #[test]
    fn cross_decimal_removes_dust() {
        // raw = 12_345_678 * 10_000 / 100_000 = 1_234_567; trimmed 8->6 (scale 100).
        assert_eq!(referrer_fee(12_345_678, 10_000, 8, 6), Ok(1_234_500));
    }

    // Zero dbps must yield no fee regardless of amount. Catches an accidental
    // minimum-fee or off-by-one that charges on a disabled referrer.
    #[test]
    fn zero_dbps_is_no_fee() {
        assert_eq!(referrer_fee(1_000_000, 0, 6, 6), Ok(0));
    }

    // Non-positive amounts must yield no fee rather than a negative or panicking
    // multiplication.
    #[test]
    fn non_positive_amount_is_no_fee() {
        assert_eq!(referrer_fee(0, 1_000, 6, 6), Ok(0));
        assert_eq!(referrer_fee(-1, 1_000, 6, 6), Ok(0));
    }

    // dbps above the u16 wire field must be rejected, not truncated. u16::MAX is
    // the accepted boundary and still computes a fee.
    #[test]
    fn out_of_range_dbps_rejected() {
        assert_eq!(
            referrer_fee(1_000_000, 65_536, 6, 6),
            Err(WrapperError::InvalidReferrerFee)
        );
        assert_eq!(referrer_fee(1_000_000, 65_535, 6, 6), Ok(655_350));
    }

    // Realistic 18->6 route: the fee rounds down to a clean 6-decimal unit,
    // proving remove_dust runs in the token's native (18) domain.
    #[test]
    fn realistic_18_to_6_rounds_down() {
        // raw = 1_234_567_000_000_000_000 / 1_000 = 1_234_567_000_000_000;
        // trimmed 18->6 (scale 1e12) drops 567_000_000_000 of dust.
        assert_eq!(
            referrer_fee(1_234_567_000_000_000_000, 100, 18, 6),
            Ok(1_234_000_000_000_000),
        );
    }
}
