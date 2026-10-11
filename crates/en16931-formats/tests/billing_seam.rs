#![cfg(all(feature = "ubl", feature = "cii"))]

//! A derived `billing` document, through `en16931`'s adapter, written as UBL
//! and as CII: nothing is dropped, and every amount reads back as written.

mod common;

use en16931::{Invoice, validate};
use en16931_formats::Syntax;

/// The amounts the seam took from the `billing` document, in a form that
/// compares field by field.
fn amounts(inv: &Invoice) -> Vec<String> {
    let t = &inv.totals;
    let mut out = vec![format!(
        "{} {:?} {:?} {} {:?} {} {:?} {:?} {}",
        t.line_total,
        t.allowance_total,
        t.charge_total,
        t.taxable_total,
        t.vat_total,
        t.gross_total,
        t.paid,
        t.rounding,
        t.due
    )];
    out.extend(
        inv.lines
            .iter()
            .map(|l| format!("{} {}", l.id, l.net_amount)),
    );
    out.extend(
        inv.allowances
            .iter()
            .map(|a| format!("BG-20 {} {:?}", a.amount, a.base_amount)),
    );
    out.extend(inv.vat_breakdown.iter().map(|r| {
        format!(
            "BG-23 {} {} {}",
            r.category.as_str(),
            r.taxable_amount,
            r.tax_amount
        )
    }));
    out
}

#[test]
fn the_billing_invoice_writes_to_both_syntaxes_without_loss() {
    let inv = common::billing_invoice();
    let report = validate(&inv);
    assert!(report.is_valid(), "{report}");

    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let written = en16931_formats::write_unchecked(&inv, syntax).expect("writable");
        assert!(
            written.entries.is_empty(),
            "{syntax} reported {:?}",
            written.entries
        );
        let back = en16931_formats::read(written.xml.as_bytes()).expect("reads back");
        assert_eq!(back.syntax, syntax);
        assert!(back.entries.is_empty(), "{syntax}: {:?}", back.entries);
        assert_eq!(amounts(&back.invoice), amounts(&inv), "{syntax}");
    }
}
