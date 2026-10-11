//! Valid invoices to start from: the eight worked examples of EN 16931-1
//! Annex A, and one invoice per shipped profile.
//!
//! Take a valid one, change one field, and look at the finding it causes:
//!
//! ```
//! use en16931::{examples, profiles};
//!
//! let mut invoice = examples::for_profile(&profiles::XRECHNUNG).expect("shipped");
//! assert!(profiles::XRECHNUNG.validate(&invoice).is_valid());
//!
//! invoice.buyer_reference = None;                       // drop the Leitweg-ID
//! let report = profiles::XRECHNUNG.validate(&invoice);
//! assert_eq!(report.fatal().map(|f| f.rule.as_str()).collect::<Vec<_>>(), ["BR-DE-15"]);
//! ```
//!
//! Every call returns a fresh value; nothing here is shared or cached.
//!
//! Annex A states only lines, breakdowns and totals. Every Annex A example adds
//! the same invented header, a German seller and a Dutch buyer with VAT
//! identifiers, and a due date (`BR-CO-25`). Reasons required by `BR-38`,
//! `BR-42` or `BR-44` but missing from the annex are documented per example.

use rust_decimal::Decimal;

use crate::invoice::{
    Contact, DocumentAllowanceCharge, InvoiceBuilder, Item, ItemAttribute, LineAllowanceCharge,
    LineVat, Party, PaymentInstructions, Period, PostalAddress, PriceDetails,
};
use crate::profiles;
use crate::{
    Date, Identifier, Invoice, InvoiceAmount, InvoiceLine, Percentage, Profile, Quantity,
    Reconciler, UnitPriceAmount,
};

/// One of the eight worked examples of EN 16931-1:2017+A1:2019 Annex A.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnexExample {
    /// The clause — `A.1.2` … `A.1.9`.
    pub clause: &'static str,
    /// The standard's title, as printed — `Example 1 (Different Invoiced item
    /// VAT rates)`.
    pub title: &'static str,
    /// The example as a model value, its totals reconciled from its lines.
    pub invoice: Invoice,
}

/// The eight worked examples of Annex A, in the standard's order.
///
/// Totals come from [`Reconciler`]. Where the annex contradicts itself, the
/// example holds the figure the rules require:
///
/// * **A.1.4** prints BT-152 = 25 % in its calculation table for the 12,5 %
///   line its own description and breakdown state; the example carries 12,5 %.
/// * **A.1.5** prints BT-106 = 286,63 for lines of 222,50 and 64,12, carries
///   the extra cent into BT-109, BT-112 and BT-115, and prints the zero-rated
///   group's base as 0 beside a 15,00 zero-rated charge. The example holds
///   286,62 / 272,96 / 337,45 and a zero-rated base of 15,00 — which is what
///   `BR-CO-10` and `BR-Z-08` require.
///
/// ```
/// let examples = en16931::examples::annex_a();
/// assert_eq!(examples.len(), 8);
/// for e in &examples {
///     assert!(en16931::validate(&e.invoice).is_valid(), "{}", e.clause);
/// }
/// assert_eq!(examples[0].invoice.totals.gross_total.to_string(), "175.80");
/// ```
#[must_use]
pub fn annex_a() -> [AnnexExample; 8] {
    [
        example_1(),
        example_2(),
        example_3(),
        example_4(),
        example_5(),
        example_6(),
        example_7(),
        example_8(),
    ]
}

