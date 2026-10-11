//! BT-20 payment terms with early-payment discounts, rendered in the form
//! XRechnung's `BR-DE-18` accepts.
//!
//! XRechnung encodes an early-payment discount (*Skonto*) inside BT-20, one
//! line per discount:
//!
//! ```text
//! #SKONTO#TAGE=10#PROZENT=2.00#
//! #SKONTO#TAGE=20#PROZENT=1.00#BASISBETRAG=500.00#
//! ```
//!
//! `BR-DE-18` requires exactly two decimals in `PROZENT` and a newline after
//! the last `#…#`; [`PaymentTerms`] renders both:
//!
//! ```
//! use en16931::{EarlyPaymentDiscount, Percentage, PaymentTerms};
//! use rust_decimal::dec;
//!
//! let terms = PaymentTerms::new("Zahlbar innerhalb 30 Tagen ohne Abzug.")?
//!     .with_discount(EarlyPaymentDiscount::new(10, Percentage::new(dec!(2)))?);
//! assert_eq!(
//!     terms.to_string(),
//!     "Zahlbar innerhalb 30 Tagen ohne Abzug.\n#SKONTO#TAGE=10#PROZENT=2.00#\n",
//! );
//! # Ok::<(), en16931::PaymentTermsError>(())
//! ```
//!
//! Put the rendering into BT-20 (`Invoice::payment_terms` or
//! `InvoiceBuilder::payment_terms`). Prose without a discount renders unchanged.
//! This type states the terms; it does not compute the discount amount — with
//! the `billing` feature, `billing::EarlyPaymentDiscount::amount` does.

use core::fmt;

use rust_decimal::Decimal;

use crate::{BtId, InvoiceAmount, Percentage};

/// Why a [`PaymentTerms`] or [`EarlyPaymentDiscount`] cannot be built. Every
/// variant concerns BT-20 ([`term`](Self::term)).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaymentTermsError {
    /// A discount period of zero days: `TAGE` counts the days the discount is
    /// open for.
    #[error("BT-20: an early-payment discount needs at least one day (TAGE)")]
    ZeroDays,
    /// A percentage below 0 or above 100.
    #[error("BT-20: an early-payment discount of {0} % is outside 0–100 (PROZENT)")]
    PercentOutOfRange(Decimal),
    /// A percentage with more than two decimals (`PROZENT` carries two).
    #[error("BT-20: {0} % needs more than the two decimals PROZENT carries")]
    PercentPrecision(Decimal),
    /// A prose line starting with `#`, which `BR-DE-18` would read as a
    /// malformed discount line.
    #[error("BT-20: the prose line {0:?} starts with `#`, which BR-DE-18 reads as a discount line")]
    HashLine(String),
}

impl PaymentTermsError {
    /// The business term every refusal concerns: BT-20.
    #[must_use]
    pub const fn term(&self) -> BtId {
        BtId(20)
    }
}

/// One early-payment discount: paid within `days`, `percent` off, optionally
/// of a stated base amount (`BASISBETRAG`). Rendered, not computed: the
/// amount comes from `billing::EarlyPaymentDiscount::amount`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EarlyPaymentDiscount {
    days: u32,
    percent: Percentage,
    base_amount: Option<InvoiceAmount>,
}

impl EarlyPaymentDiscount {
    /// `percent` off when paid within `days`.
    ///
    /// # Errors
    /// [`PaymentTermsError::ZeroDays`], [`PaymentTermsError::PercentOutOfRange`]
    /// (outside 0–100), [`PaymentTermsError::PercentPrecision`] (more than two
    /// decimals).
    pub fn new(days: u32, percent: Percentage) -> Result<Self, PaymentTermsError> {
        if days == 0 {
            return Err(PaymentTermsError::ZeroDays);
        }
        let p = percent.into_decimal();
        if p < Decimal::ZERO || p > Decimal::ONE_HUNDRED {
            return Err(PaymentTermsError::PercentOutOfRange(p));
        }
        if p.normalize().scale() > 2 {
            return Err(PaymentTermsError::PercentPrecision(p));
        }
        Ok(Self {
            days,
            percent,
            base_amount: None,
        })
    }

