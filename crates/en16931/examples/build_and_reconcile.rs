//! Build an invoice from **lines alone** and let the crate derive the rest.
//!
//! ```sh
//! cargo run --example build_and_reconcile
//! ```
//!
//! The [`Reconciler`] derives BG-23 and BG-22 from the lines: two `S` rates
//! give two groups, both `AE` lines share one (`BR-AE-01`), and VAT is computed
//! per group rather than summed per line. The `BR-AE-10` exemption reason
//! cannot be derived, so it is configured.

use en16931::invoice::{Party, PartyRole, PostalAddress};
use en16931::reconcile::Reconciler;
use en16931::{
    Date, Identifier, Invoice, InvoiceAmount, InvoiceLine, Percentage, Quantity, profiles, validate,
};
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).expect("a two-decimal amount")
}

fn party(name: &str, eas: &str, address: &str, street: &str, city: &str, post_code: &str) -> Party {
    Party::new(
        name,
        PostalAddress::new(street, city, post_code, "DE").expect("DE is a country"),
    )
    // The EAS scheme is checked here, not later by validation.
    .with_electronic_address(
        Identifier::eas(address, eas).unwrap_or_else(|e| panic!("{name}: {e}")),
    )
}

fn main() {
    // ── What a profile will ask for, before anything is built ────────────────
    println!("XRechnung wants these of a buyer, before we build anything:");
    for gap in Party::default().missing_for(&profiles::XRECHNUNG, PartyRole::Buyer) {
        println!("  {gap}");
    }

    // ── The lines ────────────────────────────────────────────────────────────
    let seller = party(
        "Stadtwerke Musterstadt GmbH",
        "0088", // GLN
        "4012345000009",
        "Hauptstraße 1",
        "Musterstadt",
        "12345",
    )
    .with_vat_identifier("DE123456789");
    let buyer = party(
        "Beispiel AG",
        "0204", // DE:LWID (Leitweg-ID)
        "991-01234-56",
        "Marktplatz 2",
        "Beispielstadt",
        "54321",
    )
    .with_vat_identifier("DE987654321");

    let invoice = Invoice::builder(
        "urn:cen.eu:en16931:2017",
        "RE-2026-0042",
        Date::new(2026, 7, 31).expect("a real date"),
        "380",
        "EUR",
    )
    .seller(seller)
    .buyer(buyer)
    // BR-CO-25: a due date or payment terms once something is owed.
    .due_in_days(14)
    .payment_terms("Zahlbar innerhalb 14 Tagen ohne Abzug")
    .line(InvoiceLine::new(
        "1",
        "Netznutzung Arbeitspreis",
        Quantity::new(dec!(10000)),
        "KWH",
        amount("2890.00"),
        "S",
        Some(Percentage::new(dec!(19))),
    ))
    .line(InvoiceLine::new(
        "2",
        "Messstellenbetrieb",
        Quantity::new(dec!(12)),
        "MON",
        amount("120.00"),
        "S",
        Some(Percentage::new(dec!(7))), // a second rate → a second BG-23 group
    ))
    .line(InvoiceLine::new(
        "3",
        "Bauleistung (§ 13b UStG)",
        Quantity::new(dec!(1)),
        "C62",
        amount("500.00"),
        "AE",
        Some(Percentage::ZERO),
    ))
    .line(InvoiceLine::new(
        "4",
        "Bauleistung, zweite Position",
        Quantity::new(dec!(1)),
        "C62",
        amount("250.00"),
        "AE",
        // `BR-AE-05`: the line rate must be 0, not absent. All `AE` lines form
        // one group regardless of rate (`BR-AE-01`).
        Some(Percentage::ZERO),
    ))
    .build_reconciled_with(&Reconciler::new().exemption("AE", None, Some("VATEX-EU-AE")))
    .expect("the lines reconcile");

    // ── What came out ────────────────────────────────────────────────────────
    println!("\nBG-23, derived from the lines:");
    for e in &invoice.vat_breakdown {
        println!(
            "  {:<3} @ {:>5}  base {:>9}  tax {:>8}{}",
            e.category.as_str(),
            e.rate.map_or_else(|| "—".into(), |r| format!("{r} %")),
            e.taxable_amount,
            e.tax_amount,
            e.exemption_reason_code
                .as_ref()
                .map_or(String::new(), |c| format!("   ({c})")),
        );
    }

    let t = &invoice.totals;
    println!("\nBG-22:");
    println!("  BT-106 line total        {:>10}", t.line_total);
    println!("  BT-109 total without VAT {:>10}", t.taxable_total);
    println!(
        "  BT-110 total VAT         {:>10}",
        t.vat_total.expect("stated whenever there is a breakdown")
    );
    println!("  BT-112 total with VAT    {:>10}", t.gross_total);
    println!("  BT-115 amount due        {:>10}", t.due);
    println!(
        "  BT-107 / BT-108          {} / {}   (absent is not zero)",
        t.allowance_total.map_or("absent".into(), |v| v.to_string()),
        t.charge_total.map_or("absent".into(), |v| v.to_string()),
    );

    // ── Validation ───────────────────────────────────────────────────────────
    let report = validate(&invoice);
    println!("\n{report}");
    assert!(report.is_valid(), "a reconciled invoice still gets checked");

    // XRechnung's findings are the ones the pre-flight listed.
    let xr = profiles::XRECHNUNG.validate(&invoice);
    println!(
        "\nAgainst XRechnung 3.0: {} rule(s) checked, {} still owed",
        xr.rules_checked(),
        xr.fatal().count()
    );
    for f in xr.fatal().take(5) {
        println!("  {f}");
    }
}
