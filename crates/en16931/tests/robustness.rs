//! Property tests: for any constructible `Invoice` (including overflowing
//! amounts, empty collections and absurd values), validation terminates without
//! panicking, and its reports are deterministic and well-formed.

mod generators;

use en16931::invoice::*;
use en16931::{Date, InvoiceAmount, Quantity, profiles, validate};
use generators::{any_invoice, any_text};
use proptest::prelude::*;
use rust_decimal::Decimal;

proptest! {
    /// `validate` never panics, whatever the document.
    #[test]
    fn validate_never_panics(inv in any_invoice()) {
        let report = validate(&inv);
        // Touch every accessor.
        let _ = report.is_valid();
        let _ = report.rules_checked();
        let _ = report.fatal().count();
        let _ = report.warnings().count();
        let _ = report.info().count();
        let _ = report.advisory().count();
        let _ = report.to_string();
    }

    /// Nor does any shipped profile, which runs strictly more rules.
    #[test]
    fn no_profile_panics(inv in any_invoice()) {
        for p in profiles::ALL {
            let report = p.validate(&inv);
            let _ = report.is_valid();
            let _ = report.to_string();
        }
    }

    /// Rule, profile and severity lookups take arbitrary user text and never panic.
    #[test]
    fn a_rule_query_is_user_input_and_never_panics(q in any_text()) {
        use en16931::validation::rules::{explain, explain_restriction, touching};
        use en16931::validation::Check;

        let _ = explain(&q);
        let _ = explain_restriction(&q);
        let _ = q.parse::<&en16931::Profile>();
        let _ = q.parse::<en16931::Severity>();

        let report = validate(&Invoice::default());
        let _ = report.has(&q);

        let deviated = Check::of::<profiles::XRechnung>().without(&q).run(&Invoice::default());
        let _ = deviated.to_string();
        let _ = deviated.suppressed().len();

        let _ = touching(en16931::BtId(0));
    }

    /// Validation is deterministic and stably ordered.
    #[test]
    fn reports_are_deterministic(inv in any_invoice()) {
        let a = validate(&inv);
        let b = validate(&inv);
        prop_assert_eq!(a.findings().len(), b.findings().len());
        for (x, y) in a.findings().iter().zip(b.findings()) {
            prop_assert_eq!(&x.rule, &y.rule);
            prop_assert_eq!(x.path.to_string(), y.path.to_string());
        }
    }

    /// Every finding under every profile names a rule `explain` can resolve.
    #[test]
    fn every_finding_can_be_explained(inv in any_invoice()) {
        use en16931::validation::rules::{explain, explain_restriction};
        for p in profiles::ALL {
            for f in p.validate(&inv).findings() {
                prop_assert!(
                    explain(&f.rule).is_some() || explain_restriction(&f.rule).is_some(),
                    "{} reports {} and nothing can explain it", p.id, f.rule
                );
            }
        }
    }

    /// §4.4.4: a conformant CIUS never accepts a document core EN 16931 rejects
    /// (what makes `Validated::widen` infallible).
    #[test]
    fn conformant_profiles_never_accept_what_core_rejects(inv in any_invoice()) {
        for p in profiles::ALL.iter().filter(|p| p.is_conformant_cius()) {
            let mut doc = inv.clone();
            doc.specification_id = Some(p.specification_id.to_owned());
            if p.validate(&doc).is_valid() {
                prop_assert!(
                    validate(&doc).is_valid(),
                    "{} accepted a document core EN 16931 rejects", p.id
                );
            }
        }
    }

    /// `is_valid()` agrees with the absence of fatal findings, always.
    #[test]
    fn is_valid_means_no_fatal_findings(inv in any_invoice()) {
        let report = validate(&inv);
        prop_assert_eq!(report.is_valid(), report.fatal().count() == 0);
    }

    /// A finding's path carries an occurrence index only on a group that
    /// [`Group::repeats`]. (An unindexed repeating group is fine, e.g. `BR-16`.)
    #[test]
    fn every_finding_points_somewhere_that_can_exist(inv in any_invoice()) {
        for p in profiles::ALL {
            for f in p.validate(&inv).findings() {
                prop_assert!(
                    f.path.index.is_none() || f.path.group.repeats(),
                    "{} reports {} at {}, and {:?} occurs at most once",
                    p.id, f.rule, f.path, f.path.group
                );
            }
        }
    }
}

/// A minimal line carrying `net` (`InvoiceLine` has no `Default`).
fn line(net: InvoiceAmount) -> InvoiceLine {
    InvoiceLine {
        id: String::new(),
        note: None,
        order_line_reference: None,
        accounting_reference: None,
        object_identifier: None,
        quantity: Quantity::new(Decimal::ONE),
        unit_code: Code::new("C62"),
        net_amount: net,
        period: None,
        allowances: vec![],
        charges: vec![],
        price: PriceDetails::default(),
        vat: LineVat::default(),
        item: Item::default(),
    }
}

/// Named boundary cases.
#[test]
fn the_extremes_are_survivable() {
    // A sum that overflows `i64` twice over.
    let mut inv = Invoice::default();
    inv.totals.line_total = InvoiceAmount::from_minor_units(i64::MAX);
    inv.lines = (0..3)
        .map(|_| line(InvoiceAmount::from_minor_units(i64::MAX)))
        .collect();
    let _ = validate(&inv);

    // A zero base quantity: `R120` divides by it if nothing stops it.
    let mut inv = Invoice::default();
    let mut l = line(InvoiceAmount::ZERO);
    l.price.base_quantity = Some(Quantity::new(Decimal::ZERO));
    inv.lines = vec![l];
    for p in profiles::ALL {
        let _ = p.validate(&inv);
    }

    // A date at the calendar's edge.
    assert!(Date::parse("9999-12-31").is_ok());
    assert!(Date::parse("0000-01-01").is_ok());
}