    /// State the amount the discount applies to (`BASISBETRAG`; may be
    /// negative). Without it, XRechnung applies the discount to the amount due.
    #[must_use]
    pub const fn with_base_amount(mut self, base: InvoiceAmount) -> Self {
        self.base_amount = Some(base);
        self
    }

    /// The days the discount is open for (`TAGE`).
    #[must_use]
    pub const fn days(&self) -> u32 {
        self.days
    }

    /// The discount in per cent (`PROZENT`).
    #[must_use]
    pub const fn percent(&self) -> Percentage {
        self.percent
    }

    /// The stated base amount (`BASISBETRAG`), if any.
    #[must_use]
    pub const fn base_amount(&self) -> Option<InvoiceAmount> {
        self.base_amount
    }
}

impl fmt::Display for EarlyPaymentDiscount {
    /// One `#SKONTO#…#` line, without the newline that follows it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#SKONTO#TAGE={}#PROZENT={:.2}#",
            self.days,
            self.percent.into_decimal()
        )?;
        if let Some(base) = self.base_amount {
            write!(f, "BASISBETRAG={base}#")?;
        }
        Ok(())
    }
}

/// BT-20 PAYMENT TERMS: prose plus zero or more [`EarlyPaymentDiscount`]s, in
/// order. `Display` renders the field; see the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct PaymentTerms {
    prose: String,
    discounts: Vec<EarlyPaymentDiscount>,
}

impl PaymentTerms {
    /// Terms with `prose` (may be empty) and no discount.
    ///
    /// # Errors
    /// [`PaymentTermsError::HashLine`] for a prose line whose text starts with
    /// `#`: `BR-DE-18` would check it as a discount line and reject the field.
    pub fn new(prose: impl Into<String>) -> Result<Self, PaymentTermsError> {
        let prose = prose.into();
        if let Some(line) = prose.lines().find(|l| l.trim().starts_with('#')) {
            return Err(PaymentTermsError::HashLine(line.to_owned()));
        }
        Ok(Self {
            prose,
            discounts: Vec::new(),
        })
    }

    /// Append a discount.
    #[must_use]
    pub fn with_discount(mut self, discount: EarlyPaymentDiscount) -> Self {
        self.discounts.push(discount);
        self
    }

    /// The prose.
    #[must_use]
    pub fn prose(&self) -> &str {
        &self.prose
    }

    /// The discounts, in order.
    #[must_use]
    pub fn discounts(&self) -> &[EarlyPaymentDiscount] {
        &self.discounts
    }
}