/// A valid invoice declaring `profile` — `Some` for every profile in
/// [`profiles::ALL`], `None` for the ZUGFeRD profiles (nothing is written for
/// them).
///
/// A German domestic invoice (two parties with address, VAT identifier and
/// electronic address, a SEPA credit transfer, an invoicing period, one line
/// at 19 %) plus what the profile requires: for XRechnung and Peppol BT-10 and
/// BT-23; for XRechnung the seller contact; for its CVD variant a contract and
/// tender reference and a vehicle line.
///
/// ```
/// use en16931::{examples, profiles};
///
/// for p in profiles::ALL {
///     let invoice = examples::for_profile(p).expect("every shipped profile");
///     assert_eq!(invoice.specification_id.as_deref(), Some(p.specification_id));
///     assert!(p.validate(&invoice).is_valid(), "{}", p.id);
/// }
/// assert!(examples::for_profile(&profiles::ZUGFERD_BASIC).is_none());
/// ```
#[must_use]
pub fn for_profile(profile: &Profile) -> Option<Invoice> {
    let shipped = profiles::ALL.iter().any(|p| core::ptr::eq(*p, profile));
    if !shipped {
        return None;
    }
    let xrechnung = profile.slug.starts_with("xrechnung");
    let cvd = core::ptr::eq(profile, &profiles::XRECHNUNG_CVD);
    let peppol = core::ptr::eq(profile, &profiles::PEPPOL_BIS_3);

    let mut seller = Party::new(
        "Stadtwerke Musterstadt GmbH",
        address("Hauptstraße 1", "Musterstadt", "12345", "DE"),
    )
    .with_vat_identifier("DE123456789")
    .with_electronic_address(eas("4012345000009", "0088"));
    if xrechnung {
        // BR-DE-2, BR-DE-5 … BR-DE-7: XRechnung wants all three.
        seller = seller.with_contact(Contact::new(
            "Erika Mustermann",
            "+49 30 1234567",
            "rechnung@stadtwerke-musterstadt.example",
        ));
    }
    let buyer = Party::new(
        "Beispiel AG",
        address("Marktplatz 2", "Beispielstadt", "54321", "DE"),
    )
    .with_vat_identifier("DE987654321")
    .with_electronic_address(eas("991-01234-56", "0204"));

    let mut line = InvoiceLine::new(
        "1",
        "Netznutzung Arbeitspreis",
        Quantity::new(Decimal::from(10_000)),
        "KWH",
        InvoiceAmount::from_minor_units(289_000),
        "S",
        Some(Percentage::new(Decimal::from(19))),
    );
    if cvd {
        // BR-DE-CVD-03 … -05: a CVD classification and the `cva` attribute.
        line = InvoiceLine::new(
            "1",
            "Elektrobus, 12 m",
            Quantity::new(Decimal::ONE),
            "C62",
            InvoiceAmount::from_minor_units(45_000_000),
            "S",
            Some(Percentage::new(Decimal::from(19))),
        )
        .with_item(Item {
            name: Some("Elektrobus, 12 m".into()),
            classification_identifiers: vec![Identifier::schemed("M3", "CVD")],
            attributes: vec![ItemAttribute {
                name: Some("cva".into()),
                value: Some("zero-emission".into()),
            }],
            ..Item::default()
        });
    }

    let mut b = Invoice::builder(
        profile.specification_id,
        "RE-2026-0001",
        date(2026, 7, 31),
        "380",
        "EUR",
    )
    .seller(seller)
    .buyer(buyer)
    .payment(PaymentInstructions::sepa_credit_transfer(
        "DE02120300000000202051",
        Some("BYLADEM1001"),
    ))
    .due_in_days(14)
    // BR-DE-TMP-32 asks for a delivery date or period.
    .invoicing_period(Period {
        start: Some(date(2026, 7, 1)),
        end: Some(date(2026, 7, 31)),
    })
    .line(line);
    if xrechnung || peppol {
        // BR-DE-15 (the Leitweg-ID); Peppol's R003 wants BT-10 or BT-13.
        b = b
            .buyer_reference("991-01234-56")
            // PEPPOL-EN16931-R001.
            .business_process("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0");
    }
    if cvd {
        b = b.contract_reference("V-2026-17");
    }
    let mut invoice = b.build_reconciled().expect("the example reconciles");
    if cvd {
        invoice.tender_reference = Some("VERGABE-2026-03".into());
    }
    Some(invoice)
}

// ── Annex A ───────────────────────────────────────────────────────────────────

/// A.1.2 — eleven bottles of wine at two VAT rates: five at 12,00 (25 %), and a
/// case of six at 90,00 (12 %). Two BG-23 groups.
fn example_1() -> AnnexExample {
    let invoice = annex_header("A.1.2")
        .line(line("1", "Wine - bottles", 5, 0, "XBO", 6_000, "S", 25, 0))
        .line(line(
            "2",
            "Wine - case of 6",
            1,
            0,
            "XCS",
            9_000,
            "S",
            12,
            0,
        ))
        .build_reconciled()
        .expect("A.1.2 reconciles");
    annex(
        "A.1.2",
        "Example 1 (Different Invoiced item VAT rates)",
        invoice,
    )
}

/// A.1.3 — 10 000 screws priced per 1 000 (BT-149): 4,50 per thousand.
fn example_2() -> AnnexExample {
    let invoice = annex_header("A.1.3")
        .line(
            line("1", "Screw", 10_000, 0, "H87", 4_500, "S", 25, 0).with_price(PriceDetails {
                net_price: unit_price(450, 2),
                base_quantity: Some(Quantity::new(Decimal::from(1_000))),
                base_quantity_code: Some("H87".into()),
                ..PriceDetails::default()
            }),
        )
        .build_reconciled()
        .expect("A.1.3 reconciles");
    annex("A.1.3", "Example 2 (Item price base quantity)", invoice)
}

