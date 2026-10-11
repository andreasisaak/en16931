//! Write one invoice as UBL **and** as CII, from a typed proof.
//!
//! ```sh
//! cargo run --example write_both_syntaxes --features cii
//! ```
//!
//! * `en16931_formats::write` takes a `Validated<P>` and stamps BT-24 from the
//!   proved profile.
//! * `Written::entries` names every term the target syntax had no place for
//!   (UBL's `<CreditNote>` has no `cbc:DueDate`; CII's document element does).
//! * `en16931_formats::convert` reads, writes, and reports what did not cross.

use en16931::invoice::*;
use en16931::profiles::En16931;
use en16931::validation::profile::Validated;
use en16931::{Date, Invoice, InvoiceAmount, InvoiceLine, Percentage, Quantity};
use en16931_formats::Syntax;
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).expect("a two-decimal amount")
}

fn build() -> Invoice {
    Invoice::builder(
        "urn:cen.eu:en16931:2017",
        "RE-2026-0042",
        Date::new(2026, 1, 15).expect("a real date"),
        Code::new("380"),
        Code::new("EUR"),
    )
    .due_date(Date::new(2026, 2, 15).expect("a real date"))
    .seller(party("Seller GmbH", Some("DE123456789")))
    .buyer(party("Buyer AG", None))
    .line(InvoiceLine::new(
        "1",
        "Widget",
        Quantity::new(dec!(2)),
        "C62",
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
    .build()
}

fn party(name: &str, vat: Option<&str>) -> Party {
    let party = Party::new(
        name,
        PostalAddress::new("Marienplatz 1", "München", "80331", "DE").expect("DE"),
    );
    match vat {
        Some(v) => party.with_vat_identifier(v),
        None => party,
    }
}

fn main() {
    let invoice = build();

    // No proof, no document.
    let proof: Validated<En16931> = match Validated::new(invoice.clone()) {
        Ok(p) => p,
        Err(rejected) => {
            let report = rejected.report();
            eprintln!("not valid, so nothing is written:");
            for f in report.findings() {
                eprintln!("  {} — {}", f.rule, f.path);
            }
            return;
        }
    };

    let out = en16931_formats::write(&proof, Syntax::Ubl);
    show("UBL", &out);

    #[cfg(feature = "cii")]
    {
        let out = en16931_formats::write(&proof, Syntax::Cii);
        show("CII", &out);

        // Convert the UBL to CII and read both back: the models must agree.
        let ubl = en16931_formats::write(&proof, Syntax::Ubl).xml;
        let converted =
            en16931_formats::convert(ubl.as_bytes(), Syntax::Cii, en16931_formats::Loss::Refuse)
                .expect("nothing is lost between the syntaxes");
        let via_ubl = en16931_formats::read(ubl.as_bytes()).expect("ubl").invoice;
        let via_cii = en16931_formats::read(converted.document.as_bytes())
            .expect("cii")
            .invoice;
        println!(
            "UBL and CII agree about the invoice: {}",
            via_ubl == via_cii
        );
    }

    // A credit note is where the syntaxes differ. It was never validated as
    // one, so it goes through `write_unchecked`.
    let mut credit_note = invoice;
    credit_note.kind = en16931::DocumentKind::CreditNote;
    credit_note.type_code = Some(Code::new("381"));
    let out = en16931_formats::write_unchecked(&credit_note, Syntax::Ubl)
        .expect("a seller, a buyer and a line: UBL has a form for it");
    println!("\ncredit note as UBL:");
    for entry in &out.entries {
        println!("  {entry}");
    }
    println!("  (UBL's <CreditNote> has no cbc:DueDate, so BT-9 has nowhere to go —");
    println!("   reported rather than discarded quietly)");
}

/// The first lines of a document, and what the writer had to say about it.
fn show(name: &str, out: &en16931_formats::Written) {
    println!("── {name} ({} bytes) ──", out.xml.len());
    for line in out.xml.lines().take(12) {
        println!("{line}");
    }
    println!("  …\nentries: {}", out.entries.len());
    for entry in &out.entries {
        println!("  {entry}");
    }
    println!();
}