impl fmt::Display for PaymentTerms {
    /// The BT-20 text: the prose, then one line per discount, each ending with
    /// the newline `BR-DE-18` requires after the last `#…#`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.prose)?;
        if self.discounts.is_empty() {
            return Ok(());
        }
        if !self.prose.is_empty() && !self.prose.ends_with('\n') {
            f.write_str("\n")?;
        }
        for d in &self.discounts {
            writeln!(f, "{d}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Invoice;
    use crate::profiles::XRECHNUNG;
    use proptest::prelude::*;
    use rust_decimal::dec;

    fn pct(d: Decimal) -> Percentage {
        Percentage::new(d)
    }

    fn br_de_18_accepts(bt20: &str) -> bool {
        let inv = Invoice {
            payment_terms: Some(bt20.to_owned()),
            ..Default::default()
        };
        !XRECHNUNG.validate(&inv).has("BR-DE-18")
    }

    #[test]
    fn prose_and_one_discount() {
        let t = PaymentTerms::new("Zahlbar in 30 Tagen.")
            .unwrap()
            .with_discount(EarlyPaymentDiscount::new(10, pct(dec!(2))).unwrap());
        let s = t.to_string();
        assert_eq!(s, "Zahlbar in 30 Tagen.\n#SKONTO#TAGE=10#PROZENT=2.00#\n");
        assert!(br_de_18_accepts(&s));
    }

    #[test]
    fn a_whole_percentage_gets_two_decimals() {
        let d = EarlyPaymentDiscount::new(14, pct(dec!(3))).unwrap();
        assert_eq!(d.to_string(), "#SKONTO#TAGE=14#PROZENT=3.00#");
        let d = EarlyPaymentDiscount::new(14, pct(dec!(2.5))).unwrap();
        assert_eq!(d.to_string(), "#SKONTO#TAGE=14#PROZENT=2.50#");
    }

    #[test]
    fn a_base_amount_has_two_decimals_and_may_be_negative() {
        let d = EarlyPaymentDiscount::new(7, pct(dec!(1)))
            .unwrap()
            .with_base_amount(InvoiceAmount::parse("500").unwrap());
        assert_eq!(
            d.to_string(),
            "#SKONTO#TAGE=7#PROZENT=1.00#BASISBETRAG=500.00#"
        );
        let neg = EarlyPaymentDiscount::new(7, pct(dec!(1)))
            .unwrap()
            .with_base_amount(InvoiceAmount::parse("-12.5").unwrap());
        let t = PaymentTerms::default().with_discount(neg);
        assert_eq!(
            t.to_string(),
            "#SKONTO#TAGE=7#PROZENT=1.00#BASISBETRAG=-12.50#\n"
        );
        assert!(br_de_18_accepts(&t.to_string()));
    }

    #[test]
    fn prose_only_is_unchanged_and_unterminated() {
        let t = PaymentTerms::new("Zahlbar sofort ohne Abzug").unwrap();
        assert_eq!(t.to_string(), "Zahlbar sofort ohne Abzug");
    }

    #[test]
    fn prose_ending_in_a_newline_gets_no_blank_line() {
        let t = PaymentTerms::new("Zahlbar in 30 Tagen.\n")
            .unwrap()
            .with_discount(EarlyPaymentDiscount::new(10, pct(dec!(2))).unwrap());
        assert_eq!(
            t.to_string(),
            "Zahlbar in 30 Tagen.\n#SKONTO#TAGE=10#PROZENT=2.00#\n"
        );
    }

    #[test]
    fn the_four_refusals_name_bt_20() {
        let errs = [
            EarlyPaymentDiscount::new(0, pct(dec!(2))).unwrap_err(),
            EarlyPaymentDiscount::new(10, pct(dec!(100.01))).unwrap_err(),
            EarlyPaymentDiscount::new(10, pct(dec!(-1))).unwrap_err(),
            EarlyPaymentDiscount::new(10, pct(dec!(2.125))).unwrap_err(),
            PaymentTerms::new("ok\n  #SKONTO#").unwrap_err(),
        ];
        assert_eq!(errs[0], PaymentTermsError::ZeroDays);
        assert!(matches!(errs[1], PaymentTermsError::PercentOutOfRange(_)));
        assert!(matches!(errs[2], PaymentTermsError::PercentOutOfRange(_)));
        assert!(matches!(errs[3], PaymentTermsError::PercentPrecision(_)));
        assert!(matches!(errs[4], PaymentTermsError::HashLine(_)));
        for e in &errs {
            assert_eq!(e.term(), BtId(20));
            assert!(e.to_string().starts_with("BT-20"), "{e}");
        }
        // Trailing zeros are not precision: 2.500 is 2.50.
        assert!(EarlyPaymentDiscount::new(10, pct(dec!(2.500))).is_ok());
    }

    fn discount() -> impl Strategy<Value = EarlyPaymentDiscount> {
        (
            1u32..=u32::MAX,
            0i64..=10_000,
            proptest::option::of(-1_000_000_000i64..=1_000_000_000),
        )
            .prop_map(|(days, hundredths, base)| {
                let d =
                    EarlyPaymentDiscount::new(days, Percentage::new(Decimal::new(hundredths, 2)))
                        .expect("in range by construction");
                match base {
                    Some(b) => d.with_base_amount(InvoiceAmount::from_minor_units(b)),
                    None => d,
                }
            })
    }

    proptest! {
        /// Every value the constructors admit renders a BT-20 `BR-DE-18` accepts.
        #[test]
        fn every_rendering_passes_br_de_18(
            prose in "[A-Za-z0-9 .,;:€%()\n-]{0,80}",
            discounts in proptest::collection::vec(discount(), 0..4),
        ) {
            let mut t = PaymentTerms::new(prose).expect("no line starts with #");
            for d in discounts {
                t = t.with_discount(d);
            }
            let s = t.to_string();
            prop_assert!(br_de_18_accepts(&s), "{:?}", s);
        }
    }
}