/// A.1.4 — one chicken invoiced by weight: 1,3 kg at 9,50 (10,00 less a 0,50
/// price discount), 12,5 % VAT.
fn example_3() -> AnnexExample {
    let invoice = annex_header("A.1.4")
        .line(
            line("1", "Chicken", 13, 1, "KGM", 1_235, "S", 125, 1).with_price(PriceDetails {
                net_price: unit_price(950, 2),
                price_discount: Some(unit_price(50, 2)),
                gross_price: Some(unit_price(1_000, 2)),
                ..PriceDetails::default()
            }),
        )
        .build_reconciled()
        .expect("A.1.4 reconciles");
    annex(
        "A.1.4",
        "Example 3 (Invoiced quantity unit of measure)",
        invoice,
    )
}

/// A.1.5 — pens with a line charge, paper with a 5 % line allowance, a 10 %
/// document allowance and a zero-rated freight charge.
///
/// The annex gives no reason for the line charge or the line allowance;
/// `BR-44` and `BR-42` require one, so they carry `"Handling"` and
/// `"Discount"`. See [`annex_a`] for the figures the annex misprints.
fn example_4() -> AnnexExample {
    let mut pens = line("1", "Pens", 25, 0, "XCS", 22_250, "S", 25, 0).with_price(PriceDetails {
        net_price: unit_price(850, 2),
        price_discount: Some(unit_price(100, 2)),
        gross_price: Some(unit_price(950, 2)),
        ..PriceDetails::default()
    });
    pens.charges.push(LineAllowanceCharge {
        amount: cents(1_000),
        base_amount: None,
        percentage: None,
        reason: Some("Handling".into()),
        reason_code: None,
    });
    let mut paper = line("2", "Paper", 15, 0, "XPK", 6_412, "S", 25, 0).with_price(PriceDetails {
        net_price: unit_price(450, 2),
        ..PriceDetails::default()
    });
    paper.allowances.push(LineAllowanceCharge {
        amount: cents(338),
        base_amount: Some(cents(6_750)),
        percentage: Some(Percentage::new(Decimal::from(5))),
        reason: Some("Discount".into()),
        reason_code: None,
    });
    let invoice = annex_header("A.1.5")
        .line(pens)
        .line(paper)
        .allowance(DocumentAllowanceCharge {
            amount: cents(2_866),
            base_amount: Some(cents(28_662)),
            percentage: Some(Percentage::new(Decimal::from(10))),
            vat: vat("S", 25, 0),
            reason: Some("Volume discount".into()),
            reason_code: None,
        })
        .charge(DocumentAllowanceCharge {
            amount: cents(1_500),
            base_amount: None,
            percentage: None,
            vat: vat("Z", 0, 0),
            reason: Some("Freight charge".into()),
            reason_code: None,
        })
        .build_reconciled()
        .expect("A.1.5 reconciles");
    annex(
        "A.1.5",
        "Example 4 (Discounts, allowances and charges)",
        invoice,
    )
}

/// A.1.6 — 25 cases of pens, and 10 returned from an earlier delivery credited
/// on the same invoice: the sign is on the quantity (BT-129 = −10), never on
/// the price (`BR-27`).
fn example_5() -> AnnexExample {
    let price = || PriceDetails {
        net_price: unit_price(850, 2),
        price_discount: Some(unit_price(100, 2)),
        gross_price: Some(unit_price(950, 2)),
        ..PriceDetails::default()
    };
    let invoice = annex_header("A.1.6")
        .line(line("1", "Pens", 25, 0, "XCS", 21_250, "S", 25, 0).with_price(price()))
        .line(line("2", "Pens", -10, 0, "XCS", -8_500, "S", 25, 0).with_price(price()))
        .build_reconciled()
        .expect("A.1.6 reconciles");
    annex("A.1.6", "Example 5 (Negative Invoice line)", invoice)
}

/// A.1.7 — the last rate of a car rental, 110,00, against a 250,00 deposit:
/// BT-115 is −112,50, a lawful refund.
fn example_6() -> AnnexExample {
    let invoice = annex_header("A.1.7")
        .line(line("1", "Car rental", 1, 0, "EA", 11_000, "S", 25, 0))
        .build_reconciled_with(&Reconciler::new().paid(cents(25_000)))
        .expect("A.1.7 reconciles");
    annex(
        "A.1.7",
        "Example 6 (Prepayment and negative Amount due for payment)",
        invoice,
    )
}

