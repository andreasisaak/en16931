//! Build an invoice, validate it, and read the report.
//!
//! ```sh
//! cargo run --example validate_an_invoice
//! ```
//!
//! A finding names a business term (`BG-25[1]/BT-151`), not an XPath.

use en16931::invoice::*;
use en16931::{Date, Invoice, InvoiceAmount, InvoiceLine, Percentage, Quantity, validate};
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).expect("a two-decimal amount")
}

fn main() {
    let invoice = Invoice::builder(
        "urn:cen.eu:en16931:2017",
        "RE-2026-0042",
        Date::new(2026, 1, 15).expect("a real date"),
        Code::new("380"), // commercial invoice
        Code::new("EUR"),
    )
    .seller(
        Party::new(
            "Seller GmbH",
            PostalAddress::new("Hauptstraße 1", "München", "80331", "DE").expect("DE"),
        )
        .with_vat_identifier("DE123456789"),
    )
    .buyer(Party::new(
        "Buyer AG",
        PostalAddress::new("Jungfernstieg 1", "Hamburg", "20354", "DE").expect("DE"),
    ))
    .line(InvoiceLine::new(
        "1",
        "Widget",
        Quantity::new(dec!(2)),
        "C62", // "one" / piece
        amount("200.00"),
        "S",
        Some(Percentage::new(dec!(19))),
    ))
    .vat_breakdown(VatBreakdown {
        taxable_amount: amount("200.00"),
        tax_amount: amount("38.00"),
        category: Code::new("S"),
        rate: Some(Percentage::new(dec!(19))),
        exemption_reason: None,
        exemption_reason_code: None,
    })
    .totals(DocumentTotals {
        line_total: amount("200.00"),
        taxable_total: amount("200.00"),
        vat_total: Some(amount("38.00")),
        gross_total: amount("238.00"),
        due: amount("238.00"),
        ..DocumentTotals::default()
    })
    .build();

    report("as built", &invoice);

    // BR-CO-25: a positive amount due needs a due date or payment terms.
    let mut fixed = invoice;
    fixed.due_date = Some(Date::new(2026, 2, 15).expect("a real date"));
    report("with BT-9 supplied", &fixed);
}

fn report(label: &str, invoice: &Invoice) {
    let report = validate(invoice);

    println!("── {label} ──");
    println!("valid:         {}", report.is_valid());
    println!("rules checked: {}", report.rules_checked());
    println!("findings:      {}\n", report.findings().len());

    for f in report.findings() {
        println!("  [{}] {} — {}", f.severity, f.rule, f.path);
        println!("      {}", f.message);
        if let Some(d) = &f.detail {
            println!("      expected {}, found {}", d.expected, d.actual);
        }
    }

    if report.is_valid() {
        println!("  (nothing to report)");
    }
    println!();
}
