//! The arithmetic the VAT-derivation rules share, written the way the
//! artefacts write it:
//!
//! ```xpath
//! round(abs(TaxableAmount) * (Percent div 100) * 10 * 10) div 100
//! ```
//!
//! guarded by `round(Percent) = 0` for the zero-rate branch. XPath's `round`
//! sends ties towards +∞, which no `rust_decimal::RoundingStrategy` matches:
//!
//! | | `round(0.5)` | `round(2.5)` | `round(-0.5)` |
//! |---|---|---|---|
//! | XPath `fn:round` | `1` | `3` | `0` |
//! | `Decimal::round` — banker's | `0` | `2` | `0` |
//! | half away from zero | `1` | `3` | `-1` |
//!
//! A 0.5 % VAT rate is the case that tells them apart.

use rust_decimal::Decimal;

/// The ±1.00 the artefacts allow on `BR-CO-17` and the category `-08`/`-09`
/// rows. Not in EN 16931-1 §6.4.2, which states a plain equation.
pub(crate) const VAT_TOLERANCE: Decimal = Decimal::ONE;

/// `0.5`, as the constant `floor(x + 0.5)` needs.
const HALF: Decimal = Decimal::from_parts(5, 0, 0, false, 1);

/// XPath's `fn:round` — `floor(x + 0.5)`, ties towards +∞. Returns `x`
/// unchanged if `x + 0.5` overflows.
pub(crate) fn xpath_round(x: Decimal) -> Decimal {
    x.checked_add(HALF).map_or(x, |shifted| shifted.floor())
}

/// The artefacts' derived VAT amount: `round(|base| × rate ÷ 100 × 100) ÷ 100`.
///
/// Computed as the equivalent `round(|base| × rate) ÷ 100`. `None` when the
/// product overflows; callers skip that group.
pub(crate) fn derived_vat(base: Decimal, rate: Decimal) -> Option<Decimal> {
    base.abs()
        .checked_mul(rate)
        .map(|product| xpath_round(product) / Decimal::ONE_HUNDRED)
}

/// Whether a stated VAT amount is within the artefacts' ±1.00 of `expected`.
///
/// Strict, as the artefacts write it: a difference of exactly 1.00 fails.
pub(crate) fn within_vat_tolerance(stated: Decimal, expected: Decimal) -> bool {
    (stated - expected).abs() < VAT_TOLERANCE
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;

    #[test]
    fn ties_go_towards_positive_infinity() {
        assert_eq!(xpath_round(dec!(0.5)), dec!(1));
        assert_eq!(xpath_round(dec!(2.5)), dec!(3));
        assert_eq!(xpath_round(dec!(-0.5)), dec!(0));
        assert_eq!(xpath_round(dec!(-1.5)), dec!(-1));
        assert_eq!(xpath_round(dec!(0.4)), dec!(0));
        assert_eq!(xpath_round(dec!(0.6)), dec!(1));
        assert_eq!(xpath_round(dec!(-0.6)), dec!(-1));
        assert_eq!(xpath_round(dec!(19)), dec!(19));
    }

    #[test]
    fn a_rate_of_half_a_per_cent_is_not_a_zero_rate() {
        assert_ne!(xpath_round(dec!(0.5)), Decimal::ZERO);
        assert_eq!(derived_vat(dec!(1000.00), dec!(0.5)), Some(dec!(5)));
    }

    #[test]
    fn the_derivation_is_taken_on_absolute_values() {
        assert_eq!(
            derived_vat(dec!(-1000.00), dec!(19)),
            derived_vat(dec!(1000.00), dec!(19))
        );
    }

    #[test]
    fn a_full_currency_unit_of_slack_excludes_its_own_boundary() {
        assert!(within_vat_tolerance(dec!(190.99), dec!(190.00)));
        assert!(!within_vat_tolerance(dec!(191.00), dec!(190.00)));
        assert!(!within_vat_tolerance(dec!(189.00), dec!(190.00)));
    }
}