/// A.1.8 — two standard rates (25 % and 10 %), two exempt lines with reason
/// "Reason A", a document charge ("Reason B") and allowance ("Reason C") at
/// 25 %. Three BG-23 groups.
fn example_7() -> AnnexExample {
    let invoice = annex_header("A.1.8")
        .line(line("1", "Printer ink", 1, 0, "C62", 12_500, "S", 25, 0))
        .line(line("2", "Poster print", 1, 0, "C62", 2_400, "S", 10, 0))
        .line(line("3", "Office chair", 1, 0, "C62", 13_600, "S", 25, 0))
        .line(line(
            "4",
            "Wireless keyboard",
            1,
            0,
            "C62",
            9_500,
            "E",
            0,
            0,
        ))
        .line(line(
            "5",
            "Drive Adapter Cable",
            1,
            0,
            "C62",
            5_300,
            "E",
            0,
            0,
        ))
        .charge(DocumentAllowanceCharge {
            amount: cents(3_500),
            base_amount: None,
            percentage: None,
            vat: vat("S", 25, 0),
            reason: Some("Reason B".into()),
            reason_code: None,
        })
        .allowance(DocumentAllowanceCharge {
            amount: cents(1_500),
            base_amount: None,
            percentage: None,
            vat: vat("S", 25, 0),
            reason: Some("Reason C".into()),
            reason_code: None,
        })
        .build_reconciled_with(&Reconciler::new().exemption("E", Some("Reason A"), None))
        .expect("A.1.8 reconciles");
    annex(
        "A.1.8",
        "Example 7 (Standard VAT including VAT exempted lines)",
        invoice,
    )
}

/// A.1.9 — two reverse-charge services and a reverse-charge document charge.
///
/// The annex gives the charge no reason; `BR-38` requires one, so it carries
/// `"Charge"`.
fn example_8() -> AnnexExample {
    let invoice = annex_header("A.1.9")
        .line(line("1", "Service 1", 1, 0, "C62", 12_500, "AE", 0, 0))
        .line(line("2", "Service 2", 1, 0, "C62", 2_400, "AE", 0, 0))
        .charge(DocumentAllowanceCharge {
            amount: cents(2_000),
            base_amount: None,
            percentage: None,
            vat: vat("AE", 0, 0),
            reason: Some("Charge".into()),
            reason_code: None,
        })
        .build_reconciled_with(&Reconciler::new().exemption("AE", Some("Reverse charge"), None))
        .expect("A.1.9 reconciles");
    annex(
        "A.1.9",
        "Example 8 (Reverse Charge, Intra EU supply and Export Invoices)",
        invoice,
    )
}

// ── Shared parts ──────────────────────────────────────────────────────────────

fn annex(clause: &'static str, title: &'static str, invoice: Invoice) -> AnnexExample {
    AnnexExample {
        clause,
        title,
        invoice,
    }
}

/// The invented header and parties every Annex A example carries.
fn annex_header(number: &str) -> InvoiceBuilder {
    Invoice::builder(
        profiles::EN16931.specification_id,
        number,
        date(2026, 6, 30),
        "380",
        "EUR",
    )
    .seller(
        Party::new(
            "Seller GmbH",
            address("Hauptstraße 1", "Musterstadt", "12345", "DE"),
        )
        .with_vat_identifier("DE123456789"),
    )
    .buyer(
        Party::new(
            "Buyer BV",
            address("Kerkstraat 1", "Amsterdam", "1011 AB", "NL"),
        )
        .with_vat_identifier("NL123456789B01"),
    )
    .due_in_days(30)
}

/// A line whose net price is its quotient — `quantity` and `rate` are
/// `mantissa × 10^-scale`, `net` is in cents.
#[allow(clippy::too_many_arguments)]
fn line(
    id: &str,
    name: &str,
    quantity: i64,
    quantity_scale: u32,
    unit: &str,
    net: i64,
    category: &str,
    rate: i64,
    rate_scale: u32,
) -> InvoiceLine {
    let vat = vat(category, rate, rate_scale);
    InvoiceLine::new(
        id,
        name,
        Quantity::new(Decimal::new(quantity, quantity_scale)),
        unit,
        cents(net),
        vat.category,
        vat.rate,
    )
}

fn vat(category: &str, rate: i64, scale: u32) -> LineVat {
    LineVat {
        category: category.into(),
        rate: Some(Percentage::new(Decimal::new(rate, scale))),
    }
}

fn cents(n: i64) -> InvoiceAmount {
    InvoiceAmount::from_minor_units(n)
}

fn unit_price(mantissa: i64, scale: u32) -> UnitPriceAmount {
    UnitPriceAmount::new(Decimal::new(mantissa, scale))
}

fn date(y: i32, m: u8, d: u8) -> Date {
    Date::new(y, m, d).expect("a calendar date")
}

fn address(street: &str, city: &str, post_code: &str, country: &str) -> PostalAddress {
    PostalAddress::new(street, city, post_code, country).expect("an ISO 3166-1 country")
}

fn eas(content: &str, scheme: &str) -> Identifier {
    Identifier::eas(content, scheme).expect("a live EAS scheme and a well-formed value")
}
