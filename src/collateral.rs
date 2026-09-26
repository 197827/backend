//! Collateral requirements for writing (selling) options, ported bit-for-bit
//! from the frontend's `lib/collateral.ts`: covered calls are 100% covered
//! by the underlying's current value, cash-secured puts are
//! over-collateralized by 110% of the strike (protects against a further
//! drop before the writer can react). Only applies to the short/write
//! side — buying an option never requires collateral, just the premium.
//!
//! Monetary results are returned as fixed-point [`Money`] (a `rust_decimal`
//! newtype) rather than `f64`. The pricing inputs (`strike`, `spot`) stay
//! `f64` because the pricing math is `f64` internally; the conversion
//! boundary is explicit via [`Money::from_price`], which rejects NaN/Inf.
//! Collateral is rounded *against the user* (up, away from zero) so a writer
//! is never under-collateralized by a rounding artifact.

use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;

/// Fixed-point monetary amount backed by `rust_decimal::Decimal`.
///
/// Display uses banker's rounding; fees and collateral use
/// round-against-the-user (see [`Money::round_against_user`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money(Decimal);

impl Money {
    /// Explicit conversion boundary from the `f64` pricing math.
    ///
    /// Returns `None` for NaN and ±Inf so callers can surface an error
    /// instead of silently propagating a non-finite balance.
    pub fn from_price(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        Decimal::from_f64_retain(value).map(Money)
    }

    /// Round-against-the-user: round away from zero to `scale` decimal
    /// places. Used for fees and collateral so the protocol never loses.
    pub fn round_against_user(self, scale: u32) -> Self {
        Money(self.0.round_dp_with_strategy(
            scale,
            RoundingStrategy::AwayFromZero,
        ))
    }

    /// Banker's rounding (round-half-to-even) for display purposes.
    pub fn round_for_display(self, scale: u32) -> Self {
        Money(self.0.round_dp_with_strategy(
            scale,
            RoundingStrategy::ToEven,
        ))
    }

    /// Checked multiplication; returns `None` on overflow instead of panicking.
    pub fn checked_mul(self, rhs: Self) -> Option<Self> {
        self.0.checked_mul(rhs.0).map(Money)
    }

    /// Checked addition; returns `None` on overflow instead of panicking.
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Money)
    }

    /// The underlying decimal value.
    pub fn value(self) -> Decimal {
        self.0
    }
}

impl std::fmt::Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Banker's rounding for display, then render as a plain string so
        // JSON consumers never see a JS float precision artifact.
        write!(f, "{}", self.round_for_display(7).0)
    }
}

/// Collateral required to write an option, as fixed-point [`Money`].
///
/// Covered calls are 100% of the underlying's current value; cash-secured
/// puts are 110% of the strike. Returns `None` if any input is non-finite
/// or if the multiplication overflows.
pub fn collateral_required(
    option_type: &str,
    contracts: f64,
    strike: f64,
    spot: f64,
) -> Option<Money> {
    let contracts = Money::from_price(contracts)?;
    if option_type == "call" {
        let spot = Money::from_price(spot)?;
        contracts.checked_mul(spot)
    } else {
        let strike = Money::from_price(strike)?;
        // 110% over-collateralization, expressed exactly as a decimal.
        let buffer = Money::from_price(1.1)?;
        contracts.checked_mul(strike)?.checked_mul(buffer)
    }
    .map(|m| m.round_against_user(7))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covered_call_is_100_percent_of_spot() {
        let got = collateral_required("call", 2.0, 70000.0, 67420.50).unwrap();
        let want = Money::from_price(2.0 * 67420.50).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn cash_secured_put_is_110_percent_of_strike() {
        let got = collateral_required("put", 3.0, 60000.0, 67420.50).unwrap();
        let want = Money::from_price(3.0 * 60000.0 * 1.1).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn rejects_nan_and_inf() {
        assert!(collateral_required("call", f64::NAN, 70000.0, 67420.50).is_none());
        assert!(collateral_required("call", 2.0, 70000.0, f64::INFINITY).is_none());
        assert!(Money::from_price(f64::NEG_INFINITY).is_none());
    }

    #[test]
    fn collateral_rounds_against_the_user() {
        // 1 contract * 0.00000001 strike * 1.1 = 0.000000011 -> rounds up to 1e-7.
        let got = collateral_required("put", 1.0, 0.00000001, 1.0).unwrap();
        assert_eq!(got.value(), rust_decimal::dec!(0.0000001));
    }
}
